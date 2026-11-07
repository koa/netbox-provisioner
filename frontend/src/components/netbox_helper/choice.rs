//! Scope: Which helper is shown, what it is opened with and its input; not a form.
//!
//! What a helper is opened with (`HelperPreset`), which one is shown (`HelperKind`), what it is
//! planned for (`HelperChoice`) and that as the input of a query (`helper`).
use super::{flags::Flag, new_device::SeenPreset, t};
use crate::graphql::authenticated::helper as input;
use std::fmt::{self, Display, Formatter};

/// The helper to open and what it starts with.
#[derive(Debug, Clone, PartialEq)]
pub enum HelperPreset {
    /// nothing chosen yet
    Kind(HelperKind),
    /// „Verbindung“ with these ends
    Link(LinkPreset),
    /// „Verbindung“ from this port, the other end to choose
    LinkFrom(u32),
    /// „Verbindung abräumen“ with this port
    Disconnect(u32),
    /// „Kunden-Netz“ on this port or VLAN
    CustomerNet(NetTarget),
    /// „Funkbrücke“ from a router port, for the bridge device cabled to it
    RadioFrom(RadioPreset),
    /// „Funkbrücke“ from a free router port: the bridge device is chosen first
    /// (`components::helper_flow`), then it is `RadioFrom`
    RadioChoose {
        router_device: u32,
        router_port: u32,
    },
    /// „PoE“ of this port
    Poe(u32),
    /// „PoE-Eintrag entfernen“: „PoE“ of this port with „Kein PoE“ chosen
    PoeRemove(u32),
    /// „Flags“ of this interface
    Flags(u32),
    /// „Richtfunk abräumen“ from this radio of the link
    RadioDisconnect(u32),
    /// „Kunden-Anschluss abräumen“ of this interface
    CustomerDisconnect(u32),
    /// „VLAN am Port“ with this port or VLAN, or both
    PortVlan {
        port: Option<u32>,
        vlan: Option<u32>,
    },
    /// „Kabel übernehmen“: the link from this port to the one the device sees there
    CableTakeover { port: u32, peer: u32 },
    /// „Aus Bridge lösen“ of this port
    BridgeLeave(u32),
    /// „Neues Gerät“ at this free port, with what the router sees there
    NewDevice { port: u32, seen: Option<SeenPreset> },
    /// „Adresse übernehmen“: this address, which only the device has, on this interface
    AddressTakeover { interface: u32, address: String },
    /// „Lease übernehmen“: this static lease, which only the device has, as a client device;
    /// the name is the one the client sent, if any
    LeaseTakeover {
        address: String,
        mac: String,
        name: Option<String>,
    },
}

/// „Funkbrücke“ from a router port: the helper is the bridge device's, its router side set.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioPreset {
    pub bridge: u32,
    pub router_device: u32,
    pub router_port: u32,
}

impl HelperPreset {
    /// Whether it gives the port the helper starts from, which the form then doesn't offer.
    pub(super) fn fixes_port(&self) -> bool {
        match self {
            HelperPreset::Link(_)
            | HelperPreset::LinkFrom(_)
            | HelperPreset::Disconnect(_)
            | HelperPreset::CustomerNet(_)
            | HelperPreset::RadioFrom(_)
            | HelperPreset::RadioChoose { .. }
            | HelperPreset::Poe(_)
            | HelperPreset::PoeRemove(_)
            | HelperPreset::NewDevice { .. }
            | HelperPreset::PortVlan { port: Some(_), .. } => true,
            HelperPreset::Kind(_)
            | HelperPreset::PortVlan { port: None, .. }
            | HelperPreset::Flags(_)
            | HelperPreset::RadioDisconnect(_)
            | HelperPreset::CustomerDisconnect(_)
            | HelperPreset::CableTakeover { .. }
            | HelperPreset::BridgeLeave(_)
            | HelperPreset::AddressTakeover { .. }
            | HelperPreset::LeaseTakeover { .. } => false,
        }
    }
    /// The device whose helper it is, `page` where it isn't another one's.
    pub fn device(&self, page: u32) -> u32 {
        match self {
            HelperPreset::RadioFrom(preset) => preset.bridge,
            _ => page,
        }
    }
}

