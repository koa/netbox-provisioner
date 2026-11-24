//! Scope: A resident's drafts: what it changed but hasn't saved, one per element, and the
//! changes they are saved as; not showing them (`components::resident`), nor saving them
//! (`save_dialog`).
use super::View;
use crate::graphql::resident::{
    change::{
        ConnectDeviceChange, DeviceRoomChange, PortKindInput, PortVlanChange, ResidentChange,
        WlanInput,
    },
    residents::{PortKind, Resident, ResidentDevice, ResidentPort},
};
use std::collections::BTreeMap;

/// A row of the resident's ports: its id with its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortKey {
    pub kind: PortKindInput,
    pub id: i32,
}

impl PortKey {
    pub fn of(port: &ResidentPort) -> Self {
        PortKey {
            kind: match port.kind {
                PortKind::Socket => PortKindInput::Socket,
                _ => PortKindInput::Interface,
            },
            id: port.id,
        }
    }
}

/// Where the cable of one of the resident's devices goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkDraft {
    /// to the port row `port`, out of the device's port `device_port` (none: its Anschluss);
    /// `seen`: a router sees the cable there already
    Connect {
        port: PortKey,
        device_port: Option<i32>,
        seen: bool,
    },
    /// nowhere: taken off its link
    Disconnect,
}

impl LinkDraft {
    /// Whether the draft links its device to `port`.
    pub fn connects(&self, port: PortKey) -> bool {
        matches!(self, LinkDraft::Connect { port: at, .. } if *at == port)
    }

    /// Whether it is how the device is linked now (`current`): the same port row, and its
    /// Anschluss where it names one.
    pub fn is(&self, current: &LinkDraft) -> bool {
        match (self, current) {
            (LinkDraft::Disconnect, LinkDraft::Disconnect) => true,
            (
                LinkDraft::Connect {
                    port, device_port, ..
                },
                LinkDraft::Connect {
                    port: now,
                    device_port: uplink,
                    ..
                },
            ) => port == now && (device_port.is_none() || device_port == uplink),
            _ => false,
        }
    }
}

/// What a WLAN gets: only what differs from it now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WlanDraft {
    pub ssid: Option<String>,
    pub psk: Option<String>,
    pub enabled: Option<bool>,
}

impl WlanDraft {
    pub fn is_empty(&self) -> bool {
        self.ssid.is_none() && self.psk.is_none() && self.enabled.is_none()
    }
}

/// The resident's drafts, by the element they change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drafts {
    pub wlans: BTreeMap<i32, WlanDraft>,
    /// the network of a port row: one of the resident's VLANs, or none
    pub port_vlans: BTreeMap<PortKey, Option<i32>>,
    /// by device
    pub links: BTreeMap<i32, LinkDraft>,
    /// the room of a device, by device
    pub rooms: BTreeMap<i32, i32>,
}

impl Drafts {
    /// How many elements have a draft; a device's link and room are two.
    pub fn len(&self) -> usize {
        self.wlans.len() + self.port_vlans.len() + self.links.len() + self.rooms.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// How many of them the tab of `view` shows: the networks the WLANs, the other the rest.
    pub fn len_in(&self, view: View) -> usize {
        match view {
            View::Networks => self.wlans.len(),
            View::Ports => self.port_vlans.len() + self.links.len() + self.rooms.len(),
            View::Changes => 0,
        }
    }
    /// The device `device` linked as `link`, none where that's how it is linked `now`. A link to
    /// a port takes the port's network off (its draft would be undone), and its place: one
    /// cable per port, another device drafted there stays where it is.
    pub fn link(&mut self, device: i32, link: LinkDraft, now: Option<LinkDraft>) {
        match now.is_some_and(|now| link.is(&now)) {
            true => self.links.remove(&device),
            false => self.links.insert(device, link),
        };
        if let LinkDraft::Connect { port, .. } = link {
            self.port_vlans.remove(&port);
            self.links
                .retain(|other, draft| *other == device || !draft.connects(port));
        }
    }
    /// The port row as saved: its network, and no device coming to it.
    pub fn reset_port(&mut self, port: PortKey) {
        self.port_vlans.remove(&port);
        self.links.retain(|_, link| !link.connects(port));
    }
    /// The changes they are saved as, in an order in which each finds NetBox as it needs it:
    /// devices off their link first (that frees their ports), then the networks of ports, the
    /// devices linked, rooms and WLANs.
    pub fn changes(&self) -> Vec<ResidentChange> {
        let disconnects = self
            .links
            .iter()
            .filter(|(_, link)| matches!(link, LinkDraft::Disconnect))
            .map(|(device, _)| ResidentChange::DisconnectDevice(*device));
        let port_vlans = self.port_vlans.iter().map(|(port, vlan)| {
            ResidentChange::PortVlan(PortVlanChange {
                port: port.id,
                kind: port.kind,
                vlan: *vlan,
            })
        });
        let connects = self.links.iter().filter_map(|(device, link)| match link {
            LinkDraft::Connect {
                port, device_port, ..
            } => Some(ResidentChange::ConnectDevice(ConnectDeviceChange {
                device: *device,
                port: port.id,
                kind: port.kind,
                device_port: *device_port,
            })),
            LinkDraft::Disconnect => None,
        });
        let rooms = self.rooms.iter().map(|(device, location)| {
            ResidentChange::DeviceRoom(DeviceRoomChange {
                device: *device,
                location: *location,
            })
        });
        let wlans = self.wlans.iter().map(|(wlan, draft)| {
            ResidentChange::Wlan(WlanInput {
                wlan: *wlan,
                ssid: draft.ssid.clone(),
                psk: draft.psk.clone(),
                enabled: draft.enabled,
            })
        });
        disconnects
            .chain(port_vlans)
            .chain(connects)
            .chain(rooms)
            .chain(wlans)
            .collect()
    }
}

/// How the device is linked as saved, as a draft would say it: at the port row its cable comes
/// from, or nowhere; none for a port not the resident's (the operator's router).
pub fn current_link(resident: &Resident, device: &ResidentDevice) -> Option<LinkDraft> {
    let at = resident.ports.iter().find(|port| {
        port.far_ends
            .iter()
            .any(|end| end.own_device && end.device.id == device.id)
    });
    match at {
        Some(port) => Some(LinkDraft::Connect {
            port: PortKey::of(port),
            device_port: uplink_id(device),
            seen: false,
        }),
        None if device.uplink.is_some() => None,
        None => Some(LinkDraft::Disconnect),
    }
}

/// The id of the device's port that is its Anschluss.
pub fn uplink_id(device: &ResidentDevice) -> Option<i32> {
    let uplink = device.uplink.as_ref()?;
    device
        .ports
        .iter()
        .find(|port| &port.name == uplink)
        .map(|port| port.id)
}

#[cfg(test)]
mod test {
    use super::*;

