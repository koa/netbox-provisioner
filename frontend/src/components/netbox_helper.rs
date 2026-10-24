//! The helpers that complete NetBox for the device (`Query.netboxPlan`), one chosen at a time:
//! what it would write, on a click the preview (the findings and the script applying would
//! bring after it), then writing it as a job, and taking the last one back. They write to
//! NetBox only, the device gets it by applying. „Verbindung“ asks for the two ports first
//! (`linkPorts`); a port cabled to a router brings the other end along. A chosen port whose
//! link is in the way gets a button to „Verbindung abräumen“ with it, which takes that link out
//! of NetBox. „Flags“ is a table of the interfaces' tags as switches, with the device's findings
//! as they would be after the switches (`topology.findings(after:)`). „Kunden-Netz“ chooses a
//! port or VLAN and a prefix of the site (`customerNetOptions`). „PoE“ chooses the direction and
//! both ports (`poeOptions`), or takes them from what the device reports feeding
//! (`access.poeObserved`). „Funkbrücke“ runs on a bridge's page: its role, the other bridge
//! (from its wireless link), and only where they aren't cabled the router ports
//! (`radioBridgeCandidates`, `linkPorts`). „VLAN am Port“ chooses a port and a VLAN
//! (`portVlanOptions`).
use crate::{
    components::{
        adjust_target::SelectedTarget, findings::finding_list, job::JobView, menu::popup::Select,
    },
    error::FrontendError,
    graphql::{
        authenticated::{
            CustomerNetOptions, FlagInterfaces, Findings, HelperJobs, customer_net_options, LinkPorts, NetboxPlan, NetboxPlanPreview,
            PoeObserved, PoeOptions, PortVlanOptions, RadioBridgeCandidates, UndoNetboxWrite,
            WriteNetboxPlan, port_vlan_options,
            flag_interfaces, radio_bridge_candidates,
            poe_observed, poe_options::{self, PoePortFields},
            findings::{self, FindingsTopologyFindings},
            helper_jobs::{self, JobKind, JobState},
            link_ports::{self, LinkPortFields},
            netbox_plan::{self, NetboxPlanNetboxPlanSteps as Step},
            netbox_plan_preview, undo_netbox_write, write_netbox_plan,
        },
        query_authenticated,
    },
    i18n::helper as t,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode, Form, FormGroup, Spinner, SpinnerSize, Switch, TextInput,
};
use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};
use yew::{
    Callback, Component, Context, Html, Properties, classes, html, html::Scope, platform::spawn_local,
};

#[derive(Clone, PartialEq, Properties)]
pub struct NetboxHelperProps {
    pub id: u32,
    /// how the device is reached for the preview of the script
    pub target: SelectedTarget,
    /// NetBox was written to (or that was taken back): the page shows what changed
    #[prop_or_default]
    pub on_written: Callback<()>,
    /// the helper to open and what it starts with (a cable from a finding, a row's action);
    /// without, „Loopback“
    #[prop_or_default]
    pub preset: Option<HelperPreset>,
    /// inside the modal of `components::helper_flow`: no card, no choice of the helper
    #[prop_or_default]
    pub embedded: bool,
    /// where it is: planning, the preview shown, writing, written
    #[prop_or_default]
    pub on_phase: Callback<Phase>,
}

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

/// How far the helper is, for the stepper of `components::helper_flow`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Phase {
    Plan,
    Preview,
    Writing,
    /// the write job is done
    Written,
    /// it failed or was cancelled; what is written can be taken back
    WriteFailed,
}

/// The ends of a link to choose at the start: the device's port and the other end.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkPreset {
    pub port: u32,
    pub peer_device: u32,
    pub peer: u32,
}

/// What the preview shows: the device's finding kinds before and after the plan, and the
/// script (`None`: the device isn't reachable).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    before: Vec<String>,
    after: Vec<String>,
    script: Option<String>,
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
}

/// What „VLAN am Port“ is choosing: the group first, then a VLAN of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PortVlanChoice {
    port: Option<u32>,
    group: Option<u32>,
    vlan: Option<u32>,
}

/// What „Funkbrücke“ is choosing: whether this bridge is the AP, the other bridge, and per side
/// (this one first) a router port where the bridge isn't cabled and an address.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioChoice {
    own_ap: bool,
    peer: Option<u32>,
    router_device: [Option<u32>; 2],
    router_port: [Option<u32>; 2],
    address: [String; 2],
    ssid: String,
}

impl Default for RadioChoice {
    fn default() -> Self {
        Self {
            own_ap: true,
            peer: None,
            router_device: [None; 2],
            router_port: [None; 2],
            address: Default::default(),
            ssid: String::new(),
        }
    }
}

/// What „PoE“ is choosing: whether the device feeds (else it is fed), its port, the other end.
#[derive(Debug, Clone, PartialEq)]
pub struct PoeChoice {
    feeds: bool,
    port: Option<u32>,
    peer_device: Option<u32>,
    peer: Option<u32>,
}

impl Default for PoeChoice {
    fn default() -> Self {
        Self {
            feeds: true,
            port: None,
            peer_device: None,
            peer: None,
        }
    }
}

/// Where „Kunden-Netz“ puts the network.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NetTarget {
    Interface(u32),
    Vlan(u32),
}

/// What „Kunden-Netz“ is choosing.
#[derive(Debug, Clone, PartialEq)]
pub struct NetChoice {
    target: Option<NetTarget>,
    parent: Option<u32>,
    length: String,
    parent_v6: Option<u32>,
    dhcp: bool,
}

impl Default for NetChoice {
    fn default() -> Self {
        Self {
            target: None,
            parent: None,
            length: String::new(),
            parent_v6: None,
            dhcp: true,
        }
    }
}

/// A port, VLAN or prefix in a select of „Kunden-Netz“.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetEntry<T> {
    value: T,
    text: String,
}

impl<T> Display for NetEntry<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A tag of „Flags“.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Flag {
    Ospf,
    Dhcp,
    DhcpClient,
    Nat,
}

const FLAGS: [Flag; 4] = [Flag::Ospf, Flag::Dhcp, Flag::DhcpClient, Flag::Nat];

impl Flag {
    fn slug(self) -> &'static str {
        match self {
            Flag::Ospf => "ospf",
            Flag::Dhcp => "dhcp",
            Flag::DhcpClient => "dhcp-client",
            Flag::Nat => "nat",
        }
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
    fn needs_ports(self) -> bool {
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
        })
    }
}

/// A port or device in a select: its id and how it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    id: u32,
    text: String,
}

