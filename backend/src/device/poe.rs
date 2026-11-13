//! Scope: Reading PoE from a RouterOS device and comparing it with NetBox's modes, for the
//! helper „PoE“, the network map and the live checks; not planning NetBox changes
//! (`helper::poe`).
//!
//! The live check (`mismatches`) compares NetBox's modes with what a RouterOS device reports: a
//! `pse` port without `poe-out`, one where nothing draws power, a `pd` port the device feeds at.
//! Whether a port takes PoE in RouterOS doesn't report, and of other devices nothing is known.
use crate::{
    Error,
    device::{
        neighbors::{PortRef, Seen, port_named},
        ros::hw_facts,
    },
    topology::{
        PoeMode,
        access::device::DeviceAccess,
        findings::{InterfaceRef, interface_ref},
        natural::naturalize_interface,
    },
};
use InterfaceEthernetPoePoeOutStatus as Status;
use async_graphql::{Enum, SimpleObject};
use mikrotik_model::{
    command::Reads,
    model::{
        InterfaceEthernet, InterfaceEthernetPoeMonitor, InterfaceEthernetPoePoeOutStatus,
        IpNeighborState,
    },
    resource::{collect_monitor_once, collect_resource},
};
use std::collections::HashMap;

/// A port the device reports feeding something (`poe-out-status=powered-on`), and the port fed
/// as far as it is known: NetBox's cable, else the neighbor the device sees there
/// (`from_neighbor`).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoeObserved {
    pub port: PortRef,
    pub far: Option<InterfaceRef>,
    pub from_neighbor: bool,
}

/// What the device reports of its Ethernet ports and their PoE out.
#[derive(Debug, Clone, Default)]
pub struct PoePorts {
    /// the Ethernet ports (`/interface/ethernet`)
    pub ethernet: Box<[EthernetPort]>,
    /// `/interface/ethernet/poe/monitor` of those with PoE out
    pub monitor: Box<[InterfaceEthernetPoeMonitor]>,
}

/// An Ethernet port of the device.
#[derive(Debug, Clone)]
pub struct EthernetPort {
    pub name: Box<str>,
    /// its factory name, NetBox's
    pub default_name: Box<str>,
    /// it has PoE out (`poe-out`)
    pub poe_out: bool,
}

impl From<&InterfaceEthernet> for EthernetPort {
    fn from(port: &InterfaceEthernet) -> Self {
        EthernetPort {
            name: port.cfg.name.to_string().into(),
            default_name: port.status.default_name.to_string().into(),
            poe_out: port.cfg.poe_out.is_some(),
        }
    }
}

/// `poe-out-status` (`InterfaceEthernetPoePoeOutStatus`), for GraphQL.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum PoeOutStatus {
    Disabled,
    WaitingForLoad,
    ShortCircuit,
    PoweredOn,
    Overload,
    VoltageTooLow,
    VoltageTooHigh,
    CurrentTooLow,
    VoltageOnPoeIn,
    Off,
    PowerReset,
    ControllerInit,
    ControllerUpgrade,
    ControllerError,
}

impl From<&InterfaceEthernetPoePoeOutStatus> for PoeOutStatus {
    fn from(status: &InterfaceEthernetPoePoeOutStatus) -> Self {
        use InterfaceEthernetPoePoeOutStatus as S;
        match status {
            S::Disabled => Self::Disabled,
            S::WaitingForLoad => Self::WaitingForLoad,
            S::ShortCircuit => Self::ShortCircuit,
            S::PoweredOn => Self::PoweredOn,
            S::Overload => Self::Overload,
            S::VoltageTooLow => Self::VoltageTooLow,
            S::VoltageTooHigh => Self::VoltageTooHigh,
            S::CurrentTooLow => Self::CurrentTooLow,
            S::VoltageOnPoeIn => Self::VoltageOnPoeIn,
            S::Off => Self::Off,
            S::PowerReset => Self::PowerReset,
            S::ControllerInit => Self::ControllerInit,
            S::ControllerUpgrade => Self::ControllerUpgrade,
            S::ControllerError => Self::ControllerError,
        }
    }
}