/// The ends of a link to choose at the start: the device's port and the other end.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkPreset {
    pub port: u32,
    pub peer_device: u32,
    pub peer: u32,
}

/// The helper chosen, with what it needs.
#[derive(Debug, Clone, PartialEq)]
pub enum HelperChoice {
    Loopback,
    Connection {
        interface: u32,
        peer: u32,
        mtu: Option<u16>,
    },
    /// the link of this port, on the device or at the far end of one of its links
    Disconnect(u32),
    /// the tags switched: interface, tag, set
    Flags(Vec<(u32, Flag, bool)>),
    CustomerNet {
        target: NetTarget,
        parent: Option<u32>,
        length: Option<u8>,
        parent_v6: Option<u32>,
        dhcp: bool,
    },
    Poe {
        pse: u32,
        /// none: PoE on at `pse` alone, the other end not known yet
        pd: Option<u32>,
    },
    PoeRemove {
        port: u32,
    },
    RadioBridge {
        ap: u32,
        station: u32,
        ap_router_port: Option<u32>,
        station_router_port: Option<u32>,
        ap_address: Option<String>,
        station_address: Option<String>,
        ssid: Option<String>,
    },
    RadioDisconnect {
        radio: u32,
        wireless_link: bool,
    },
    CustomerDisconnect(u32),
    PortVlan {
        port: u32,
        vlan: u32,
    },
    CableTakeover {
        port: u32,
        peer: u32,
    },
    BridgeLeave(u32),
    NewDevice {
        port: u32,
        name: String,
        device_type: u32,
        role: u32,
        far_port: String,
        wlan_group: Option<u32>,
    },
    AddressTakeover {
        interface: u32,
        address: String,
    },
    LeaseTakeover {
        address: String,
        mac: String,
        name: String,
        device_type: u32,
        role: u32,
    },
}

/// Where „Kunden-Netz“ puts the network.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NetTarget {
    Interface(u32),
    Vlan(u32),
}

/// A port, VLAN or prefix in a select of „Kunden-Netz“.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetEntry<T> {
    pub(super) value: T,
    pub(super) text: String,
}

impl<T> Display for NetEntry<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The input naming the helper of `choice` on device `id`.
pub(crate) fn helper(choice: HelperChoice, id: u32) -> input::Helper {
    use input::*;
    let device = id as i32;
    match choice {
        HelperChoice::Loopback => Helper::Loopback(device),
        HelperChoice::Connection {
            interface,
            peer,
            mtu,
        } => Helper::Connection(ConnectionInput {
            interface: interface as i32,
            peer: peer as i32,
            mtu: mtu.map(i32::from),
        }),
        HelperChoice::Disconnect(interface) => Helper::Disconnect(DisconnectInput {
            device,
            interface: interface as i32,
        }),
        HelperChoice::Flags(changes) => Helper::Flags(FlagsInput {
            device,
            changes: changes
                .into_iter()
                .map(|(interface, flag, set)| TagChange {
                    interface: interface as i32,
                    tag: match flag {
                        Flag::Ospf => InterfaceTag::Ospf,
                        Flag::Dhcp => InterfaceTag::Dhcp,
                        Flag::DhcpClient => InterfaceTag::DhcpClient,
                        Flag::Nat => InterfaceTag::Nat,
                        Flag::Internet => InterfaceTag::Internet,
                    },
                    set,
                })
                .collect(),
        }),
        HelperChoice::CustomerNet {
            target,
            parent,
            length,
            parent_v6,
            dhcp,
        } => Helper::CustomerNet(CustomerNetInput {
            device,
            target: match target {
                NetTarget::Interface(id) => CustomerNetTarget::Interface(id as i32),
                NetTarget::Vlan(id) => CustomerNetTarget::Vlan(id as i32),
            },
            parent: parent.map(|id| id as i32),
            length: length.map(i32::from),
            parent_v6: parent_v6.map(|id| id as i32),
            dhcp,
        }),
        HelperChoice::Poe { pse, pd } => Helper::Poe(PoeInput {
            device,
            pse: pse as i32,
            pd: pd.map(|pd| pd as i32),
        }),
        HelperChoice::PoeRemove { port } => Helper::PoeRemove(PoeRemoveInput {
            device,
            port: port as i32,
        }),
        HelperChoice::RadioBridge {
            ap,
            station,
            ap_router_port,
            station_router_port,
            ap_address,
            station_address,
            ssid,
        } => Helper::RadioBridge(RadioBridgeInput {
            ap: ap as i32,
            station: station as i32,
            ap_router_port: ap_router_port.map(|port| port as i32),
            station_router_port: station_router_port.map(|port| port as i32),
            ap_address,
            station_address,
            ssid,
        }),
        HelperChoice::RadioDisconnect {
            radio,
            wireless_link,
        } => {
            let radio = radio as i32;
            Helper::RadioDisconnect(RadioDisconnectInput {
                device,
                radio,
                wireless_link,
            })
        }
        HelperChoice::CustomerDisconnect(interface) => {
            Helper::CustomerDisconnect(CustomerDisconnectInput {
                device,
                interface: interface as i32,
            })
        }
        HelperChoice::PortVlan { port, vlan } => Helper::PortVlan(PortVlanInput {
            device,
            port: port as i32,
            vlan: vlan as i32,
        }),
        HelperChoice::CableTakeover { port, peer } => Helper::CableTakeover(CableTakeoverInput {
            port: port as i32,
            peer: peer as i32,
        }),
        HelperChoice::BridgeLeave(port) => Helper::BridgeLeave(BridgeLeaveInput {
            device,
            port: port as i32,
        }),
        HelperChoice::NewDevice {
            port,
            name,
            device_type,
            role,
            far_port,
            wlan_group,
        } => Helper::NewDevice(NewDeviceInput {
            port: port as i32,
            name,
            device_type: device_type as i32,
            role: role as i32,
            far_port,
            wlan_group: wlan_group.map(|group| group as i32),
        }),
        HelperChoice::LeaseTakeover {
            address,
            mac,
            name,
            device_type,
            role,
        } => Helper::LeaseTakeover(LeaseTakeoverInput {
            device,
            address,
            mac,
            name,
            device_type: device_type as i32,
            role: role as i32,
        }),
        HelperChoice::AddressTakeover { interface, address } => {
            let interface = interface as i32;
            Helper::AddressTakeover(AddressTakeoverInput {
                device,
                interface,
                address,
            })
        }
    }
}