impl Display for Choice {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The ends of a link being chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkChoice {
    port: Option<u32>,
    peer_device: Option<u32>,
    peer: Option<u32>,
    mtu: String,
}

pub struct NetboxHelper {
    /// the device of the page
    device: u32,
    kind: HelperKind,
    link: LinkChoice,
    /// the port whose link „Verbindung abräumen“ takes out
    disconnect: Option<u32>,
    /// „Richtfunk abräumen“: the radio, and whether the wireless link goes too
    radio_disconnect: Option<u32>,
    radio_disconnect_link: bool,
    /// „Kunden-Anschluss abräumen“: the row's interface
    customer_disconnect: Option<u32>,
    ports: Option<Result<link_ports::ResponseData, FrontendError>>,
    flag_interfaces: Option<Result<Vec<FlagInterface>, FrontendError>>,
    /// the tags switched away from what NetBox has
    flags: BTreeMap<(u32, Flag), bool>,
    /// the device's findings after the switches, with NetBox' address: for these
    flag_findings: Option<FlagFindings>,
    net: NetChoice,
    net_options: Option<Result<NetOptions, FrontendError>>,
    port_vlan: PortVlanChoice,
    port_vlan_options: Option<Result<PortVlanOptionsData, FrontendError>>,
    poe: PoeChoice,
    poe_options: Option<Result<PoeOptionsData, FrontendError>>,
    /// what the device reports feeding, once asked
    poe_observed: Option<Result<Vec<Observed>, FrontendError>>,
    poe_observing: bool,
    radio: RadioChoice,
    radio_options: Option<Result<Vec<Candidate>, FrontendError>>,
    /// what the plan shown was computed for
    planned: Option<HelperChoice>,
    plan: Option<Result<netbox_plan::ResponseData, FrontendError>>,
    /// a removal written: its choice is gone from NetBox, there is no plan for it any more
    removed: bool,
    preview: Option<Result<Preview, FrontendError>>,
    preview_loading: bool,
    /// the job shown, writing or undoing
    job: Option<i64>,
    job_running: bool,
    /// the write job whose objects "Rückgängig" deletes or sets back
    written: Option<i64>,
    /// how the write job of this session ended
    write_done: Option<bool>,
    /// the phase told last (`on_phase`)
    reported: Option<Phase>,
    error: Option<FrontendError>,
}

pub enum NetboxHelperMsg {
    Kind(HelperKind),
    Ports(Result<link_ports::ResponseData, FrontendError>),
    Port(Choice),
    PeerDevice(Choice),
    Peer(Choice),
    Mtu(String),
    /// „Verbindung abräumen“ with this port
    DisconnectPort(u32),
    FlagInterfaces(Result<Vec<FlagInterface>, FrontendError>),
    /// a switch of „Flags“ to this
    Toggle(u32, Flag, bool),
    FlagFindings(
        Vec<(u32, Flag, bool)>,
        Result<(String, Vec<FindingsTopologyFindings>), FrontendError>,
    ),
    NetOptions(Result<NetOptions, FrontendError>),
    NetTarget(NetTarget),
    NetParent(Option<u32>),
    NetParentV6(Option<u32>),
    NetLength(String),
    NetDhcp(bool),
    PortVlanOptions(Result<PortVlanOptionsData, FrontendError>),
    PortVlanPort(u32),
    PortVlanGroup(u32),
    PortVlanVlan(u32),
    PoeOptions(Result<PoeOptionsData, FrontendError>),
    PoeFeeds(bool),
    PoePort(Choice),
    PoePeerDevice(Choice),
    PoePeer(Choice),
    PoeObserve,
    PoeObserved(Result<Vec<Observed>, FrontendError>),
    /// what the device reports: its port feeds the other end (device, port) if known
    PoeTake(u32, Option<(u32, u32)>),
    RadioOptions(Result<Vec<Candidate>, FrontendError>),
    RadioOwnAp(bool),
    RadioPeer(Choice),
    /// side 0 is this bridge, 1 the other one
    RadioRouterDevice(usize, Choice),
    RadioRouterPort(usize, Choice),
    RadioAddress(usize, String),
    RadioSsid(String),
    RadioDisconnectLink(bool),
    Plan(HelperChoice, Result<netbox_plan::ResponseData, FrontendError>),
    LoadPreview,
    Preview(Result<Preview, FrontendError>),
    Write,
    Undo,
    /// a write job (`true`) or an undo job started
    JobStarted(Result<i64, FrontendError>, bool),
    /// `true`: done
    JobFinished(bool),
    /// the last write job of the device the server still knows, done and not taken back
    LastWrite(i64),
}

impl Component for NetboxHelper {
    type Message = NetboxHelperMsg;
    type Properties = NetboxHelperProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut link = LinkChoice::default();
        let mut disconnect = None;
        let mut net = NetChoice::default();
        let mut radio = RadioChoice::default();
        let mut radio_disconnect = None;
        let mut customer_disconnect = None;
        let mut port_vlan = PortVlanChoice::default();
        let kind = match &ctx.props().preset {
            None => HelperKind::Loopback,
            Some(HelperPreset::Kind(kind)) => *kind,
            Some(HelperPreset::Link(preset)) => {
                link = LinkChoice {
                    port: Some(preset.port),
                    peer_device: Some(preset.peer_device),
                    peer: Some(preset.peer),
                    mtu: String::new(),
                };
                HelperKind::Connection
            }
            Some(HelperPreset::LinkFrom(port)) => {
                link.port = Some(*port);
                HelperKind::Connection
            }
            Some(HelperPreset::Disconnect(port)) => {
                disconnect = Some(*port);
                HelperKind::Disconnect
            }
            Some(HelperPreset::CustomerNet(target)) => {
                net.target = Some(*target);
                HelperKind::CustomerNet
            }
            Some(HelperPreset::RadioFrom(preset)) => {
                radio.router_device[0] = Some(preset.router_device);
                radio.router_port[0] = Some(preset.router_port);
                HelperKind::RadioBridge
            }
            Some(HelperPreset::RadioDisconnect(radio)) => {
                radio_disconnect = Some(*radio);
                HelperKind::RadioDisconnect
            }
            Some(HelperPreset::CustomerDisconnect(interface)) => {
                customer_disconnect = Some(*interface);
                HelperKind::CustomerDisconnect
            }
            Some(HelperPreset::PortVlan { port, vlan }) => {
                port_vlan = PortVlanChoice {
                    port: *port,
                    group: None,
                    vlan: *vlan,
                };
                HelperKind::PortVlan
            }
        };
        last_write(ctx.link().clone(), ctx.props().id);
        let mut helper = Self {
            device: ctx.props().id,
            kind,
            link,
            disconnect,
            radio_disconnect,
            radio_disconnect_link: false,
            customer_disconnect,
            ports: None,
            flag_interfaces: None,
            flags: BTreeMap::new(),
            flag_findings: None,
            net,
            net_options: None,
            port_vlan,
            port_vlan_options: None,
            poe: PoeChoice::default(),
            poe_options: None,
            poe_observed: None,
            poe_observing: false,
            radio,
            radio_options: None,
            planned: None,
            plan: None,
            removed: false,
            preview: None,
            preview_loading: false,
            job: None,
            job_running: false,
            written: None,
            write_done: None,
            reported: None,
            error: None,
        };
        helper.load(ctx);
        helper.choose(ctx);
        helper
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NetboxHelperMsg::Kind(kind) => {
                self.kind = kind;
                self.load(ctx);
                self.choose(ctx);
            }
            NetboxHelperMsg::FlagInterfaces(interfaces) => self.flag_interfaces = Some(interfaces),
            NetboxHelperMsg::NetOptions(options) => self.net_options = Some(options),
            NetboxHelperMsg::NetTarget(target) => {
                self.net.target = Some(target);
                self.choose(ctx);
            }
            NetboxHelperMsg::NetParent(parent) => {
                self.net.parent = parent;
                self.choose(ctx);
            }
            NetboxHelperMsg::NetParentV6(parent) => {
                self.net.parent_v6 = parent;
                self.choose(ctx);
            }
            NetboxHelperMsg::NetLength(length) => {
                self.net.length = length;
                self.choose(ctx);
            }
            NetboxHelperMsg::NetDhcp(dhcp) => {
                self.net.dhcp = dhcp;
                self.choose(ctx);
            }
            NetboxHelperMsg::PortVlanOptions(options) => {
                // the group of the VLAN given, else the one the backend starts with
                if let Ok(options) = &options
                    && self.port_vlan.group.is_none()
                {
                    self.port_vlan.group = self
                        .port_vlan
                        .vlan
                        .and_then(|vlan| {
                            options.groups.iter().find(|g| g.vlans.iter().any(|v| v.id as u32 == vlan))
                        })
                        .map(|g| g.id as u32)
                        .or(options.group.map(|g| g as u32));
                }
                self.port_vlan_options = Some(options);
            }
            NetboxHelperMsg::PortVlanGroup(group) => {
                if self.port_vlan.group != Some(group) {
                    self.port_vlan.group = Some(group);
                    self.port_vlan.vlan = None;
                    self.choose(ctx);
                }
            }
            NetboxHelperMsg::PortVlanPort(port) => {
                self.port_vlan.port = Some(port);
                self.choose(ctx);
            }
            NetboxHelperMsg::PortVlanVlan(vlan) => {
                self.port_vlan.vlan = Some(vlan);
                self.choose(ctx);
            }
            NetboxHelperMsg::PoeOptions(options) => self.poe_options = Some(options),
            NetboxHelperMsg::RadioOptions(options) => {
                // this bridge's role and the other end of its link, as NetBox has them
                if let Ok(candidates) = &options
                    && let Some(own) = candidates.iter().find(|c| c.device.id as u32 == ctx.props().id)
                {
                    if self.radio.peer.is_none() {
                        self.radio.peer = own.peer.as_ref().map(|peer| peer.id as u32);
                    }
                    if let Some(role) = &own.role {
                        self.radio.own_ap = !matches!(role, radio_bridge_candidates::RadioRole::STATION);
                    }
                }
                self.radio_options = Some(options);
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioOwnAp(ap) => {
                self.radio.own_ap = ap;
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioPeer(peer) => {
                if self.radio.peer != Some(peer.id) {
                    self.radio.router_device[1] = None;
                    self.radio.router_port[1] = None;
                    self.radio.address[1].clear();
                }
                self.radio.peer = Some(peer.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioRouterDevice(side, device) => {
                if self.radio.router_device[side] != Some(device.id) {
                    self.radio.router_port[side] = None;
                }
                self.radio.router_device[side] = Some(device.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioRouterPort(side, port) => {
                self.radio.router_port[side] = Some(port.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioAddress(side, address) => {
                self.radio.address[side] = address;
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioDisconnectLink(link) => {
                self.radio_disconnect_link = link;
                self.choose(ctx);
            }
            NetboxHelperMsg::RadioSsid(ssid) => {
                self.radio.ssid = ssid;
                self.choose(ctx);
            }
            NetboxHelperMsg::PoeFeeds(feeds) => {
                self.poe.feeds = feeds;
                self.choose(ctx);
            }
            NetboxHelperMsg::PoePort(port) => {
                self.poe.port = Some(port.id);
                // the other end of its cable
                let far = self
                    .poe_port(port.id)
                    .and_then(|p| p.connected.first())
                    .map(|far| (far.device.id as u32, far.id as u32));
                if let Some((device, far)) = far {
                    self.poe.peer_device = Some(device);
                    self.poe.peer = Some(far);
                }
                self.choose(ctx);
            }
            NetboxHelperMsg::PoePeerDevice(device) => {
                if self.poe.peer_device != Some(device.id) {
                    self.poe.peer = None;
                }
                self.poe.peer_device = Some(device.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::PoePeer(port) => {
                self.poe.peer = Some(port.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::PoeObserve => {
                self.poe_observing = true;
                fetch_poe_observed(ctx.link().clone(), ctx.props().id, ctx.props().target.clone());
            }
            NetboxHelperMsg::PoeObserved(observed) => {
                self.poe_observing = false;
                self.poe_observed = Some(observed);
            }
            NetboxHelperMsg::PoeTake(port, far) => {
                self.poe = PoeChoice {
                    feeds: true,
                    port: Some(port),
                    peer_device: far.map(|(device, _)| device),
                    peer: far.map(|(_, port)| port),
                };
                self.choose(ctx);
            }
            NetboxHelperMsg::Toggle(interface, flag, set) => {
                let has = self
                    .flag_interface(interface)
                    .is_some_and(|i| i.tags.iter().any(|tag| tag_slug(tag) == flag.slug()));
                if has == set {
                    self.flags.remove(&(interface, flag));
                } else {
                    self.flags.insert((interface, flag), set);
                }
                self.check_flags(ctx);
                self.choose(ctx);
            }
            NetboxHelperMsg::FlagFindings(changes, _) if changes != self.flag_changes() => {
                return false;
            }
            NetboxHelperMsg::FlagFindings(changes, findings) => {
                self.flag_findings = Some((changes, findings));
            }
            NetboxHelperMsg::Ports(ports) => self.ports = Some(ports),
            NetboxHelperMsg::Port(port) => {
                self.link.port = Some(port.id);
                // a cable to a router: its other end
                let far = self
                    .port(port.id)
                    .and_then(|p| p.connected.first())
                    .map(|far| (far.device.id as u32, far.id as u32));
                if let Some((device, far)) = far {
                    self.link.peer_device = Some(device);
                    self.link.peer = Some(far);
                }
                self.choose(ctx);
            }
            NetboxHelperMsg::PeerDevice(device) => {
                if self.link.peer_device != Some(device.id) {
                    self.link.peer = None;
                }
                self.link.peer_device = Some(device.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::Peer(port) => {
                self.link.peer = Some(port.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::Mtu(mtu) => {
                self.link.mtu = mtu;
                self.choose(ctx);
            }
            NetboxHelperMsg::DisconnectPort(port) => {
                self.kind = HelperKind::Disconnect;
                self.disconnect = Some(port);
                self.choose(ctx);
            }
            NetboxHelperMsg::Plan(choice, _) if self.planned.as_ref() != Some(&choice) => {
                // for a choice changed since
                return false;
            }
            NetboxHelperMsg::Plan(_, plan) => {
                // another plan, another preview
                let fingerprint = |plan: Option<&Result<netbox_plan::ResponseData, _>>| {
                    plan.and_then(|plan| plan.as_ref().ok())
                        .map(|plan| plan.netbox_plan.fingerprint.clone())
                };
                if fingerprint(self.plan.as_ref()) != fingerprint(Some(&plan)) {
                    self.preview = None;
                }
                self.plan = Some(plan);
            }
            NetboxHelperMsg::LoadPreview => {
                self.preview_loading = true;
                let Some(choice) = self.planned.clone() else {
                    return false;
                };
                fetch_preview(
                    ctx.link().clone(),
                    ctx.props().id,
                    choice,
                    ctx.props().target.clone(),
                );
            }
            NetboxHelperMsg::Preview(preview) => {
                self.preview_loading = false;
                self.preview = Some(preview);
            }
            NetboxHelperMsg::Write => {
                let (Some(Ok(plan)), Some(choice)) = (&self.plan, self.planned.clone()) else {
                    return false;
                };
                self.job_running = true;
                self.error = None;
                let scope = ctx.link().clone();
                let variables = write_netbox_plan::Variables {
                    helper: helper_input!(write_netbox_plan, choice, ctx.props().id),
                    fingerprint: plan.netbox_plan.fingerprint.clone(),
                };
                spawn_local(async move {
                    let result = query_authenticated::<WriteNetboxPlan, _>(scope.clone(), variables)
                        .await
                        .map(|data| data.write_netbox_plan.id);
                    scope.send_message(NetboxHelperMsg::JobStarted(result, true));
                });
            }
            NetboxHelperMsg::Undo => {
                let Some(job_id) = self.written else {
                    return false;
                };
                self.job_running = true;
                self.error = None;
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let result = query_authenticated::<UndoNetboxWrite, _>(
                        scope.clone(),
                        undo_netbox_write::Variables { job_id },
                    )
                    .await
                    .map(|data| data.undo_netbox_write.id);
                    scope.send_message(NetboxHelperMsg::JobStarted(result, false));
                });
            }
            NetboxHelperMsg::JobStarted(result, write) => match result {
                Ok(id) => {
                    self.job = Some(id);
                    self.written = write.then_some(id);
                    self.write_done = None;
                }
                Err(error) => {
                    self.job_running = false;
                    self.error = Some(error);
                }
            },
            NetboxHelperMsg::LastWrite(id) => {
                if self.job.is_none() {
                    self.written = Some(id);
                }
            }
            NetboxHelperMsg::JobFinished(done) => {
                self.job_running = false;
                if self.written.is_some() && self.written == self.job {
                    self.write_done = Some(done);
                }
                self.preview = None;
                // a written removal took what it was chosen for: planned again it finds nothing
                let removed = done
                    && self.written.is_some()
                    && self.written == self.job
                    && matches!(
                        self.kind,
                        HelperKind::Disconnect
                            | HelperKind::RadioDisconnect
                            | HelperKind::CustomerDisconnect
                    );
                self.removed = removed;
                if removed {
                    self.plan = None;
                } else if let Some(choice) = self.planned.clone() {
                    fetch_plan(ctx.link().clone(), ctx.props().id, choice);
                }
                // the ports have the link now
                if self.ports.is_some() {
                    fetch_ports(ctx.link().clone(), ctx.props().id);
                }
                // the port or VLAN has its network now
                if self.net_options.is_some() {
                    self.net = NetChoice::default();
                    fetch_net_options(ctx.link().clone(), ctx.props().id);
                    self.choose(ctx);
                }
                // the port has its VLAN now
                if self.port_vlan_options.is_some() {
                    self.port_vlan = PortVlanChoice::default();
                    fetch_port_vlan_options(ctx.link().clone(), ctx.props().id);
                    self.choose(ctx);
                }
                // the bridges have their links and addresses now
                if self.radio_options.is_some() {
                    fetch_radio_options(ctx.link().clone(), ctx.props().id);
                }
                // the ports have their modes now
                if self.poe_options.is_some() {
                    fetch_poe_options(ctx.link().clone(), ctx.props().id);
                }
                // NetBox has the tags switched now (or again what it had)
                if self.flag_interfaces.is_some() {
                    self.flags.clear();
                    fetch_flag_interfaces(ctx.link().clone(), ctx.props().id);
                    self.check_flags(ctx);
                    self.choose(ctx);
                }
                ctx.props().on_written.emit(());
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let job = self.job.map(|id| html! {
            <JobView key={id} id={id} on_finished={ctx.link().callback(NetboxHelperMsg::JobFinished)}/>
        });
        let error = self.error.as_ref().map(|error| error.to_html());
        let plan = match (&self.planned, &self.plan) {
            (None, _) if self.kind == HelperKind::Disconnect => {
                html!(<p>{t::CHOOSE_DISCONNECT_PORT}</p>)
            }
            (None, _) if self.kind == HelperKind::Flags => html!(<p>{t::CHOOSE_FLAGS}</p>),
            (None, _) if self.kind == HelperKind::CustomerNet => html!(<p>{t::CHOOSE_NET}</p>),
            (None, _) if self.kind == HelperKind::Poe => html!(<p>{t::CHOOSE_POE}</p>),
            (None, _) if self.kind == HelperKind::RadioBridge => {
                html!(<p>{t::CHOOSE_RADIO_BRIDGE}</p>)
            }
            (None, _) if self.kind == HelperKind::RadioDisconnect => {
                html!(<p>{t::CHOOSE_RADIO_DISCONNECT}</p>)
            }
            (None, _) if self.kind == HelperKind::CustomerDisconnect => {
                html!(<p>{t::CHOOSE_CUSTOMER_DISCONNECT}</p>)
            }
            (None, _) if self.kind == HelperKind::PortVlan => html!(<p>{t::CHOOSE_PORT_VLAN}</p>),
            (None, _) => html!(<p>{t::CHOOSE_PORTS}</p>),
            (_, None) if self.removed => html!(),
            (_, None) => html!(<Spinner size={SpinnerSize::Md}/>),
            (_, Some(Err(error))) => error.to_html(),
            (_, Some(Ok(plan))) if plan.netbox_plan.steps.is_empty() => html!(<p>{t::COMPLETE}</p>),
            (_, Some(Ok(plan))) => {
                let allowed = plan.allow_netbox_write;
                html! {<>
                    <ol class="helper-steps">
                        { for plan.netbox_plan.steps.iter().map(|step| html!(<li>{describe(step)}</li>)) }
                    </ol>
                    if let Some(router) = &plan.netbox_plan.checked_with {
                        <p>{t::checked_with(router)}</p>
                    }
                    if plan.netbox_plan.steps.iter().any(|step| matches!(step, Step::DeleteAddress(_))) {
                        <p>{t::ADDRESSES_GO}</p>
                    }
                    if !plan.netbox_plan.cut_off.is_empty() {
                        <Alert inline=true title={t::CUT_OFF} r#type={AlertType::Warning}>
                            <ul>
                            { for plan.netbox_plan.cut_off.iter().map(|device| html!(<li>{&device.name}</li>)) }
                            </ul>
                        </Alert>
                    }
                    if !plan.netbox_plan.apply_together.is_empty() {
                        <Alert inline=true title={t::APPLY_TOGETHER} r#type={AlertType::Warning}>
                            <ul>
                            { for plan.netbox_plan.apply_together.iter().map(|device| html!(<li>{&device.name}</li>)) }
                            </ul>
                        </Alert>
                    }
                    if !plan.netbox_plan.undocumented.is_empty() {
                        <Alert inline=true title={t::UNDOCUMENTED} r#type={AlertType::Warning}>
                            <ul>
                            { for plan.netbox_plan.undocumented.iter().map(|taken| html!(
                                <li>{t::undocumented(&taken.address, &taken.route, taken.gateway.as_deref(), &taken.router)}</li>
                            ))}
                            </ul>
                        </Alert>
                    }
                    { self.preview_view() }
                    if !allowed {
                        <Alert inline=true title={t::LOCKED} r#type={AlertType::Info}/>
                    }
                    <div class="helper-actions">
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={t::PREVIEW}
                            disabled={self.preview_loading || self.job_running}
                            onclick={ctx.link().callback(|_| NetboxHelperMsg::LoadPreview)}
                        />
                        {" "}
                        <Button
                            variant={ButtonVariant::Primary}
                            label={t::WRITE}
                            disabled={!allowed || self.job_running}
                            onclick={ctx.link().callback(|_| NetboxHelperMsg::Write)}
                        />
                    </div>
                </>}
            }
        };
        let undo = self.written.filter(|_| !self.job_running).map(|_| html! {
            <div class="helper-actions">
                <p>{t::WRITTEN}</p>
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::UNDO}
                    onclick={ctx.link().callback(|_| NetboxHelperMsg::Undo)}
                />
            </div>
        });
        let hint = match self.kind {
            HelperKind::Loopback => t::LOOPBACK_HINT,
            HelperKind::Connection => t::CONNECTION_HINT,
            HelperKind::Disconnect => t::DISCONNECT_HINT,
            HelperKind::Flags => t::FLAGS_HINT,
            HelperKind::CustomerNet => t::CUSTOMER_NET_HINT,
            HelperKind::Poe => t::POE_HINT,
            HelperKind::RadioBridge => t::RADIO_BRIDGE_HINT,
            HelperKind::RadioDisconnect => t::RADIO_DISCONNECT_HINT,
            HelperKind::CustomerDisconnect => t::CUSTOMER_DISCONNECT_HINT,
            HelperKind::PortVlan => t::PORT_VLAN_HINT,
        };
        let content = html! {<>
                    {error}
                    {job}
                    {undo}
                    if !ctx.props().embedded {
                    <div class="helper-kind">
                        <Select<HelperKind>
                            selected={Some(self.kind)}
                            entries={vec![
                                HelperKind::Loopback,
                                HelperKind::Connection,
                                HelperKind::Disconnect,
                                HelperKind::Flags,
                                HelperKind::CustomerNet,
                                HelperKind::PortVlan,
                                HelperKind::Poe,
                                HelperKind::RadioBridge,
                            ]}
                            onselect={ctx.link().callback(NetboxHelperMsg::Kind)}
                        />
                    </div>
                    }
                    <p>{hint}</p>
                    if self.kind == HelperKind::Connection {
                        { self.link_form(ctx) }
                    }
                    if self.kind == HelperKind::Disconnect {
                        { self.disconnect_form(ctx) }
                    }
                    if self.kind == HelperKind::Flags {
                        { self.flags_form(ctx) }
                    }
                    if self.kind == HelperKind::CustomerNet {
                        { self.net_form(ctx) }
                    }
                    if self.kind == HelperKind::PortVlan {
                        { self.port_vlan_form(ctx) }
                    }
                    if self.kind == HelperKind::Poe {
                        { self.poe_form(ctx) }
                    }
                    if self.kind == HelperKind::RadioBridge {
                        { self.radio_form(ctx) }
                    }
                    if self.kind == HelperKind::RadioDisconnect {
                        <Form>
                            <Switch
                                label={t::RADIO_DISCONNECT_LINK}
                                checked={self.radio_disconnect_link}
                                onchange={ctx.link().callback(NetboxHelperMsg::RadioDisconnectLink)}
                            />
                        </Form>
                    }
                    {plan}
        </>};
        if ctx.props().embedded {
            return html!(<div class="netbox-helper">{content}</div>);
        }
        html! {
            <Card class="netbox-helper">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>{content}</CardBody>
            </Card>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        let phase = self.phase();
        if self.reported != Some(phase) {
            self.reported = Some(phase);
            ctx.props().on_phase.emit(phase);
        }
    }
}

impl NetboxHelper {
    /// Fetches what the chosen helper chooses from, unless it is there.
    fn load(&mut self, ctx: &Context<Self>) {
        let kind = self.kind;
        if kind.needs_ports() && self.ports.is_none() {
            fetch_ports(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::CustomerNet && self.net_options.is_none() {
            fetch_net_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::PortVlan && self.port_vlan_options.is_none() {
            fetch_port_vlan_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::Poe && self.poe_options.is_none() {
            fetch_poe_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::RadioBridge && self.radio_options.is_none() {
            fetch_radio_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::Flags {
            if self.flag_interfaces.is_none() {
                fetch_flag_interfaces(ctx.link().clone(), ctx.props().id);
            }
            self.check_flags(ctx);
        }
    }
    fn phase(&self) -> Phase {
        let writing = self.written.is_some() && self.written == self.job;
        if writing && self.job_running {
            Phase::Writing
        } else if writing && self.write_done == Some(true) {
            Phase::Written
        } else if writing && self.write_done == Some(false) {
            Phase::WriteFailed
        } else if matches!(self.preview, Some(Ok(_))) {
            Phase::Preview
        } else {
            Phase::Plan
        }
    }
    /// The helper with its input as chosen now, `None` while ports are missing; a new one
    /// gets its plan.
    fn choice(&self) -> Option<HelperChoice> {
        match self.kind {
            HelperKind::Loopback => Some(HelperChoice::Loopback),
            HelperKind::Connection => Some(HelperChoice::Connection {
                interface: self.link.port?,
                peer: self.link.peer?,
                mtu: self.link.mtu.trim().parse().ok(),
            }),
            HelperKind::Disconnect => Some(HelperChoice::Disconnect(self.disconnect?)),
            HelperKind::CustomerDisconnect => {
                Some(HelperChoice::CustomerDisconnect(self.customer_disconnect?))
            }
            HelperKind::PortVlan => Some(HelperChoice::PortVlan {
                port: self.port_vlan.port?,
                vlan: self.port_vlan.vlan?,
            }),
            HelperKind::RadioDisconnect => Some(HelperChoice::RadioDisconnect {
                radio: self.radio_disconnect?,
                wireless_link: self.radio_disconnect_link,
            }),
            HelperKind::Flags => {
                let changes = self.flag_changes();
                (!changes.is_empty()).then_some(HelperChoice::Flags(changes))
            }
            HelperKind::CustomerNet => (self.net.parent.is_some() || self.net.parent_v6.is_some())
                .then_some(HelperChoice::CustomerNet {
                    target: self.net.target?,
                    parent: self.net.parent,
                    length: self.net.length.trim().trim_start_matches('/').parse().ok(),
                    parent_v6: self.net.parent_v6,
                    dhcp: self.net.dhcp,
                }),
            HelperKind::Poe => {
                let (own, peer) = (self.poe.port?, self.poe.peer?);
                let (pse, pd) = if self.poe.feeds { (own, peer) } else { (peer, own) };
                Some(HelperChoice::Poe { pse, pd })
            }
            HelperKind::RadioBridge => {
                let radio = &self.radio;
                let (own, peer) = (self.own_candidate()?.device.id as u32, radio.peer?);
                let address = |side: usize| {
                    Some(radio.address[side].trim().to_string()).filter(|a| !a.is_empty())
                };
                // this side first, the AP's first in the input
                let [mut a, mut b] = [0, 1].map(|side| (radio.router_port[side], address(side)));
                let (ap, station) = if radio.own_ap {
                    (own, peer)
                } else {
                    std::mem::swap(&mut a, &mut b);
                    (peer, own)
                };
                Some(HelperChoice::RadioBridge {
                    ap,
                    station,
                    ap_router_port: a.0,
                    station_router_port: b.0,
                    ap_address: a.1,
                    station_address: b.1,
                    ssid: Some(radio.ssid.trim().to_string()).filter(|s| !s.is_empty()),
                })
            }
        }
    }
    /// This device among the radio bridges, if it is one.
    fn own_candidate(&self) -> Option<&Candidate> {
        self.candidate(self.device)
    }
    fn candidate(&self, id: u32) -> Option<&Candidate> {
        let candidates = self.radio_options.as_ref()?.as_ref().ok()?;
        candidates.iter().find(|c| c.device.id as u32 == id)
    }
    /// A port of the device or of another device of „PoE“.
    fn poe_port(&self, id: u32) -> Option<&PoePortFields> {
        let options = self.poe_options.as_ref()?.as_ref().ok()?;
        options
            .ports
            .iter()
            .chain(options.peers.iter().flat_map(|peer| &peer.ports))
            .find(|port| port.id as u32 == id)
    }
    fn poe_form(&self, ctx: &Context<Self>) -> Html {
        let options = match &self.poe_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => options,
        };
        let choices = |ports: &[PoePortFields]| -> Vec<Choice> {
            ports
                .iter()
                .map(|port| Choice {
                    id: port.id as u32,
                    text: t::poe_port(
                        &port.name,
                        port.label.as_deref(),
                        port.poe.as_ref().map(poe_mode),
                        port.connected
                            .first()
                            .map(|far| (far.device.name.as_str(), far.name.as_str())),
                        port.cabled,
                    ),
                })
                .collect()
        };
        let directions = vec![
            NetEntry { value: true, text: t::POE_FEEDS.to_string() },
            NetEntry { value: false, text: t::POE_FED.to_string() },
        ];
        let direction = directions.iter().find(|d| d.value == self.poe.feeds).cloned();
        let own = choices(&options.ports);
        let devices: Vec<Choice> = options
            .peers
            .iter()
            .map(|peer| Choice { id: peer.id as u32, text: peer.name.clone() })
            .collect();
        let peer_ports = self
            .poe
            .peer_device
            .and_then(|id| options.peers.iter().find(|peer| peer.id as u32 == id))
            .map(|peer| choices(&peer.ports))
            .unwrap_or_default();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let observed = match &self.poe_observed {
            _ if self.poe_observing => html!(<Spinner size={SpinnerSize::Md}/>),
            None => Html::default(),
            Some(Err(error)) => error.to_html(),
            Some(Ok(observed)) if observed.is_empty() => html!(<p>{t::POE_OBSERVED_NONE}</p>),
            Some(Ok(observed)) => html! {
                <ul class="poe-observed">
                { for observed.iter().map(|o| {
                    let far = o.far.as_ref();
                    // NetBox has it as the device does: the cable, PSE here and PD there
                    let mode = |id: i64| self.poe_port(id as u32).and_then(|p| p.poe.clone());
                    let recorded = !o.from_neighbor
                        && o.port.interface.as_ref().is_some_and(|port| mode(port.id) == Some(poe_options::PoeMode::PSE))
                        && far.is_some_and(|far| mode(far.id) == Some(poe_options::PoeMode::PD));
                    let text = t::poe_observed(
                        &o.port.name,
                        far.map(|far| (far.device.name.as_str(), far.name.as_str())),
                        o.from_neighbor,
                        recorded,
                    );
                    let take = o.port.interface.as_ref().filter(|_| !recorded).map(|port| {
                        let (port, far) = (
                            port.id as u32,
                            far.map(|far| (far.device.id as u32, far.id as u32)),
                        );
                        html! {
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::POE_TAKE}
                                onclick={ctx.link().callback(move |_| NetboxHelperMsg::PoeTake(port, far))}
                            />
                        }
                    });
                    html!(<li>{text}{" "}{take}</li>)
                })}
                </ul>
            },
        };
        html! {<>
            <div class="helper-actions">
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::POE_OBSERVE}
                    disabled={self.poe_observing}
                    onclick={ctx.link().callback(|_| NetboxHelperMsg::PoeObserve)}
                />
            </div>
            {observed}
            <Form>
                <FormGroup label={t::POE_DIRECTION}>
                    <Select<NetEntry<bool>>
                        selected={direction}
                        entries={directions}
                        onselect={ctx.link().callback(|entry: NetEntry<bool>| NetboxHelperMsg::PoeFeeds(entry.value))}
                    />
                </FormGroup>
                <FormGroup label={t::PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PORT}
                        selected={selected(&own, self.poe.port)}
                        entries={own.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::PoePort)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_DEVICE}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_DEVICE}
                        selected={selected(&devices, self.poe.peer_device)}
                        entries={devices.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::PoePeerDevice)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PEER_PORT}
                        selected={selected(&peer_ports, self.poe.peer)}
                        entries={peer_ports.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::PoePeer)}
                    />
                </FormGroup>
            </Form>
        </>}
    }
    fn radio_form(&self, ctx: &Context<Self>) -> Html {
        let candidates = match &self.radio_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(candidates)) => candidates,
        };
        let Some(own) = self.own_candidate() else {
            return html!(<p>{t::NOT_A_RADIO_BRIDGE}</p>);
        };
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let roles = vec![
            NetEntry { value: true, text: t::RADIO_AP.to_string() },
            NetEntry { value: false, text: t::RADIO_STATION.to_string() },
        ];
        let role = roles.iter().find(|r| r.value == self.radio.own_ap).cloned();
        let peers: Vec<Choice> = candidates
            .iter()
            .filter(|c| c.device.id != own.device.id)
            .map(|c| Choice {
                id: c.device.id as u32,
                text: t::radio_candidate(
                    &c.device.name,
                    &c.radio.name,
                    c.peer.as_ref().map(|p| p.name.as_str()),
                ),
            })
            .collect();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let routers: Vec<Choice> = ports
            .topology
            .all_devices
            .iter()
            .filter(|device| self.candidate(device.id as u32).is_none())
            .map(|device| Choice {
                id: device.id as u32,
                text: t::link_device(&device.name, device.site.as_ref().map(|s| s.name.as_str())),
            })
            .collect();
        let router_ports = |side: usize| -> Vec<Choice> {
            self.radio.router_device[side]
                .and_then(|id| ports.topology.all_devices.iter().find(|d| d.id as u32 == id))
                .map(|device| {
                    device
                        .link_ports
                        .iter()
                        .map(|port| Choice {
                            id: port.id as u32,
                            text: t::link_port(
                                &port.name,
                                port.label.as_deref(),
                                port.connected
                                    .first()
                                    .map(|far| (far.device.name.as_str(), far.name.as_str())),
                                port.cabled,
                                port.ipv4.as_deref(),
                                port.bridge.as_deref(),
                            ),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let peer = self.radio.peer.and_then(|id| self.candidate(id));
        let side = |index: usize, bridge: Option<&Candidate>, ap: bool| {
            let cabled = bridge.and_then(|b| b.router_port.as_ref());
            let placeholder = match cabled {
                Some(port) => format!("{}: {} {}", t::CABLED_ROUTER, port.device.name, port.name),
                None => t::CHOOSE_DEVICE.to_string(),
            };
            let address_placeholder = bridge
                .and_then(|b| b.address.clone())
                .unwrap_or_else(|| t::BRIDGE_ADDRESS_PLACEHOLDER.to_string());
            let title = bridge
                .map(|b| t::radio_side(&b.device.name, ap))
                .unwrap_or_default();
            let port_entries = router_ports(index);
            html! {<>
                <h4>{title}</h4>
                <FormGroup label={t::ROUTER_DEVICE}>
                    <Select<Choice>
                        placeholder={placeholder}
                        selected={selected(&routers, self.radio.router_device[index])}
                        entries={routers.clone()}
                        onselect={ctx.link().callback(move |c| NetboxHelperMsg::RadioRouterDevice(index, c))}
                    />
                </FormGroup>
                if self.radio.router_device[index].is_some() {
                    <FormGroup label={t::ROUTER_PORT}>
                        <Select<Choice>
                            placeholder={t::CHOOSE_PORT}
                            selected={selected(&port_entries, self.radio.router_port[index])}
                            entries={port_entries.clone()}
                            onselect={ctx.link().callback(move |c| NetboxHelperMsg::RadioRouterPort(index, c))}
                        />
                    </FormGroup>
                }
                <FormGroup label={t::BRIDGE_ADDRESS}>
                    <TextInput
                        value={self.radio.address[index].clone()}
                        placeholder={address_placeholder}
                        onchange={ctx.link().callback(move |a| NetboxHelperMsg::RadioAddress(index, a))}
                    />
                </FormGroup>
            </>}
        };
        html! {
            <Form>
                <FormGroup label={t::RADIO_ROLE}>
                    <Select<NetEntry<bool>>
                        selected={role}
                        entries={roles}
                        onselect={ctx.link().callback(|entry: NetEntry<bool>| NetboxHelperMsg::RadioOwnAp(entry.value))}
                    />
                </FormGroup>
                <FormGroup label={t::RADIO_PEER}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_DEVICE}
                        selected={selected(&peers, self.radio.peer)}
                        entries={peers.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::RadioPeer)}
                    />
                </FormGroup>
                <FormGroup label={t::SSID}>
                    <TextInput
                        value={self.radio.ssid.clone()}
                        placeholder={t::SSID_PLACEHOLDER}
                        onchange={ctx.link().callback(NetboxHelperMsg::RadioSsid)}
                    />
                </FormGroup>
                { side(0, Some(own), self.radio.own_ap) }
                if peer.is_some() {
                    { side(1, peer, !self.radio.own_ap) }
                }
            </Form>
        }
    }
    fn net_form(&self, ctx: &Context<Self>) -> Html {
        let options = match &self.net_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => options,
        };
        let targets: Vec<NetEntry<NetTarget>> = options
            .interfaces
            .iter()
            .map(|interface| NetEntry {
                value: NetTarget::Interface(interface.id as u32),
                text: t::net_port(&interface.name, interface.label.as_deref()),
            })
            .chain(options.vlans.iter().map(|vlan| NetEntry {
                value: NetTarget::Vlan(vlan.id as u32),
                text: t::net_vlan(&vlan.name, vlan.vid, vlan.interface.as_deref()),
            }))
            .collect();
        // with „keins“ first: a network may come in one family only
        let entries = |parents: Vec<(i64, &String)>| -> Vec<NetEntry<Option<u32>>> {
            (!parents.is_empty())
                .then(|| NetEntry {
                    value: None,
                    text: t::NO_PARENT.into(),
                })
                .into_iter()
                .chain(parents.into_iter().map(|(id, prefix)| NetEntry {
                    value: Some(id as u32),
                    text: prefix.clone(),
                }))
                .collect()
        };
        let parents = entries(options.parents.iter().map(|p| (p.id, &p.prefix)).collect());
        let parents_v6 = entries(options.parents_v6.iter().map(|p| (p.id, &p.prefix)).collect());
        let target = targets.iter().find(|e| Some(e.value) == self.net.target).cloned();
        let selected = |entries: &[NetEntry<Option<u32>>], value: Option<u32>| {
            value.and_then(|value| entries.iter().find(|e| e.value == Some(value)).cloned())
        };
        let parent = selected(&parents, self.net.parent);
        let parent_v6 = selected(&parents_v6, self.net.parent_v6);
        let dhcp = self.net.dhcp;
        html! {
            <Form>
                <FormGroup label={t::NET_TARGET}>
                    if targets.is_empty() {
                        <p>{t::NO_NET_TARGETS}</p>
                    } else {
                        <Select<NetEntry<NetTarget>>
                            placeholder={t::CHOOSE_NET_TARGET}
                            selected={target}
                            entries={targets}
                            onselect={ctx.link().callback(|entry: NetEntry<NetTarget>| NetboxHelperMsg::NetTarget(entry.value))}
                        />
                    }
                </FormGroup>
                <FormGroup label={t::PARENT}>
                    if parents.is_empty() {
                        <p>{t::NO_PARENTS}</p>
                    } else {
                        <Select<NetEntry<Option<u32>>>
                            placeholder={t::CHOOSE_PARENT}
                            selected={parent}
                            entries={parents}
                            onselect={ctx.link().callback(|entry: NetEntry<Option<u32>>| NetboxHelperMsg::NetParent(entry.value))}
                        />
                    }
                </FormGroup>
                <FormGroup label={t::PARENT_V6}>
                    if parents_v6.is_empty() {
                        <p>{t::NO_PARENTS_V6}</p>
                    } else {
                        <Select<NetEntry<Option<u32>>>
                            placeholder={t::CHOOSE_PARENT}
                            selected={parent_v6}
                            entries={parents_v6}
                            onselect={ctx.link().callback(|entry: NetEntry<Option<u32>>| NetboxHelperMsg::NetParentV6(entry.value))}
                        />
                    }
                </FormGroup>
                <FormGroup label={t::LENGTH}>
                    <TextInput
                        value={self.net.length.clone()}
                        placeholder={t::LENGTH_PLACEHOLDER}
                        onchange={ctx.link().callback(NetboxHelperMsg::NetLength)}
                    />
                </FormGroup>
                <FormGroup>
                    <label class="net-dhcp">
                        <input
                            type="checkbox"
                            checked={dhcp}
                            onchange={ctx.link().callback(move |_| NetboxHelperMsg::NetDhcp(!dhcp))}
                        />
                        {" "}{t::DHCP_SERVER}
                    </label>
                </FormGroup>
            </Form>
        }
    }
    fn port_vlan_form(&self, ctx: &Context<Self>) -> Html {
        let options = match &self.port_vlan_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => options,
        };
        let ports: Vec<Choice> = options
            .ports
            .iter()
            .map(|port| Choice {
                id: port.id as u32,
                text: t::port_vlan_port(
                    &port.name,
                    port.label.as_deref(),
                    port.vlan.as_ref().map(|vlan| (vlan.vid, vlan.name.as_str())),
                ),
            })
            .collect();
        let groups: Vec<Choice> = options
            .groups
            .iter()
            .map(|group| Choice {
                id: group.id as u32,
                text: t::port_vlan_group(&group.name, group.vxlan),
            })
            .collect();
        let vlans: Vec<Choice> = options
            .groups
            .iter()
            .filter(|group| Some(group.id as u32) == self.port_vlan.group)
            .flat_map(|group| &group.vlans)
            .map(|vlan| Choice {
                id: vlan.id as u32,
                text: t::port_vlan_vlan(vlan.vid, &vlan.name),
            })
            .collect();
        let port = ports.iter().find(|c| Some(c.id) == self.port_vlan.port).cloned();
        let group = groups.iter().find(|c| Some(c.id) == self.port_vlan.group).cloned();
        let vlan = vlans.iter().find(|c| Some(c.id) == self.port_vlan.vlan).cloned();
        html! {
            <Form>
                <FormGroup label={t::PORT_VLAN_PORT}>
                    if ports.is_empty() {
                        <p>{t::NO_PORT_VLAN_PORTS}</p>
                    } else {
                        <Select<Choice>
                            placeholder={t::CHOOSE_PORT_VLAN_PORT}
                            selected={port}
                            entries={ports}
                            onselect={ctx.link().callback(|choice: Choice| NetboxHelperMsg::PortVlanPort(choice.id))}
                        />
                    }
                </FormGroup>
                if groups.is_empty() {
                    <FormGroup label={t::PORT_VLAN_VLAN}>
                        <p>{t::NO_PORT_VLAN_VLANS}</p>
                    </FormGroup>
                } else {
                    <FormGroup label={t::PORT_VLAN_GROUP}>
                        <Select<Choice>
                            placeholder={t::CHOOSE_PORT_VLAN_GROUP}
                            selected={group}
                            entries={groups}
                            onselect={ctx.link().callback(|choice: Choice| NetboxHelperMsg::PortVlanGroup(choice.id))}
                        />
                    </FormGroup>
                    if self.port_vlan.group.is_some() {
                        <FormGroup label={t::PORT_VLAN_VLAN}>
                            <Select<Choice>
                                placeholder={t::CHOOSE_PORT_VLAN_VLAN}
                                selected={vlan}
                                entries={vlans}
                                onselect={ctx.link().callback(|choice: Choice| NetboxHelperMsg::PortVlanVlan(choice.id))}
                            />
                        </FormGroup>
                    }
                }
            </Form>
        }
    }
    fn flag_changes(&self) -> Vec<(u32, Flag, bool)> {
        self.flags
            .iter()
            .map(|(&(interface, flag), &set)| (interface, flag, set))
            .collect()
    }
    fn flag_interface(&self, id: u32) -> Option<&FlagInterface> {
        self.flag_interfaces
            .as_ref()?
            .as_ref()
            .ok()?
            .iter()
            .find(|interface| interface.id as u32 == id)
    }
    /// Fetches the device's findings after the switches, unless they are there already.
    fn check_flags(&mut self, ctx: &Context<Self>) {
        let changes = self.flag_changes();
        if self.flag_findings.as_ref().is_some_and(|(shown, _)| *shown == changes) {
            return;
        }
        self.flag_findings = None;
        fetch_flag_findings(ctx.link().clone(), ctx.props().id, changes);
    }
    fn flags_form(&self, ctx: &Context<Self>) -> Html {
        let interfaces = match &self.flag_interfaces {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(interfaces)) => interfaces,
        };
        let rows = interfaces.iter().map(|interface| {
            let id = interface.id as u32;
            let name = match &interface.label {
                Some(label) => format!("{} ({label})", interface.name),
                None => interface.name.clone(),
            };
            let mut about: Vec<String> = interface.addresses.clone();
            about.extend(
                interface
                    .connected
                    .iter()
                    .map(|far| t::cabled_to(&far.device.name, &far.name)),
            );
            if let Some(bridge) = &interface.bridge {
                about.push(t::bridge_port(bridge));
            }
            let switches = FLAGS.iter().map(|&flag| {
                let has = interface.tags.iter().any(|tag| tag_slug(tag) == flag.slug());
                let set = self.flags.get(&(id, flag)).copied().unwrap_or(has);
                let class = (set != has).then_some("flag-changed");
                let onchange = ctx.link().callback(move |_| NetboxHelperMsg::Toggle(id, flag, !set));
                html! {
                    <td class={classes!("pf-v6-c-table__td", class)} data-label={flag.slug()}>
                        <input
                            type="checkbox"
                            checked={set}
                            aria-label={format!("{} {}", interface.name, flag.slug())}
                            disabled={self.job_running}
                            {onchange}
                        />
                    </td>
                }
            });
            html! {
                <tr class="pf-v6-c-table__tr" key={id}>
                    <td class="pf-v6-c-table__td" data-label={t::INTERFACE}>
                        <div>{name}</div>
                        <small>{about.join(", ")}</small>
                    </td>
                    { for switches }
                </tr>
            }
        });
        let findings = match &self.flag_findings {
            None => html!(<Spinner size={SpinnerSize::Md}/>),
            Some((_, Err(error))) => error.to_html(),
            Some((changes, Ok((_, findings)))) if findings.is_empty() => {
                html!(<p>{t::no_flag_findings(!changes.is_empty())}</p>)
            }
            Some((changes, Ok((netbox, findings)))) => html! {<>
                <p>{t::flag_findings(!changes.is_empty())}</p>
                { finding_list(netbox, findings) }
            </>},
        };
        html! {<>
            <div class="flag-table">
                <table class="pf-v6-c-table pf-m-compact pf-m-grid-md" role="grid">
                    <thead class="pf-v6-c-table__thead">
                        <tr class="pf-v6-c-table__tr">
                            <th class="pf-v6-c-table__th">{t::INTERFACE}</th>
                            { for FLAGS.iter().map(|flag| html!(<th class="pf-v6-c-table__th">{flag.slug()}</th>)) }
                        </tr>
                    </thead>
                    <tbody class="pf-v6-c-table__tbody">{ for rows }</tbody>
                </table>
            </div>
            <div class="helper-findings">{findings}</div>
        </>}
    }
    fn choose(&mut self, ctx: &Context<Self>) {
        let choice = self.choice();
        if choice == self.planned {
            return;
        }
        self.planned = choice.clone();
        self.plan = None;
        self.removed = false;
        self.preview = None;
        if let Some(choice) = choice {
            fetch_plan(ctx.link().clone(), ctx.props().id, choice);
        }
    }
    fn ports(&self) -> Option<&link_ports::ResponseData> {
        self.ports.as_ref().and_then(|ports| ports.as_ref().ok())
    }
    fn port(&self, id: u32) -> Option<&LinkPortFields> {
        self.ports()?
            .topology
            .device_by_id
            .as_ref()?
            .link_ports
            .iter()
            .find(|port| port.id as u32 == id)
    }
    /// A port of any router, with its device's name.
    fn any_port(&self, id: u32) -> Option<(&str, &LinkPortFields)> {
        self.ports()?.topology.all_devices.iter().find_map(|device| {
            device
                .link_ports
                .iter()
                .find(|port| port.id as u32 == id)
                .map(|port| (device.name.as_str(), port))
        })
    }
    /// The chosen ends of „Verbindung“ whose link is in the way: a cable elsewhere, or an IPv4
    /// not in a network with the other end.
    fn in_the_way(&self) -> Vec<(String, u32)> {
        let (Some(own), Some(peer)) = (self.link.port, self.link.peer) else {
            return Vec::new();
        };
        let ends = [(own, peer), (peer, own)];
        ends.iter()
            .filter_map(|&(end, other)| {
                let (device, port) = self.any_port(end).or_else(|| {
                    // the device's own port (the device may not be a router in the list)
                    let device = self.ports()?.topology.device_by_id.as_ref()?;
                    self.port(end).map(|port| (device.name.as_str(), port))
                })?;
                let other_port = self.any_port(other).map(|(_, p)| p).or_else(|| self.port(other));
                let cabled_elsewhere = port.cabled
                    && !port.connected.iter().any(|far| far.id as u32 == other);
                let addressed_elsewhere = port.ipv4.as_deref().is_some_and(|own| {
                    !other_port
                        .and_then(|other| other.ipv4.as_deref())
                        .is_some_and(|other| same_network(own, other))
                });
                (cabled_elsewhere || addressed_elsewhere)
                    .then(|| (t::disconnect_port(device, &port.name), end))
            })
            .collect()
    }
    fn disconnect_form(&self, ctx: &Context<Self>) -> Html {
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let mut entries: Vec<Choice> = ports
            .topology
            .device_by_id
            .iter()
            .flat_map(|device| &device.link_ports)
            .filter(|port| port.cabled || port.ipv4.is_some())
            .map(|port| Choice {
                id: port.id as u32,
                text: t::link_port(
                    &port.name,
                    port.label.as_deref(),
                    port.connected
                        .first()
                        .map(|far| (far.device.name.as_str(), far.name.as_str())),
                    port.cabled,
                    port.ipv4.as_deref(),
                    port.bridge.as_deref(),
                ),
            })
            .collect();
        // a port at the far end, from „Verbindung“
        if let Some(id) = self.disconnect
            && !entries.iter().any(|entry| entry.id == id)
            && let Some((device, port)) = self.any_port(id)
        {
            entries.push(Choice {
                id,
                text: format!("{device} {}", port.name),
            });
        }
        let selected = self
            .disconnect
            .and_then(|id| entries.iter().find(|entry| entry.id == id).cloned());
        html! {
            <Form>
                <FormGroup label={t::PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PORT}
                        selected={selected}
                        entries={entries}
                        onselect={ctx.link().callback(|port: Choice| NetboxHelperMsg::DisconnectPort(port.id))}
                    />
                </FormGroup>
            </Form>
        }
    }
    fn link_form(&self, ctx: &Context<Self>) -> Html {
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let choices = |ports: &[LinkPortFields]| -> Vec<Choice> {
            ports
                .iter()
                .map(|port| Choice {
                    id: port.id as u32,
                    text: t::link_port(
                        &port.name,
                        port.label.as_deref(),
                        port.connected
                            .first()
                            .map(|far| (far.device.name.as_str(), far.name.as_str())),
                        port.cabled,
                        port.ipv4.as_deref(),
                        port.bridge.as_deref(),
                    ),
                })
                .collect()
        };
        let own = ports
            .topology
            .device_by_id
            .as_ref()
            .map(|device| choices(&device.link_ports))
            .unwrap_or_default();
        let devices: Vec<Choice> = ports
            .topology
            .all_devices
            .iter()
            .filter(|device| device.id as u32 != ctx.props().id)
            .map(|device| Choice {
                id: device.id as u32,
                text: t::link_device(&device.name, device.site.as_ref().map(|s| s.name.as_str())),
            })
            .collect();
        let peer_ports = self
            .link
            .peer_device
            .and_then(|id| ports.topology.all_devices.iter().find(|d| d.id as u32 == id))
            .map(|device| choices(&device.link_ports))
            .unwrap_or_default();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let mtu = self.link.mtu.clone();
        let in_the_way = self.in_the_way();
        html! {
            <Form>
                <FormGroup label={t::PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PORT}
                        selected={selected(&own, self.link.port)}
                        entries={own.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::Port)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_DEVICE}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_DEVICE}
                        selected={selected(&devices, self.link.peer_device)}
                        entries={devices.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::PeerDevice)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PEER_PORT}
                        selected={selected(&peer_ports, self.link.peer)}
                        entries={peer_ports.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::Peer)}
                    />
                </FormGroup>
                <FormGroup label={t::MTU}>
                    <TextInput
                        value={mtu}
                        placeholder={t::MTU_PLACEHOLDER}
                        onchange={ctx.link().callback(NetboxHelperMsg::Mtu)}
                    />
                </FormGroup>
                if !in_the_way.is_empty() {
                    <div class="helper-actions">
                        { for in_the_way.into_iter().map(|(label, port)| html! {
                            <Button
                                variant={ButtonVariant::Secondary}
                                label={label}
                                onclick={ctx.link().callback(move |_| NetboxHelperMsg::DisconnectPort(port))}
                            />
                        })}
                    </div>
                }
            </Form>
        }
    }
    fn preview_view(&self) -> Html {
        if self.preview_loading {
            return html!(<Spinner size={SpinnerSize::Md}/>);
        }
        match &self.preview {
            None => Html::default(),
            Some(Err(error)) => error.to_html(),
            Some(Ok(preview)) => {
                let gone: Vec<&String> =
                    preview.before.iter().filter(|kind| !preview.after.contains(kind)).collect();
                html! {<div class="helper-preview">
                    if preview.after.is_empty() {
                        <p>{t::NO_FINDINGS_AFTER}</p>
                    } else {
                        <p>{t::FINDINGS_AFTER}{" "}{preview.after.join(", ")}</p>
                    }
                    if !gone.is_empty() {
                        <p>{t::findings_gone(&gone)}</p>
                    }
                    { match &preview.script {
                        None => html!(<p>{t::PREVIEW_UNREACHABLE}</p>),
                        Some(script) if script.trim().is_empty() => html!(<p>{t::PREVIEW_NOTHING}</p>),
                        Some(script) => html! {<>
                            <p>{t::PREVIEW_TITLE}</p>
                            <CodeBlock><CodeBlockCode>{script.clone()}</CodeBlockCode></CodeBlock>
                        </>},
                    }}
                </div>}
            }
        }
    }
}

fn describe(step: &Step) -> String {
    match step {
        Step::CreateInterface(create) => t::create_interface(
            &create.name,
            &create.kind,
            &create.tags.iter().map(tag_slug).collect::<Vec<_>>(),
        ),
        Step::CreateAddress(create) => {
            t::create_address(&create.address, &create.device.name, &create.interface, &create.prefix)
        }
        Step::CreateCable(create) => t::create_cable(
            (&create.a.device.name, &create.a.name),
            (&create.b.device.name, &create.b.name),
        ),
        Step::CreatePrefix(create) => {
            t::create_prefix(&create.prefix, &create.pool, &create.description)
        }
        Step::UpdateInterface(update) => t::update_interface(
            (&update.interface.device.name, &update.interface.name),
            update
                .label
                .as_deref()
                .map(|label| (label, update.label_before.as_deref())),
            update.mtu.map(|mtu| (mtu, update.mtu_before)),
            &update.tags_added.iter().map(tag_slug).collect::<Vec<_>>(),
            &update.tags_removed.iter().map(tag_slug).collect::<Vec<_>>(),
            update.poe.as_ref().map(|poe| (plan_poe_mode(poe), update.poe_before.as_ref().map(plan_poe_mode))),
        ),
        Step::DeleteCable(delete) => t::delete_cable(
            (&delete.port.device.name, &delete.port.name),
            delete.far.as_ref().map(|far| (far.device.name.as_str(), far.name.as_str())),
        ),
        Step::DeleteAddress(delete) => t::delete_address(
            &delete.address,
            &delete.interface.device.name,
            &delete.interface.name,
        ),
        Step::DeletePrefix(delete) => t::delete_prefix(&delete.prefix),
        Step::UpdatePort(update) => t::update_port(
            (&update.interface.device.name, &update.interface.name),
            update.name.as_deref(),
            update
                .rf_role
                .as_ref()
                .map(|role| (rf_role(role), update.rf_role_before.as_ref().map(rf_role))),
            update.bridge.as_deref(),
        ),
        Step::CreateWirelessLink(create) => t::create_wireless_link(
            (&create.a.device.name, &create.a.name),
            (&create.b.device.name, &create.b.name),
            &create.ssid,
        ),
        Step::UpdatePortVlan(update) => {
            fn vlan(vlan: &netbox_plan::PlanVlan) -> (i64, &str) {
                (vlan.vid, vlan.name.as_str())
            }
            t::update_port_vlan(
                (&update.interface.device.name, &update.interface.name),
                update.vlan.as_ref().map(vlan),
                update.vlan_before.as_ref().map(vlan),
                &update.tagged_before.iter().map(vlan).collect::<Vec<_>>(),
            )
        }
        Step::DeleteInterface(delete) => {
            t::delete_interface((&delete.interface.device.name, &delete.interface.name))
        }
        Step::DeleteWirelessLink(delete) => t::delete_wireless_link(
            (&delete.a.device.name, &delete.a.name),
            (&delete.b.device.name, &delete.b.name),
        ),
        Step::UpdateWirelessLink(update) => t::update_wireless_link(
            (&update.a.device.name, &update.a.name),
            (&update.b.device.name, &update.b.name),
            update.ssid.as_deref().map(|ssid| (ssid, update.ssid_before.as_str())),
            update.psk_set,
        ),
        Step::SetPrimaryIp(set) => t::set_primary(
            match set.version {
                netbox_plan::IpVersion::V4 => "IPv4",
                netbox_plan::IpVersion::V6 => "IPv6",
                netbox_plan::IpVersion::Other(_) => "IP",
            },
            &set.address,
            set.before.as_deref(),
        ),
    }
}

/// The newest helper job of the device, if it is a finished write: it can still be taken back
/// (until the server restarts, which forgets its jobs).
fn last_write(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let Ok(data) = query_authenticated::<HelperJobs, _>(
            scope.clone(),
            helper_jobs::Variables { device_id: id as i64 },
        )
        .await
        else {
            return;
        };
        let last = data
            .jobs
            .into_iter()
            .find(|job| matches!(job.kind, JobKind::NETBOX_WRITE | JobKind::NETBOX_UNDO));
        if let Some(job) = last
            && matches!(job.kind, JobKind::NETBOX_WRITE)
            && matches!(job.state, JobState::DONE)
        {
            scope.send_message(NetboxHelperMsg::LastWrite(job.id));
        }
    });
}

fn fetch_plan(scope: Scope<NetboxHelper>, id: u32, choice: HelperChoice) {
    spawn_local(async move {
        let plan = query_authenticated::<NetboxPlan, _>(
            scope.clone(),
            netbox_plan::Variables {
                helper: helper_input!(netbox_plan, choice.clone(), id),
            },
        )
        .await;
        scope.send_message(NetboxHelperMsg::Plan(choice, plan));
    });
}

fn fetch_net_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<CustomerNetOptions, _>(
            scope.clone(),
            customer_net_options::Variables { id: id as i64 },
        )
        .await
        .map(|data| match data.topology.device_by_id {
            Some(device) => device.customer_net_options,
            None => NetOptions {
                interfaces: Vec::new(),
                vlans: Vec::new(),
                parents: Vec::new(),
                parents_v6: Vec::new(),
            },
        });
        scope.send_message(NetboxHelperMsg::NetOptions(options));
    });
}

fn fetch_port_vlan_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<PortVlanOptions, _>(
            scope.clone(),
            port_vlan_options::Variables { id: id as i64 },
        )
        .await
        .map(|data| match data.topology.device_by_id {
            Some(device) => device.port_vlan_options,
            None => PortVlanOptionsData {
                ports: Vec::new(),
                groups: Vec::new(),
                group: None,
            },
        });
        scope.send_message(NetboxHelperMsg::PortVlanOptions(options));
    });
}

fn fetch_radio_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<RadioBridgeCandidates, _>(
            scope.clone(),
            radio_bridge_candidates::Variables { id: id as i64 },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .map(|device| device.radio_bridge_candidates)
                .unwrap_or_default()
        });
        scope.send_message(NetboxHelperMsg::RadioOptions(options));
    });
}