/// Reads the Ethernet ports and the state of their PoE out.
pub async fn read_ports(client: &impl Reads) -> Result<PoePorts, Error> {
    let ethernet = collect_resource::<InterfaceEthernet>(client).await?;
    // a port's entry in `/interface/ethernet/poe` has the port's id (lab-np2, 7.23.8)
    let with_poe = ethernet
        .iter()
        .filter(|port| port.cfg.poe_out.is_some())
        .map(|port| port.status.id)
        .collect::<Vec<_>>();
    let monitor =
        collect_monitor_once::<InterfaceEthernetPoeMonitor>(with_poe.into_iter(), client).await?;
    Ok(PoePorts {
        ethernet: ethernet.iter().map(EthernetPort::from).collect(),
        monitor,
    })
}

/// Reads which ports of the device feed something now, and what NetBox and the neighbors say is
/// at their other end.
pub async fn observed(
    client: &impl Reads,
    device: &DeviceAccess,
) -> Result<Vec<PoeObserved>, Error> {
    let ports = read_ports(client).await?;
    if ports.monitor.is_empty() {
        return Ok(Vec::new());
    }
    let neighbors = collect_resource::<IpNeighborState>(client)
        .await?
        .iter()
        .map(Seen::from)
        .collect::<Vec<_>>();
    Ok(compare(device, &ports.ethernet, &ports.monitor, &neighbors))
}

/// Where the device contradicts NetBox's PoE mode of a port.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum PoeProblem {
    /// NetBox has it feed (`pse`), the port has no PoE out
    NoPoeOut,
    /// NetBox has it feed and a cable there, nothing at the other end draws power
    /// (`waiting-for-load`): the device there takes no PoE, has a supply of its own, or is off.
    /// Without a cable the port waits for its device.
    NoLoad,
    /// it feeds (`powered-on`, `pse` in NetBox), the port at its cable's far end isn't `pd`:
    /// „PoE vervollständigen“
    PdMissing,
    /// NetBox has it fed (`pd`), the device feeds there (`powered-on`)
    FeedsInstead,
    /// it feeds (`powered-on`), NetBox has no PoE mode there at all: the network map shows it
    /// (`network_state::graphql`), [`mismatches`] doesn't (unchanged from before this existed)
    Undocumented,
}

/// A port whose PoE mode in NetBox the device contradicts.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoeMismatch {
    pub port: InterfaceRef,
    pub problem: PoeProblem,
    /// `poe-out-status` as the device reports it
    pub status: Option<PoeOutStatus>,
}

/// A PoE-capable Ethernet port, modeled to feed or fed in NetBox or reporting `powered-on`,
/// with what the device says now. For the network map's PSE→PD arrows
/// (`network_state::graphql`, `docs/network-map.md` step 4) and, filtered to a
/// problem, [`mismatches`].
#[derive(Debug, Clone, PartialEq)]
pub struct Feeding {
    pub port: InterfaceRef,
    pub mode: Option<PoeMode>,
    /// `poe-out-status` as the device reports it
    pub status: Option<InterfaceEthernetPoePoeOutStatus>,
    /// `poe-out-power`, watts; only while `status` is `powered-on`
    pub power: Option<f64>,
    /// the port at its cable's far end, if NetBox has one
    pub far: Option<InterfaceRef>,
    pub problem: Option<PoeProblem>,
}

