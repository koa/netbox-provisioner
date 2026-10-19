//! What a device sees at its ports (`/ip/neighbor`: MNDP, LLDP, CDP) against the cables NetBox
//! has there (the Befund „NetBox gegen Gerät“ of `docs/concepts/config-helper.md`, neighbors →
//! cables). Read-only. A neighbor is matched by its identity (the NetBox name the generated
//! config gives it) and the name of its port, never by MAC.
//!
//! Over a radio bridge a port sees the router on the other side, not the bridge device (its
//! L2 bridge doesn't announce itself there), so a cable to a bridge device is only checked
//! when the device sees something NetBox has nowhere near that port. A radio port sees across
//! the air (walter-schopf's `wlan60-1` saw router-schopf behind the other end), where NetBox has
//! a wireless link, not a cable: no cable is checked there.
use crate::{
    Error,
    device::raw::{Row, field, rows},
    topology::{
        DeviceRole,
        PhysicalPortId,
        access::{AccessTopology, device::DeviceAccess, interface::InterfaceAccess},
        findings::{DeviceRef, InterfaceRef, Severity, device_ref, interface_ref},
    },
};
use async_graphql::{SimpleObject, Union};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NeighborFinding {
    pub severity: Severity,
    pub kind: NeighborFindingKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Union)]
pub enum NeighborFindingKind {
    CableMissing(CableMissing),
    CableElsewhere(CableElsewhere),
    OtherFarPort(OtherFarPort),
    NeighborNotSeen(NeighborNotSeen),
    NeighborsNotInNetbox(NeighborsNotInNetbox),
    DiscoveryOff(DiscoveryOff),
}

/// A port of the device, as it names it, and the NetBox interface it is (by default name);
/// `None` for a port NetBox doesn't have.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PortRef {
    pub name: Box<str>,
    pub interface: Option<InterfaceRef>,
}

/// A neighbor as the device reports it, and what it is in NetBox where its identity is a
/// device there.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct SeenNeighbor {
    pub identity: Box<str>,
    /// its port, as it names it
    pub interface_name: Box<str>,
    pub board: Box<str>,
    pub device: Option<DeviceRef>,
    pub interface: Option<InterfaceRef>,
}

/// NetBox has no cable at `port`, and the device sees devices NetBox knows there.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CableMissing {
    pub port: PortRef,
    pub seen: Vec<SeenNeighbor>,
}
/// NetBox's cable at `port` goes to `far_end`, the device sees others there and not it.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CableElsewhere {
    pub port: PortRef,
    pub far_end: InterfaceRef,
    pub seen: Vec<SeenNeighbor>,
}
/// The device at the other end is the one of NetBox's cable, but on another port.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct OtherFarPort {
    pub port: PortRef,
    pub far_end: InterfaceRef,
    pub seen: SeenNeighbor,
}
/// NetBox has a cable at `port` to a RouterOS device, the device sees nothing there (off, or
/// the cable is unplugged).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NeighborNotSeen {
    pub port: PortRef,
    pub far_end: InterfaceRef,
}
/// What the device sees at `port` is not in NetBox (customer devices, or a name that differs).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NeighborsNotInNetbox {
    pub port: PortRef,
    pub seen: Vec<SeenNeighbor>,
}

/// The device discovers on no interface: `/ip/neighbor/discovery-settings` refers to the
/// interface list `list` that doesn't exist (`*2000011`, a list an apply removed), so it sees
/// nobody and nothing about its cables can be said.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DiscoveryOff {
    pub list: Box<str>,
}

/// Reads the device's ports and neighbors and compares them with NetBox.
pub async fn check(
    client: &RawDevice<SimpleResult>,
    device: &DeviceAccess,
) -> Result<Vec<NeighborFinding>, Error> {
    let settings = rows(client, "/ip/neighbor/discovery-settings/print").await?;
    let lists = rows(client, "/interface/list/print").await?;
    if let Some(list) = settings
        .first()
        .and_then(|row| field(row, "discover-interface-list"))
        && !lists.iter().any(|row| field(row, "name") == Some(list))
    {
        return Ok(vec![NeighborFinding {
            severity: Severity::Error,
            kind: NeighborFindingKind::DiscoveryOff(DiscoveryOff { list: list.into() }),
        }]);
    }
    let interfaces = rows(client, "/interface/print").await?;
    let neighbors = rows(client, "/ip/neighbor/print").await?;
    Ok(compare(device, &interfaces, &neighbors))
}