fn fetch_poe_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<PoeOptions, _>(
            scope.clone(),
            poe_options::Variables { id: id as i64 },
        )
        .await
        .map(|data| match data.topology.device_by_id {
            Some(device) => device.poe_options,
            None => PoeOptionsData {
                ports: Vec::new(),
                peers: Vec::new(),
            },
        });
        scope.send_message(NetboxHelperMsg::PoeOptions(options));
    });
}

/// What the device reports feeding, reached as the page reaches it.
fn fetch_poe_observed(scope: Scope<NetboxHelper>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| {
                poe_observed::AdhocCredentials { username, password }
            });
        let observed = query_authenticated::<PoeObserved, _>(
            scope.clone(),
            poe_observed::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .and_then(|device| device.access)
                .map(|access| access.poe_observed)
                .unwrap_or_default()
        });
        scope.send_message(NetboxHelperMsg::PoeObserved(observed));
    });
}

fn fetch_flag_interfaces(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let interfaces = query_authenticated::<FlagInterfaces, _>(
            scope.clone(),
            flag_interfaces::Variables { id: id as i64 },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .map(|device| device.flag_interfaces)
                .unwrap_or_default()
        });
        scope.send_message(NetboxHelperMsg::FlagInterfaces(interfaces));
    });
}

/// The device's findings as they would be with `changes` (as they are without any).
fn fetch_flag_findings(scope: Scope<NetboxHelper>, id: u32, changes: Vec<(u32, Flag, bool)>) {
    spawn_local(async move {
        let after = (!changes.is_empty())
            .then(|| helper_input!(findings, HelperChoice::Flags(changes.clone()), id));
        let result = query_authenticated::<Findings, _>(
            scope.clone(),
            findings::Variables {
                site: None,
                device: Some(id as i64),
                after,
            },
        )
        .await
        .map(|data| (data.netbox_url, data.topology.findings));
        scope.send_message(NetboxHelperMsg::FlagFindings(changes, result));
    });
}

