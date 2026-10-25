//! What a device sees at its ports (`/ip/neighbor`: MNDP, LLDP, CDP) against the cables NetBox
//! has there (the Befund „NetBox gegen Gerät“ of `docs/config-helper.md`, neighbors →
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
        access::{device::DeviceAccess, interface::InterfaceAccess},
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
    Ok(read(client, device).await?.findings)
}

/// What [`read`] found: the findings, and who the device sees at each NetBox interface (by id).
pub struct Neighbors {
    pub findings: Vec<NeighborFinding>,
    pub seen: BTreeMap<u32, Vec<SeenNeighbor>>,
}

/// [`check`] with who is seen where.
pub async fn read(client: &RawDevice<SimpleResult>, device: &DeviceAccess) -> Result<Neighbors, Error> {
    let settings = rows(client, "/ip/neighbor/discovery-settings/print").await?;
    let lists = rows(client, "/interface/list/print").await?;
    if let Some(list) = settings
        .first()
        .and_then(|row| field(row, "discover-interface-list"))
        && !lists.iter().any(|row| field(row, "name") == Some(list))
    {
        return Ok(Neighbors {
            findings: vec![NeighborFinding {
                severity: Severity::Error,
                kind: NeighborFindingKind::DiscoveryOff(DiscoveryOff { list: list.into() }),
            }],
            seen: BTreeMap::new(),
        });
    }
    let interfaces = rows(client, "/interface/print").await?;
    let neighbors = rows(client, "/ip/neighbor/print").await?;
    let (seen_at, netbox_ports) = seen_at(device, &interfaces, &neighbors);
    let seen = seen_at
        .iter()
        .filter_map(|(default_name, (_, seen))| {
            Some((netbox_ports.get(default_name)?.id().0, seen.clone()))
        })
        .collect();
    Ok(Neighbors {
        findings: compare(device, &interfaces, &neighbors),
        seen,
    })
}

/// Who the device sees at each port, by the port's default name (its own name where it has
/// none), with the port's own name.
type SeenAt = BTreeMap<String, (Box<str>, Vec<SeenNeighbor>)>;

/// [`SeenAt`], and NetBox's ports by name.
fn seen_at(
    device: &DeviceAccess,
    interfaces: &[Row],
    neighbors: &[Row],
) -> (SeenAt, BTreeMap<String, InterfaceAccess>) {
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
    let mut seen_at = SeenAt::new();
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
    (seen_at, netbox_ports)
}

/// [`check`] on what the device answered: `interfaces` from `/interface/print`, `neighbors`
/// from `/ip/neighbor/print`.
pub fn compare(device: &DeviceAccess, interfaces: &[Row], neighbors: &[Row]) -> Vec<NeighborFinding> {
    let (seen_at, netbox_ports) = seen_at(device, interfaces, neighbors);
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
mod test;