    fn socket(id: i32) -> PortKey {
        PortKey {
            kind: PortKindInput::Socket,
            id,
        }
    }

    /// A device taken off a socket frees it before the socket gets a network or another device.
    #[test]
    fn devices_leave_before_ports_change() {
        let mut drafts = Drafts::default();
        drafts.links.insert(
            7,
            LinkDraft::Connect {
                port: socket(1),
                device_port: None,
                seen: false,
            },
        );
        drafts.port_vlans.insert(socket(2), Some(104));
        drafts.links.insert(8, LinkDraft::Disconnect);
        drafts.rooms.insert(7, 3);
        let kinds = drafts
            .changes()
            .into_iter()
            .map(|change| match change {
                ResidentChange::DisconnectDevice(_) => "disconnect",
                ResidentChange::PortVlan(_) => "port",
                ResidentChange::ConnectDevice(_) => "connect",
                ResidentChange::DeviceRoom(_) => "room",
                ResidentChange::Wlan(_) => "wlan",
            })
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["disconnect", "port", "connect", "room"]);
        assert_eq!(drafts.len(), 4);
        assert_eq!(drafts.len_in(View::Ports), 4);
        assert_eq!(drafts.len_in(View::Networks), 0);
    }

    /// One cable per port: a second device linked to a port takes the first one's draft off,
    /// and the port reset takes both its network and the device coming there off.
    #[test]
    fn a_port_takes_one_device() {
        let connect = |port| LinkDraft::Connect {
            port,
            device_port: None,
            seen: false,
        };
        let mut drafts = Drafts::default();
        drafts.port_vlans.insert(socket(1), Some(104));
        drafts.link(7, connect(socket(1)), None);
        drafts.link(9, connect(socket(2)), None);
        assert!(drafts.port_vlans.is_empty());
        drafts.link(8, connect(socket(1)), Some(LinkDraft::Disconnect));
        assert_eq!(
            drafts.links,
            BTreeMap::from([(8, connect(socket(1))), (9, connect(socket(2)))])
        );
        drafts.port_vlans.insert(socket(2), None);
        drafts.reset_port(socket(1));
        drafts.reset_port(socket(2));
        assert!(drafts.is_empty());
    }

    /// A link to the port row a device is at already is no draft, unless it names another
    /// Anschluss.
    #[test]
    fn a_link_as_it_is_is_none() {
        let now = LinkDraft::Connect {
            port: socket(1),
            device_port: Some(11),
            seen: false,
        };
        let connect = |port, device_port| LinkDraft::Connect {
            port,
            device_port,
            seen: false,
        };
        assert!(connect(socket(1), None).is(&now));
        assert!(connect(socket(1), Some(11)).is(&now));
        assert!(!connect(socket(1), Some(12)).is(&now));
        assert!(!connect(socket(2), None).is(&now));
        assert!(!LinkDraft::Disconnect.is(&now));
    }
}