/// Every Ethernet port of the device with a PoE mode in NetBox, or reporting `powered-on`
/// without one, by NetBox's name, which is the port's factory name (`default-name`). A port the
/// device doesn't have is left to the other checks.
pub fn feeding(device: &DeviceAccess, ports: &PoePorts) -> Vec<Feeding> {
    let mut found: Vec<Feeding> = device
        .interfaces()
        .iter()
        .filter(|interface| interface.is_ethernet() && interface.external_port().is_some())
        .filter_map(|interface| {
            let mode = interface.poe();
            let port = ports
                .ethernet
                .iter()
                .find(|port| *port.default_name == *interface.name())?;
            let reading = ports
                .monitor
                .iter()
                .find(|m| *m.name.0 == *port.name.as_bytes());
            let status = reading.map(|m| m.poe_out_status);
            let power = reading.and_then(|m| m.poe_out_power);
            let far = interface.connected_interfaces().into_iter().next();
            if mode.is_none() && status != Some(Status::PoweredOn) {
                return None;
            }
            let problem = match (mode, port.poe_out, status) {
                (Some(PoeMode::Pse), false, _) => Some(PoeProblem::NoPoeOut),
                (Some(PoeMode::Pse), true, Some(Status::WaitingForLoad)) if far.is_some() => {
                    Some(PoeProblem::NoLoad)
                }
                (Some(PoeMode::Pse), true, Some(Status::PoweredOn))
                    if far
                        .as_ref()
                        .is_some_and(|far| far.poe() != Some(PoeMode::Pd)) =>
                {
                    Some(PoeProblem::PdMissing)
                }
                (Some(PoeMode::Pd), _, Some(Status::PoweredOn)) => Some(PoeProblem::FeedsInstead),
                (None, _, Some(Status::PoweredOn)) => Some(PoeProblem::Undocumented),
                _ => None,
            };
            Some(Feeding {
                port: interface_ref(interface),
                mode,
                status,
                power,
                far: far.as_ref().map(interface_ref),
                problem,
            })
        })
        .collect();
    found.sort_by_cached_key(|f| naturalize_interface(&f.port.name));
    found
}

/// What a group of PoE out ports ([`hw_facts::PoeGroup`]) draws at one voltage class, against
/// its limit.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PoeLoad {
    /// the group's first and last port, by NetBox's name (`ether1`, `ether8`)
    pub first: Box<str>,
    pub last: Box<str>,
    /// the ports run above [`hw_facts::POE_HIGH_VOLTAGE`]
    pub high_voltage: bool,
    /// the sum of `poe-out-current` of the ports feeding at this voltage, A
    pub current: f64,
    /// the board's limit for the group at this voltage, A; none where the board feeds none at it
    pub limit: Option<f64>,
}

/// The load of each PoE out group of `board` ([`hw_facts::poe_groups`]), per voltage class a
/// port reports (a netPower switches it per port): one entry per class with a port feeding,
/// none for a group feeding nothing or a board without entered limits.
pub fn load(board: &[u8], ports: &PoePorts) -> Vec<PoeLoad> {
    let mut found = Vec::new();
    for group in hw_facts::poe_groups(board) {
        let readings = ports
            .ethernet
            .iter()
            .filter(|port| {
                port.default_name
                    .strip_prefix("ether")
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| group.ports.contains(&n))
            })
            .filter_map(|port| {
                ports
                    .monitor
                    .iter()
                    .find(|m| *m.name.0 == *port.name.as_bytes())
            })
            .filter(|m| m.poe_out_status == Status::PoweredOn)
            .collect::<Vec<_>>();
        for high_voltage in [false, true] {
            let class = readings
                .iter()
                .filter(|m| {
                    m.poe_out_voltage
                        .is_some_and(|v| v >= hw_facts::POE_HIGH_VOLTAGE)
                        == high_voltage
                })
                .collect::<Vec<_>>();
            if class.is_empty() {
                continue;
            }
            let milliamps = if high_voltage {
                group.high
            } else {
                Some(group.low)
            };
            found.push(PoeLoad {
                first: format!("ether{}", group.ports.start()).into(),
                last: format!("ether{}", group.ports.end()).into(),
                high_voltage,
                current: class.iter().filter_map(|m| m.poe_out_current).sum::<f64>() / 1000.0,
                limit: milliamps.map(|ma| f64::from(ma) / 1000.0),
            });
        }
    }
    found
}

/// [`feeding`], filtered to those with a [`PoeProblem`] (`Undocumented` excluded: it didn't
/// exist as a check before the network map, and still isn't one here).
pub fn mismatches(device: &DeviceAccess, ports: &PoePorts) -> Vec<PoeMismatch> {
    feeding(device, ports)
        .into_iter()
        .filter_map(|f| match f.problem {
            None | Some(PoeProblem::Undocumented) => None,
            Some(problem) => Some(PoeMismatch {
                port: f.port,
                problem,
                status: f.status.as_ref().map(PoeOutStatus::from),
            }),
        })
        .collect()
}