/// [`check`] on what the device answered: `interfaces` from `/interface/print`, `neighbors`
/// from `/ip/neighbor/print`.
pub fn compare(device: &DeviceAccess, interfaces: &[Row], neighbors: &[Row]) -> Vec<NeighborFinding> {
    let topology_devices = device.topology().list_devices().collect::<Vec<_>>();
    let default_names = interfaces
        .iter()
        .filter_map(|row| Some((field(row, "name")?, field(row, "default-name")?)))
        .collect::<HashMap<_, _>>();
    let netbox_ports = device
        .interfaces()
        .into_iter()
        .filter(|i| i.external_port().is_some())
        .map(|i| (i.name().to_string(), i))
        .collect::<BTreeMap<_, _>>();

    // what the device sees, by the default name of its port (its own name where it has none)
    let mut seen_at = BTreeMap::<String, (Box<str>, Vec<SeenNeighbor>)>::new();
    for row in neighbors {
        let interface = field(row, "interface").unwrap_or_default();
        let Some((name, default_name)) = interface
            .split(',')
            .filter_map(|name| {
                let default_name = default_names.get(name).copied().unwrap_or(name);
                netbox_ports
                    .contains_key(default_name)
                    .then_some((name, default_name))
            })
            .next()
            .or_else(|| {
                // a port NetBox doesn't have: the first one with a default name
                interface
                    .split(',')
                    .find_map(|name| default_names.get(name).map(|default| (name, *default)))
            })
        else {
            continue;
        };
        let identity = field(row, "identity").unwrap_or_default();
        let interface_name = field(row, "interface-name").unwrap_or_default();
        let netbox_device = topology_devices.iter().find(|d| d.name() == identity);
        let netbox_interface = netbox_device.and_then(|d| port_named(d, interface_name));
        seen_at
            .entry(default_name.to_string())
            .or_insert_with(|| (name.into(), Vec::new()))
            .1
            .push(SeenNeighbor {
                identity: identity.into(),
                interface_name: interface_name.into(),
                board: field(row, "board").unwrap_or_default().into(),
                device: netbox_device.map(device_ref),
                interface: netbox_interface.as_ref().map(interface_ref),
            });
    }

    let mut findings = Vec::new();
    for (default_name, (name, seen)) in &seen_at {
        let interface = netbox_ports.get(default_name);
        let port = PortRef {
            name: name.clone(),
            interface: interface.map(interface_ref),
        };
        let far_ends = interface
            .map(InterfaceAccess::connected_interfaces)
            .unwrap_or_default();
        let (known, unknown): (Vec<_>, Vec<_>) =
            seen.iter().cloned().partition(|s| s.device.is_some());
        if !unknown.is_empty() {
            findings.push(NeighborFinding {
                severity: Severity::Hint,
                kind: NeighborFindingKind::NeighborsNotInNetbox(NeighborsNotInNetbox {
                    port: port.clone(),
                    seen: unknown,
                }),
            });
        }
        if known.is_empty() || interface.is_some_and(is_radio) {
            continue;
        }
        if far_ends.is_empty() {
            findings.push(NeighborFinding {
                severity: Severity::Warning,
                kind: NeighborFindingKind::CableMissing(CableMissing { port, seen: known }),
            });
            continue;
        }
        let matching = far_ends.iter().find_map(|far_end| {
            let far_device = far_end.device()?;
            known
                .iter()
                .find(|s| s.device.as_ref().is_some_and(|d| d.id == far_device.id().0))
                .map(|s| (far_end, s))
        });
        match matching {
            Some((far_end, seen)) => {
                if seen.interface.as_ref().is_none_or(|i| i.id != far_end.id().0) {
                    findings.push(NeighborFinding {
                        severity: Severity::Error,
                        kind: NeighborFindingKind::OtherFarPort(OtherFarPort {
                            port,
                            far_end: interface_ref(far_end),
                            seen: seen.clone(),
                        }),
                    });
                }
            }
            None if far_ends.iter().all(is_on_bridge_device) => {}
            None => findings.push(NeighborFinding {
                severity: Severity::Error,
                kind: NeighborFindingKind::CableElsewhere(CableElsewhere {
                    port,
                    far_end: interface_ref(&far_ends[0]),
                    seen: known,
                }),
            }),
        }
    }
    for (default_name, interface) in &netbox_ports {
        if seen_at.contains_key(default_name) {
            continue;
        }
        let Some(far_end) = interface.connected_interfaces().into_iter().find(|far_end| {
            far_end.device().is_some_and(|d| d.has_routeros()) && !is_on_bridge_device(far_end)
        }) else {
            continue;
        };
        let name = interface
            .interface_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| default_name.clone());
        findings.push(NeighborFinding {
            severity: Severity::Hint,
            kind: NeighborFindingKind::NeighborNotSeen(NeighborNotSeen {
                port: PortRef {
                    name: name.into(),
                    interface: Some(interface_ref(interface)),
                },
                far_end: interface_ref(&far_end),
            }),
        });
    }
    findings.sort_by_key(|finding| finding.severity);
    findings
}

