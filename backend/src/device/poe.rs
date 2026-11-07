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
        neighbors::{PortRef, port_named},
        raw::{Row, command, field, rows},
    },
    topology::{
        PoeMode,
        access::device::DeviceAccess,
        findings::{InterfaceRef, interface_ref},
        natural::naturalize_interface,
    },
};
use async_graphql::{Enum, SimpleObject};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
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
    /// `/interface/ethernet/print`: a port with PoE out has `poe-out`
    pub ethernet: Vec<Row>,
    /// `/interface/ethernet/poe/monitor` of those ports
    pub monitor: Vec<Row>,
}

/// Reads the Ethernet ports and the state of their PoE out.
pub async fn read_ports(client: &RawDevice<SimpleResult>) -> Result<PoePorts, Error> {
    let ethernet = rows(client, "/interface/ethernet/print").await?;
    let with_poe = ethernet
        .iter()
        .filter(|row| field(row, "poe-out").is_some())
        .filter_map(|row| field(row, "name"))
        .collect::<Vec<_>>();
    let monitor = match with_poe.is_empty() {
        true => Vec::new(),
        false => {
            command(
                client,
                "/interface/ethernet/poe/monitor",
                &[("numbers", &with_poe.join(",")), ("once", "")],
            )
            .await?
        }
    };
    Ok(PoePorts { ethernet, monitor })
}

/// Reads which ports of the device feed something now, and what NetBox and the neighbors say is
/// at their other end.
pub async fn observed(
    client: &RawDevice<SimpleResult>,
    device: &DeviceAccess,
) -> Result<Vec<PoeObserved>, Error> {
    let ports = read_ports(client).await?;
    if ports.monitor.is_empty() {
        return Ok(Vec::new());
    }
    let neighbors = rows(client, "/ip/neighbor/print").await?;
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
    pub status: Option<Box<str>>,
}

/// A PoE-capable Ethernet port, modeled to feed or fed in NetBox or reporting `powered-on`,
/// with what the device says now. For the network map's PSE→PD arrows
/// (`network_state::graphql`, `docs/concepts/network-map.md` step 4) and, filtered to a
/// problem, [`mismatches`].
#[derive(Debug, Clone, PartialEq)]
pub struct Feeding {
    pub port: InterfaceRef,
    pub mode: Option<PoeMode>,
    /// `poe-out-status` as the device reports it
    pub status: Option<Box<str>>,
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
            let row = ports
                .ethernet
                .iter()
                .find(|row| field(row, "default-name") == Some(interface.name()))?;
            let status = field(row, "name").and_then(|name| {
                ports
                    .monitor
                    .iter()
                    .find(|m| field(m, "name") == Some(name))
                    .and_then(|m| field(m, "poe-out-status"))
            });
            let power = field(row, "name").and_then(|name| {
                ports
                    .monitor
                    .iter()
                    .find(|m| field(m, "name") == Some(name))
                    .and_then(|m| field(m, "poe-out-power"))
                    .and_then(|v| v.parse().ok())
            });
            let far = interface.connected_interfaces().into_iter().next();
            if mode.is_none() && status != Some("powered-on") {
                return None;
            }
            let problem = match (mode, field(row, "poe-out").is_some(), status) {
                (Some(PoeMode::Pse), false, _) => Some(PoeProblem::NoPoeOut),
                (Some(PoeMode::Pse), true, Some("waiting-for-load")) if far.is_some() => {
                    Some(PoeProblem::NoLoad)
                }
                (Some(PoeMode::Pse), true, Some("powered-on"))
                    if far
                        .as_ref()
                        .is_some_and(|far| far.poe() != Some(PoeMode::Pd)) =>
                {
                    Some(PoeProblem::PdMissing)
                }
                (Some(PoeMode::Pd), _, Some("powered-on")) => Some(PoeProblem::FeedsInstead),
                (None, _, Some("powered-on")) => Some(PoeProblem::Undocumented),
                _ => None,
            };
            Some(Feeding {
                port: interface_ref(interface),
                mode,
                status: status.map(Box::from),
                power,
                far: far.as_ref().map(interface_ref),
                problem,
            })
        })
        .collect();
    found.sort_by_cached_key(|f| naturalize_interface(&f.port.name));
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
                status: f.status,
            }),
        })
        .collect()
}