/// [`observed`] on what the device answered: `ethernet` from `/interface/ethernet/print`,
/// `monitor` from the PoE monitor, `neighbors` from `/ip/neighbor/print`.
fn compare(
    device: &DeviceAccess,
    ethernet: &[EthernetPort],
    monitor: &[InterfaceEthernetPoeMonitor],
    neighbors: &[Seen],
) -> Vec<PoeObserved> {
    let default_names = ethernet
        .iter()
        .map(|port| (&*port.name, &*port.default_name))
        .collect::<HashMap<_, _>>();
    let devices = device.topology().list_devices().collect::<Vec<_>>();
    let mut observed: Vec<PoeObserved> = monitor
        .iter()
        .filter(|m| m.poe_out_status == Status::PoweredOn)
        .map(|m| m.name.to_string())
        .map(|name| {
            let name = name.as_str();
            let default_name = default_names.get(name).copied().unwrap_or(name);
            let interface = device
                .interfaces()
                .into_iter()
                .find(|i| i.external_port().is_some() && i.name() == default_name);
            let cabled = interface
                .as_ref()
                .and_then(|i| i.connected_interfaces().into_iter().next());
            let seen = || {
                neighbors
                    .iter()
                    .filter(|seen| seen.interface.split(',').any(|i| i == name))
                    .find_map(|seen| {
                        let far_device = devices.iter().find(|d| d.name() == &*seen.identity)?;
                        port_named(far_device, &seen.interface_name)
                    })
            };
            let (far, from_neighbor) = match cabled {
                Some(far) => (Some(far), false),
                None => {
                    let seen = seen();
                    let from_neighbor = seen.is_some();
                    (seen, from_neighbor)
                }
            };
            PoeObserved {
                port: PortRef {
                    name: name.into(),
                    interface: interface.as_ref().map(interface_ref),
                },
                far: far.as_ref().map(interface_ref),
                from_neighbor,
            }
        })
        .collect();
    observed.sort_by_cached_key(|o| naturalize_interface(&o.port.name));
    observed
}