/// The port of `device` its RouterOS names `name`: NetBox's name (the default name), or the
/// one the generated config gives it, which starts with the port's short name (`s01-core`).
/// `bridge/ether2` and `ether2,bridge` name the port too.
pub(crate) fn port_named(device: &DeviceAccess, name: &str) -> Option<InterfaceAccess> {
    let names = name.split(['/', ',']).collect::<Vec<_>>();
    device.interfaces().into_iter().find(|interface| {
        let short = interface.external_port().map(|port| port.short_name().to_string());
        names.iter().any(|name| {
            interface.name() == *name
                || short.as_deref().is_some_and(|short| {
                    name.strip_prefix(short)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
                })
        })
    })
}

/// A radio port: what it sees is across the air.
fn is_radio(interface: &InterfaceAccess) -> bool {
    interface.has_wireless_link()
        || interface.radio().is_some()
        || matches!(
            interface.external_port(),
            Some(PhysicalPortId::Wifi(_) | PhysicalPortId::Wlan(_) | PhysicalPortId::W60g(_))
        )
}

/// A device that only bridges (a radio link end): it doesn't show up as neighbor of the port
/// it is cabled to.
fn is_on_bridge_device(interface: &InterfaceAccess) -> bool {
    interface.device().is_some_and(|device| {
        device.role() == Some(DeviceRole::WlanBridge)
            || device.interfaces().iter().any(|i| i.radio().is_some())
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::topology::{
        Cable, CablePort, CircuitTermination, Device, DeviceId, FrontPort, Interface,
        InterfaceId, PhysicalPortId, PortType, RearPort, builder::TopologyBuilder,
    };
    use std::{str::FromStr, sync::Arc};

    fn device(builder: &mut TopologyBuilder, name: &str, role: Option<DeviceRole>) -> DeviceId {
        let id = builder.devices.next_id();
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                has_routeros: true,
                role,
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
    fn row(fields: &[(&str, &str)]) -> Row {
        fields
            .iter()
            .map(|(k, v)| (Box::from(*k), Some(Box::from(*v))))
            .collect()
    }
    /// r1's ports as RouterOS names them: `ether1` renamed to `e01`, `ether2` as it is.
    fn interfaces() -> Vec<Row> {
        vec![
            row(&[("name", "e01"), ("default-name", "ether1")]),
            row(&[("name", "ether2"), ("default-name", "ether2")]),
            row(&[("name", "bridge")]),
        ]
    }
    fn seen(local: &str, identity: &str, remote: &str) -> Row {
        row(&[
            ("interface", local),
            ("identity", identity),
            ("interface-name", remote),
            ("board", "RB5009UG+S+"),
        ])
    }

    /// r1 ether1 -- r2 ether1, r2 has ether2 too; r3 has ether1.
    #[derive(Clone)]
    struct Net {
        builder: TopologyBuilder,
        r1: DeviceId,
        r1e1: InterfaceId,
        r2e1: InterfaceId,
    }
    fn net() -> Net {
        let mut builder = TopologyBuilder::default();
        let r1 = device(&mut builder, "r1", None);
        let r2 = device(&mut builder, "r2", None);
        let r3 = device(&mut builder, "r3", None);
        let r1e1 = port(&mut builder, r1, "ether1");
        port(&mut builder, r1, "ether2");
        let r2e1 = port(&mut builder, r2, "ether1");
        port(&mut builder, r2, "ether2");
        port(&mut builder, r3, "ether1");
        Net {
            builder,
            r1,
            r1e1,
            r2e1,
        }
    }
    impl Net {
        fn compare(self, neighbors: &[Row]) -> Vec<NeighborFindingKind> {
            let r1 = Arc::new(self.builder.build())
                .get_device_by_id(&self.r1)
                .unwrap();
            compare(&r1, &interfaces(), neighbors)
                .into_iter()
                .map(|finding| finding.kind)
                .collect()
        }
    }

    #[test]
    fn the_neighbor_of_the_cable_is_no_finding() {
        let mut net = net();
        cable(&mut net.builder, net.r1e1, net.r2e1);
        assert_eq!(net.clone().compare(&[seen("e01", "r2", "e01-r1")]), vec![]);
        assert_eq!(net.compare(&[seen("e01,bridge", "r2", "bridge/ether1")]), vec![]);
    }

    #[test]
    fn a_neighbor_without_cable_is_a_missing_cable() {
        let kinds = net().compare(&[seen("ether2", "r2", "ether2")]);
        assert!(matches!(&kinds[..], [NeighborFindingKind::CableMissing(c)]
            if &*c.port.name == "ether2"
                && c.seen[0].interface.as_ref().is_some_and(|i| &*i.name == "ether2")),
            "{kinds:#?}");
    }

    #[test]
    fn another_device_or_port_than_the_cable_is_an_error() {
        let mut net = net();
        cable(&mut net.builder, net.r1e1, net.r2e1);
        let kinds = net.clone().compare(&[seen("e01", "r3", "ether1")]);
        assert!(matches!(&kinds[..], [NeighborFindingKind::CableElsewhere(_)]), "{kinds:#?}");
        let kinds = net.compare(&[seen("e01", "r2", "ether2")]);
        assert!(matches!(&kinds[..], [NeighborFindingKind::OtherFarPort(o)]
            if &*o.seen.interface_name == "ether2"), "{kinds:#?}");
    }

    #[test]
    fn a_cabled_router_not_seen_is_a_hint() {
        let mut net = net();
        cable(&mut net.builder, net.r1e1, net.r2e1);
        let kinds = net.compare(&[]);
        assert!(matches!(&kinds[..], [NeighborFindingKind::NeighborNotSeen(n)]
            if &*n.far_end.device.name == "r2"), "{kinds:#?}");
    }

    #[test]
    fn neighbors_not_in_netbox_are_a_hint() {
        let kinds = net().compare(&[seen("ether2", "customer-ap", "eth0")]);
        assert!(matches!(&kinds[..], [NeighborFindingKind::NeighborsNotInNetbox(n)]
            if n.seen.len() == 1), "{kinds:#?}");
    }

    /// `port` -- front port of a panel, its rear port -- `end` of a circuit: a building's
    /// side of a fiber to another one.
    fn panel_to_circuit(builder: &mut TopologyBuilder, port: InterfaceId, end: CablePort) {
        let panel = builder.devices.next_id();
        builder.devices.insert(panel, Device { name: "panel".into(), ..Device::default() });
        let front = builder.front_ports.next_id();
        let rear = builder.rear_ports.next_id();
        builder.front_ports.insert(
            front,
            FrontPort { name: "1".into(), device: panel, rear_port: Some(rear), cable: None },
        );
        builder.rear_ports.insert(
            rear,
            RearPort { name: "1".into(), device: panel, front_port: Some(front), cable: None },
        );
        for (a, b) in [
            (CablePort::Interface(port), CablePort::FrontPort(front)),
            (CablePort::RearPort(rear), end),
        ] {
            let id = builder.cables.next_id();
            builder
                .cables
                .insert(id, Cable { port_a: Box::new([a]), port_b: Box::new([b]), kind: None });
        }
    }

    /// The cable path goes on at the other end of a circuit, as NetBox traces it: r1 ether1
    /// -- panel -- circuit -- panel -- r2 ether1 is a cable from r1 ether1 to r2 ether1.
    #[test]
    fn a_cable_path_goes_through_a_circuit() {
        let mut net = net();
        let a = net.builder.circuit_terminations.next_id();
        let z = net.builder.circuit_terminations.next_id();
        for (end, peer) in [(a, z), (z, a)] {
            net.builder.circuit_terminations.insert(
                end,
                CircuitTermination { circuit: "fiber-1".into(), peer: Some(peer), cable: None },
            );
        }
        panel_to_circuit(&mut net.builder, net.r1e1, CablePort::CircuitTermination(a));
        panel_to_circuit(&mut net.builder, net.r2e1, CablePort::CircuitTermination(z));
        assert_eq!(net.clone().compare(&[seen("e01", "r2", "e01-r1")]), vec![]);
        let kinds = net.compare(&[]);
        assert!(matches!(&kinds[..], [NeighborFindingKind::NeighborNotSeen(n)]
            if &*n.far_end.device.name == "r2"), "{kinds:#?}");
    }

    /// A radio port sees the router behind the other end of its link: no cable is missing.
    #[test]
    fn a_radio_port_needs_no_cable() {
        let mut net = net();
        port(&mut net.builder, net.r1, "wlan60-1");
        let seen = seen("wlan60-1", "r2", "ether1");
        assert_eq!(net.compare(&[seen]), vec![]);
    }

    /// A radio bridge doesn't announce itself, the router on the other side shows up instead.
    #[test]
    fn a_cable_to_a_bridge_device_sees_through_it() {
        let mut net = net();
        let bridge = device(&mut net.builder, "r1-r2", Some(DeviceRole::WlanBridge));
        let bridge_port = port(&mut net.builder, bridge, "ether1");
        cable(&mut net.builder, net.r1e1, bridge_port);
        assert_eq!(net.compare(&[seen("e01", "r2", "ether1")]), vec![]);
    }
}
