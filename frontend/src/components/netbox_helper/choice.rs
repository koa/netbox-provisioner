//! What a helper is opened with (`HelperPreset`), which one is shown (`HelperKind`), what it is
//! planned for (`HelperChoice`) and that as the input of a query (`helper_input!`).
use super::{flags::Flag, t};
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
    /// „Richtfunk abräumen“ from this radio of the link
    RadioDisconnect(u32),
    /// „Kunden-Anschluss abräumen“ of this interface
    CustomerDisconnect(u32),
    /// „VLAN am Port“ with this port or VLAN, or both
    PortVlan { port: Option<u32>, vlan: Option<u32> },
    /// „Kabel übernehmen“: the link from this port to the one the device sees there
    CableTakeover { port: u32, peer: u32 },
}

/// „Funkbrücke“ from a router port: the helper is the bridge device's, its router side set.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioPreset {
    pub bridge: u32,
    pub router_device: u32,
    pub router_port: u32,
}

impl HelperPreset {
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
        pd: u32,
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

/// The helper's input in the variables of `$module` (each query has its own types).
macro_rules! helper_input {
    ($module:ident, $choice:expr, $id:expr) => {
        match $choice {
            HelperChoice::Loopback => $module::Helper::Loopback($id as i64),
            HelperChoice::Connection {
                interface,
                peer,
                mtu,
            } => $module::Helper::Connection($module::ConnectionInput {
                interface: interface as i64,
                peer: peer as i64,
                mtu: mtu.map(i64::from),
            }),
            HelperChoice::Disconnect(interface) => {
                $module::Helper::Disconnect($module::DisconnectInput {
                    device: $id as i64,
                    interface: interface as i64,
                })
            }
            HelperChoice::Flags(changes) => $module::Helper::Flags($module::FlagsInput {
                device: $id as i64,
                changes: changes
                    .into_iter()
                    .map(|(interface, flag, set)| $module::TagChange {
                        interface: interface as i64,
                        tag: match flag {
                            Flag::Ospf => $module::InterfaceTag::OSPF,
                            Flag::Dhcp => $module::InterfaceTag::DHCP,
                            Flag::DhcpClient => $module::InterfaceTag::DHCP_CLIENT,
                            Flag::Nat => $module::InterfaceTag::NAT,
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
            } => $module::Helper::CustomerNet($module::CustomerNetInput {
                device: $id as i64,
                target: match target {
                    NetTarget::Interface(id) => $module::CustomerNetTarget::Interface(id as i64),
                    NetTarget::Vlan(id) => $module::CustomerNetTarget::Vlan(id as i64),
                },
                parent: parent.map(i64::from),
                length: length.map(i64::from),
                parent_v6: parent_v6.map(i64::from),
                dhcp,
            }),
            HelperChoice::Poe { pse, pd } => $module::Helper::Poe($module::PoeInput {
                device: $id as i64,
                pse: pse as i64,
                pd: pd as i64,
            }),
            HelperChoice::RadioBridge {
                ap,
                station,
                ap_router_port,
                station_router_port,
                ap_address,
                station_address,
                ssid,
            } => $module::Helper::RadioBridge($module::RadioBridgeInput {
                ap: ap as i64,
                station: station as i64,
                ap_router_port: ap_router_port.map(i64::from),
                station_router_port: station_router_port.map(i64::from),
                ap_address,
                station_address,
                ssid,
            }),
            HelperChoice::RadioDisconnect {
                radio,
                wireless_link,
            } => $module::Helper::RadioDisconnect($module::RadioDisconnectInput {
                device: $id as i64,
                radio: radio as i64,
                wireless_link,
            }),
            HelperChoice::CustomerDisconnect(interface) => {
                $module::Helper::CustomerDisconnect($module::CustomerDisconnectInput {
                    device: $id as i64,
                    interface: interface as i64,
                })
            }
            HelperChoice::PortVlan { port, vlan } => {
                $module::Helper::PortVlan($module::PortVlanInput {
                    device: $id as i64,
                    port: port as i64,
                    vlan: vlan as i64,
                })
            }
            HelperChoice::CableTakeover { port, peer } => {
                $module::Helper::CableTakeover($module::CableTakeoverInput {
                    port: port as i64,
                    peer: peer as i64,
                })
            }
        }
    };
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
}

impl HelperKind {
    pub const ALL: [HelperKind; 8] = [
        HelperKind::Loopback,
        HelperKind::Connection,
        HelperKind::Disconnect,
        HelperKind::CustomerNet,
        HelperKind::PortVlan,
        HelperKind::RadioBridge,
        HelperKind::Poe,
        HelperKind::Flags,
    ];
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