fn fetch_ports(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let ports = query_authenticated::<LinkPorts, _>(
            scope.clone(),
            link_ports::Variables { id: id as i64 },
        )
        .await;
        scope.send_message(NetboxHelperMsg::Ports(ports));
    });
}

fn fetch_preview(scope: Scope<NetboxHelper>, id: u32, choice: HelperChoice, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| {
                netbox_plan_preview::AdhocCredentials { username, password }
            });
        let preview = query_authenticated::<NetboxPlanPreview, _>(
            scope.clone(),
            netbox_plan_preview::Variables {
                id: id as i64,
                helper: helper_input!(netbox_plan_preview, choice, id),
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| {
            let kinds = |findings: Vec<String>| findings.into_iter().map(t::finding_kind).collect();
            Preview {
                before: kinds(
                    data.topology
                        .device_by_id
                        .map(|d| d.findings.into_iter().map(|f| format!("{:?}", f.kind)).collect())
                        .unwrap_or_default(),
                ),
                after: kinds(
                    data.netbox_plan
                        .preview
                        .findings
                        .into_iter()
                        .map(|f| format!("{:?}", f.kind))
                        .collect(),
                ),
                script: data
                    .netbox_plan
                    .preview_access
                    .and_then(|access| access.generate_cfg)
                    .map(|cfg| cfg.script),
            }
        });
        scope.send_message(NetboxHelperMsg::Preview(preview));
    });
}