/// [`observed`] on what the device answered: `ethernet` from `/interface/ethernet/print`,
/// `monitor` from the PoE monitor, `neighbors` from `/ip/neighbor/print`.
fn compare(
    device: &DeviceAccess,
    ethernet: &[Row],
    monitor: &[Row],
    neighbors: &[Row],
) -> Vec<PoeObserved> {
    let default_names = ethernet
        .iter()
        .filter_map(|row| Some((field(row, "name")?, field(row, "default-name")?)))
        .collect::<HashMap<_, _>>();
    let devices = device.topology().list_devices().collect::<Vec<_>>();
    let mut observed: Vec<PoeObserved> = monitor
        .iter()
        .filter(|row| field(row, "poe-out-status") == Some("powered-on"))
        .filter_map(|row| field(row, "name"))
        .map(|name| {
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
                    .filter(|row| {
                        field(row, "interface").is_some_and(|i| i.split(',').any(|i| i == name))
                    })
                    .find_map(|row| {
                        let identity = field(row, "identity")?;
                        let far_device = devices.iter().find(|d| d.name() == identity)?;
                        port_named(far_device, field(row, "interface-name")?)
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
    fn row(fields: &[(&str, &str)]) -> Row {
        fields
            .iter()
            .map(|(k, v)| (Box::from(*k), Some(Box::from(*v))))
            .collect()
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
            row(&[
                ("name", "e02-ap"),
                ("default-name", "ether2"),
                ("poe-out", "auto-on"),
            ]),
            row(&[
                ("name", "ether3"),
                ("default-name", "ether3"),
                ("poe-out", "auto-on"),
            ]),
            row(&[
                ("name", "ether4"),
                ("default-name", "ether4"),
                ("poe-out", "auto-on"),
            ]),
        ];
        let monitor = [
            row(&[("name", "e02-ap"), ("poe-out-status", "powered-on")]),
            row(&[("name", "ether3"), ("poe-out-status", "powered-on")]),
            row(&[("name", "ether4"), ("poe-out-status", "waiting-for-load")]),
        ];
        let neighbors = [row(&[
            ("interface", "ether3,bridge"),
            ("identity", "cam"),
            ("interface-name", "eth0"),
        ])];
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
        let with_poe = |name: &str, renamed: &str| {
            row(&[
                ("name", renamed),
                ("default-name", name),
                ("poe-out", "auto-on"),
            ])
        };
        let status = |name: &str, status: &str| row(&[("name", name), ("poe-out-status", status)]);
        let ports_read = PoePorts {
            ethernet: vec![
                row(&[("name", "ether2"), ("default-name", "ether2")]),
                with_poe("ether3", "e03-cam"),
                with_poe("ether4", "ether4"),
                with_poe("ether5", "ether5"),
                with_poe("ether6", "ether6"),
                with_poe("ether7", "ether7"),
            ],
            monitor: vec![
                status("e03-cam", "waiting-for-load"),
                status("ether4", "powered-on"),
                status("ether5", "powered-on"),
                status("ether6", "waiting-for-load"),
                status("ether7", "powered-on"),
            ],
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
        let with_poe = |name: &str| {
            row(&[
                ("name", name),
                ("default-name", name),
                ("poe-out", "auto-on"),
            ])
        };
        let status = |name: &str, status: &str| row(&[("name", name), ("poe-out-status", status)]);
        let ports = PoePorts {
            ethernet: vec![with_poe("ether2"), with_poe("ether3")],
            monitor: vec![
                status("ether2", "powered-on"),
                status("ether3", "powered-on"),
            ],
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
}