/// A reading of the PoE monitor from the fields RouterOS reports (`poe-out` `auto-on` unless
/// given), for tests.
#[cfg(test)]
pub(crate) fn monitor_reading(fields: &[(&str, &str)]) -> InterfaceEthernetPoeMonitor {
    crate::device::answer::parsed_with(fields, &[("poe-out", "auto-on")])
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::topology::{
        Cable, CablePort, Device, DeviceId, Interface, InterfaceId, PhysicalPortId, PortType,
        builder::TopologyBuilder,
    };
    use std::{str::FromStr, sync::Arc};

    fn device(builder: &mut TopologyBuilder, name: &str) -> DeviceId {
        let id = builder.devices.next_id();
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                has_routeros: name == "sw",
                ..Device::default()
            },
        );
        id
    }
    fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str) -> InterfaceId {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external: PhysicalPortId::from_str(name).ok(),
                port_type: Some(PortType::Ethernet),
                ..Interface::default()
            },
        );
        id
    }
    fn ethernet(name: &str, default_name: &str, poe_out: bool) -> EthernetPort {
        EthernetPort {
            name: name.into(),
            default_name: default_name.into(),
            poe_out,
        }
    }
    fn status(name: &str, status: &str) -> InterfaceEthernetPoeMonitor {
        monitor_reading(&[("name", name), ("poe-out-status", status)])
    }

    /// `sw` feeds on `ether2` (renamed, cabled to `ap` in NetBox) and `ether3` (no cable, `cam`
    /// seen there); `ether4` only waits for a load.
    #[test]
    fn powered_ports_with_their_far_end() {
        let mut builder = TopologyBuilder::default();
        let sw = device(&mut builder, "sw");
        let ap = device(&mut builder, "ap");
        let cam = device(&mut builder, "cam");
        let sw2 = port(&mut builder, sw, "ether2");
        let sw3 = port(&mut builder, sw, "ether3");
        port(&mut builder, sw, "ether4");
        let ap1 = port(&mut builder, ap, "ether1");
        let cam1 = port(&mut builder, cam, "eth0");
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(sw2)]),
                port_b: Box::new([CablePort::Interface(ap1)]),
                kind: None,
            },
        );
        let topology = Arc::new(builder.build());
        let sw = topology.get_device_by_id(&sw).unwrap();
        let ethernet = [
            ethernet("e02-ap", "ether2", true),
            ethernet("ether3", "ether3", true),
            ethernet("ether4", "ether4", true),
        ];
        let monitor = [
            status("e02-ap", "powered-on"),
            status("ether3", "powered-on"),
            status("ether4", "waiting-for-load"),
        ];
        let neighbors = [Seen {
            interface: "ether3,bridge".into(),
            identity: "cam".into(),
            interface_name: "eth0".into(),
            board: "".into(),
        }];
        let observed = compare(&sw, &ethernet, &monitor, &neighbors);
        let seen = observed
            .iter()
            .map(|o| {
                (
                    &*o.port.name,
                    o.port.interface.as_ref().map(|i| i.id),
                    o.far.as_ref().map(|i| i.id),
                    o.from_neighbor,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            seen,
            [
                ("e02-ap", Some(sw2.0), Some(ap1.0), false),
                ("ether3", Some(sw3.0), Some(cam1.0), true),
            ]
        );
    }

    fn cable(builder: &mut TopologyBuilder, a: InterfaceId, b: InterfaceId) {
        let id = builder.cables.next_id();
        builder.cables.insert(
            id,
            Cable {
                port_a: Box::new([CablePort::Interface(a)]),
                port_b: Box::new([CablePort::Interface(b)]),
                kind: None,
            },
        );
    }

    /// `sw` has `pse` on ether2 (no PoE out there), ether3 (cabled to `cam`, nothing drawing
    /// power), ether4 (feeding `ap`, whose port isn't `pd`), ether6 (no cable, waiting for its
    /// device), ether7 (feeding `ap2`'s `pd`) and `pd` on ether5, where it feeds.
    #[test]
    fn mismatches_with_the_device() {
        let mut builder = TopologyBuilder::default();
        let sw = device(&mut builder, "sw");
        let names = ["ether2", "ether3", "ether4", "ether5", "ether6", "ether7"];
        let ports = names.map(|name| port(&mut builder, sw, name));
        let modes = [
            PoeMode::Pse,
            PoeMode::Pse,
            PoeMode::Pse,
            PoeMode::Pd,
            PoeMode::Pse,
            PoeMode::Pse,
        ];
        for (port, mode) in ports.iter().zip(modes) {
            builder.interfaces.get_mut(port).unwrap().poe = Some(mode);
        }
        for (name, at) in [("cam", 1), ("ap", 2), ("ap2", 5)] {
            let far_device = device(&mut builder, name);
            let far = port(&mut builder, far_device, "ether1");
            if name == "ap2" {
                builder.interfaces.get_mut(&far).unwrap().poe = Some(PoeMode::Pd);
            }
            cable(&mut builder, ports[at], far);
        }
        let topology = Arc::new(builder.build());
        let sw = topology.get_device_by_id(&sw).unwrap();
        let with_poe = |name: &str, renamed: &str| ethernet(renamed, name, true);
        let ports_read = PoePorts {
            ethernet: vec![
                ethernet("ether2", "ether2", false),
                with_poe("ether3", "e03-cam"),
                with_poe("ether4", "ether4"),
                with_poe("ether5", "ether5"),
                with_poe("ether6", "ether6"),
                with_poe("ether7", "ether7"),
            ]
            .into(),
            monitor: vec![
                status("e03-cam", "waiting-for-load"),
                status("ether4", "powered-on"),
                status("ether5", "powered-on"),
                status("ether6", "waiting-for-load"),
                status("ether7", "powered-on"),
            ]
            .into(),
        };
        let found = mismatches(&sw, &ports_read)
            .into_iter()
            .map(|m| (m.port.id, m.problem))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                (ports[0].0, PoeProblem::NoPoeOut),
                (ports[1].0, PoeProblem::NoLoad),
                (ports[2].0, PoeProblem::PdMissing),
                (ports[3].0, PoeProblem::FeedsInstead),
            ]
        );
    }

    /// A port feeding without any PoE mode in NetBox at all is `Undocumented` in [`feeding`],
    /// but [`mismatches`] still says nothing about it, same as before `feeding` existed.
    #[test]
    fn feeding_reports_what_mismatches_leaves_out() {
        let mut builder = TopologyBuilder::default();
        let sw = device(&mut builder, "sw");
        let modeled = port(&mut builder, sw, "ether2");
        let unmodeled = port(&mut builder, sw, "ether3");
        builder.interfaces.get_mut(&modeled).unwrap().poe = Some(PoeMode::Pse);
        let topology = Arc::new(builder.build());
        let sw = topology.get_device_by_id(&sw).unwrap();
        let with_poe = |name: &str| ethernet(name, name, true);
        let ports = PoePorts {
            ethernet: vec![with_poe("ether2"), with_poe("ether3")].into(),
            monitor: vec![
                status("ether2", "powered-on"),
                status("ether3", "powered-on"),
            ]
            .into(),
        };
        let found = feeding(&sw, &ports)
            .into_iter()
            .map(|f| (f.port.id, f.problem))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                (modeled.0, None),
                (unmodeled.0, Some(PoeProblem::Undocumented))
            ]
        );
        assert!(
            mismatches(&sw, &ports).is_empty(),
            "neither is a mismatch mismatches() would report"
        );
    }

    /// lab-np2 (netPower 16P, two PSUs): lab-wap at 47.7 V and lab-hex at 24 V in the first
    /// group, lab-sxt-b in the second; a port waiting for a load draws nothing.
    #[test]
    fn a_netpower_group_draws_per_voltage() {
        let ports = PoePorts {
            ethernet: vec![
                ethernet("e03-lab-wap", "ether3", true),
                ethernet("e05-hex", "ether5", true),
                ethernet("e07", "ether7", true),
                ethernet("e13-lab-sxt-b", "ether13", true),
            ]
            .into(),
            monitor: vec![
                monitor_reading(&[
                    ("name", "e03-lab-wap"),
                    ("poe-out-status", "powered-on"),
                    ("poe-out-voltage", "47.7"),
                    ("poe-out-current", "52"),
                ]),
                monitor_reading(&[
                    ("name", "e05-hex"),
                    ("poe-out-status", "powered-on"),
                    ("poe-out-voltage", "24.1"),
                    ("poe-out-current", "150"),
                ]),
                monitor_reading(&[("name", "e07"), ("poe-out-status", "waiting-for-load")]),
                monitor_reading(&[
                    ("name", "e13-lab-sxt-b"),
                    ("poe-out-status", "powered-on"),
                    ("poe-out-voltage", "24.0"),
                    ("poe-out-current", "200"),
                ]),
            ]
            .into(),
        };
        let found = load(b"CRS318-16P-2S+", &ports)
            .into_iter()
            .map(|l| (l.first, l.high_voltage, l.current, l.limit))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                ("ether1".into(), false, 0.15, Some(2.8)),
                ("ether1".into(), true, 0.052, Some(1.4)),
                ("ether9".into(), false, 0.2, Some(2.8)),
            ]
        );
    }

    /// A board without entered limits, or one feeding nothing, has no load to show.
    #[test]
    fn no_limits_no_load() {
        let ports = PoePorts {
            ethernet: vec![ethernet("e01", "ether1", true)].into(),
            monitor: vec![monitor_reading(&[
                ("name", "e01"),
                ("poe-out-status", "powered-on"),
                ("poe-out-voltage", "24.0"),
                ("poe-out-current", "100"),
            ])]
            .into(),
        };
        assert!(load(b"RB5009UPr+S+", &ports).is_empty());
        let idle = PoePorts {
            ethernet: ports.ethernet.clone(),
            monitor: Box::default(),
        };
        assert!(load(b"C52iG-5HaxD2HaxD", &idle).is_empty());
    }
}