/// Whether two IPv4 addresses with their length (`172.17.1.41/30`) are in one network.
fn same_network(a: &str, b: &str) -> bool {
    let parse = |text: &str| -> Option<(u32, u32)> {
        let (address, len) = text.split_once('/')?;
        let address: std::net::Ipv4Addr = address.parse().ok()?;
        let len: u32 = len.parse().ok()?;
        let mask = u32::MAX.checked_shl(32 - len).unwrap_or(0);
        Some((u32::from(address) & mask, len))
    };
    matches!((parse(a), parse(b)), (Some(a), Some(b)) if a == b)
}

type FlagInterface = flag_interfaces::FlagInterfacesTopologyDeviceByIdFlagInterfaces;

/// The device's findings after these switches, with NetBox' address.
type FlagFindings = (
    Vec<(u32, Flag, bool)>,
    Result<(String, Vec<FindingsTopologyFindings>), FrontendError>,
);

/// A tag as NetBox names it, from the `InterfaceTag` of a query (each has its own).
trait TagSlug {
    fn slug(&self) -> &str;
}

macro_rules! tag_slug {
    ($($module:ident),*) => {$(
        impl TagSlug for $module::InterfaceTag {
            fn slug(&self) -> &str {
                match self {
                    $module::InterfaceTag::OSPF => "ospf",
                    $module::InterfaceTag::DHCP => "dhcp",
                    $module::InterfaceTag::DHCP_CLIENT => "dhcp-client",
                    $module::InterfaceTag::NAT => "nat",
                    $module::InterfaceTag::Other(other) => other,
                }
            }
        }
    )*};
}