/// Which helper the card shows.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HelperKind {
    Loopback,
    Connection,
    Disconnect,
    Flags,
    CustomerNet,
    Poe,
    RadioBridge,
    /// only from a radio link's row: it needs the radio
    RadioDisconnect,
    /// only from a customer row: it needs the row's interface
    CustomerDisconnect,
    PortVlan,
    /// only from a live finding: it needs the port the device sees
    CableTakeover,
    /// only from a bridge's port: it needs the port
    BridgeLeave,
    /// only from a free port: it needs the port
    NewDevice,
    /// only from a live check: it needs the address the device has
    AddressTakeover,
    /// only from a live check: it needs the lease the device has
    LeaseTakeover,
}

impl HelperKind {
    /// whether it chooses among the ports of `linkPorts`
    pub(super) fn needs_ports(self) -> bool {
        matches!(
            self,
            HelperKind::Connection | HelperKind::Disconnect | HelperKind::RadioBridge
        )
    }
}

impl Display for HelperKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HelperKind::Loopback => t::LOOPBACK,
            HelperKind::Connection => t::CONNECTION,
            HelperKind::Disconnect => t::DISCONNECT,
            HelperKind::Flags => t::FLAGS,
            HelperKind::CustomerNet => t::CUSTOMER_NET,
            HelperKind::Poe => t::POE,
            HelperKind::RadioBridge => t::RADIO_BRIDGE,
            HelperKind::RadioDisconnect => t::RADIO_DISCONNECT,
            HelperKind::CustomerDisconnect => t::CUSTOMER_DISCONNECT,
            HelperKind::PortVlan => t::PORT_VLAN,
            HelperKind::CableTakeover => t::CABLE_TAKEOVER,
            HelperKind::BridgeLeave => t::BRIDGE_LEAVE,
            HelperKind::NewDevice => t::NEW_DEVICE,
            HelperKind::AddressTakeover => t::ADDRESS_TAKEOVER,
            HelperKind::LeaseTakeover => t::LEASE_TAKEOVER,
        })
    }
}

/// A port or device in a select: its id and how it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub(super) id: u32,
    pub(super) text: String,
}

impl Display for Choice {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