tag_slug!(flag_interfaces, netbox_plan);

fn tag_slug(tag: &impl TagSlug) -> &str {
    tag.slug()
}

type NetOptions = customer_net_options::CustomerNetOptionsTopologyDeviceByIdCustomerNetOptions;

type PoeOptionsData = poe_options::PoeOptionsTopologyDeviceByIdPoeOptions;

type PortVlanOptionsData = port_vlan_options::PortVlanOptionsTopologyDeviceByIdPortVlanOptions;

type Observed = poe_observed::PoeObservedTopologyDeviceByIdAccessPoeObserved;

/// NetBox' PoE mode as the UI shows it.
fn poe_mode(mode: &poe_options::PoeMode) -> &str {
    match mode {
        poe_options::PoeMode::PSE => "PSE",
        poe_options::PoeMode::PD => "PD",
        poe_options::PoeMode::Other(other) => other,
    }
}

type Candidate =
    radio_bridge_candidates::RadioBridgeCandidatesTopologyDeviceByIdRadioBridgeCandidates;

fn rf_role(role: &netbox_plan::RadioRole) -> &str {
    match role {
        netbox_plan::RadioRole::AP => "AP",
        netbox_plan::RadioRole::STATION => "Station",
        netbox_plan::RadioRole::Other(other) => other,
    }
}

fn plan_poe_mode(mode: &netbox_plan::PoeMode) -> &str {
    match mode {
        netbox_plan::PoeMode::PSE => "PSE",
        netbox_plan::PoeMode::PD => "PD",
        netbox_plan::PoeMode::Other(other) => other,
    }
}
