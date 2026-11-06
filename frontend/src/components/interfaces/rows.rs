//! The rows of `DeviceAccess.interfaceView` as the card shows them: the task and the actions
//! translated, the rest as the query has it. The findings are those of `topology.findings`, so
//! `components::findings` words them as everywhere else.
use crate::{
    components::netbox_helper::{
        HelperKind, HelperPreset, LinkPreset, NetTarget, RadioPreset, SeenPreset,
    },
    error::FrontendError,
    i18n::interfaces as t,
    graphql::authenticated::{
        findings::Finding,
        interfaces::{self, ActionHelper, ActionKind, InterfaceTask, RadioPath, RowAction},
        types::{AddressRef, DeviceRef, InterfaceRef, VlanRef},
    },
};

/// What an interface is for, in the order of the card.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Task {
    Loopback,
    Uplink,
    Direct,
    RadioLink,
    Customer,
    Unknown,
}

impl Task {
    pub const ALL: [Task; 6] = [
        Task::Loopback,
        Task::Uplink,
        Task::Direct,
        Task::RadioLink,
        Task::Customer,
        Task::Unknown,
    ];
    /// for the browser's memory of the filter
    pub fn key(self) -> &'static str {
        match self {
            Task::Loopback => "loopback",
            Task::Uplink => "uplink",
            Task::Direct => "direct",
            Task::RadioLink => "radio",
            Task::Customer => "customer",
            Task::Unknown => "unknown",
        }
    }
    fn of(task: &InterfaceTask) -> Task {
        match task {
            InterfaceTask::Loopback => Task::Loopback,
            InterfaceTask::Uplink => Task::Uplink,
            InterfaceTask::Direct => Task::Direct,
            InterfaceTask::RadioLink => Task::RadioLink,
            InterfaceTask::Customer => Task::Customer,
            InterfaceTask::Unknown | InterfaceTask::Other => Task::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub task: Task,
    pub interface: InterfaceRef,
    pub label: Option<String>,
    pub ports: Vec<InterfaceRef>,
    pub addresses: Vec<AddressRef>,
    pub ospf: bool,
    pub dhcp: bool,
    pub dhcp_client: bool,
    pub nat: bool,
    pub internet: bool,
    pub mtu: Option<i32>,
    pub untagged_vlan: Option<VlanRef>,
    pub tagged_vlans: Vec<VlanRef>,
    pub far_ends: Vec<InterfaceRef>,
    pub radio_path: Option<RadioPath>,
    pub gateways: Vec<DeviceRef>,
    pub free: bool,
    pub findings: Vec<Finding>,
    pub actions: Vec<Action>,
    /// a bridge's ports, each chosen on its own
    pub port_rows: Vec<PortRow>,
}

/// A port of a bridge with its own actions.
#[derive(Debug, Clone, PartialEq)]
pub struct PortRow {
    pub interface: InterfaceRef,
    pub label: Option<String>,
    pub far_ends: Vec<InterfaceRef>,
    pub actions: Vec<Action>,
}

/// What a button of a row does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Kind {
    Suggestion,
    Remove,
    AssignRole,
    /// a setting of the interface: PoE, tags
    Edit,
}

/// A row's action: what it is and the helper it opens with what.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub kind: Kind,
    pub helper: HelperKind,
    pub preset: HelperPreset,
    /// the port it starts from: a bridge's port shows its own
    pub interface: Option<u32>,
}

impl Action {
    /// its button or menu entry: „Adresse übernehmen“ names the address, a row may have several
    pub fn label(&self) -> String {
        match &self.preset {
            HelperPreset::AddressTakeover { address, .. } => t::address_takeover(address),
            HelperPreset::LeaseTakeover { address, .. } => t::lease_takeover(address),
            HelperPreset::PoeRemove(_) => t::POE_REMOVE.to_string(),
            HelperPreset::Poe(_) if self.kind == Kind::Suggestion => t::POE_COMPLETE.to_string(),
            _ => t::action(self.kind, self.helper),
        }
    }
}

/// The view of a device: its rows, the findings about the device (shown with the loopback) and
/// the ones on none of its interfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub netbox: String,
    pub rows: Vec<Row>,
    pub device_findings: Vec<Finding>,
    pub other_findings: Vec<Finding>,
    pub loopback_actions: Vec<Action>,
}

impl View {
    pub fn of(data: interfaces::DeviceInterfaces) -> Result<View, FrontendError> {
        let view = data
            .topology
            .device_by_id
            .ok_or(FrontendError::MissingData)?
            .interface_view;
        Ok(View {
            netbox: data.topology.netbox_url,
            rows: view.rows.into_iter().map(Row::of).collect(),
            device_findings: view.device_findings,
            other_findings: view.other_findings,
            loopback_actions: view
                .loopback_actions
                .into_iter()
                .filter_map(|a| action(a, 0))
                .collect(),
        })
    }
}

impl Row {
    fn of(row: interfaces::InterfaceRow) -> Row {
        let own_device = row.interface.device.id as u32;
        let actions = |actions: Vec<RowAction>| {
            actions.into_iter().filter_map(|a| action(a, own_device)).collect()
        };
        Row {
            task: Task::of(&row.task),
            interface: row.interface,
            label: row.label,
            ports: row.ports,
            addresses: row.addresses,
            ospf: row.ospf,
            dhcp: row.dhcp,
            dhcp_client: row.dhcp_client,
            nat: row.nat,
            internet: row.internet,
            mtu: row.mtu,
            untagged_vlan: row.untagged_vlan,
            tagged_vlans: row.tagged_vlans,
            far_ends: row.far_ends,
            radio_path: row.radio_path,
            gateways: row.gateways,
            free: row.free,
            findings: row.findings,
            actions: actions(row.actions),
            port_rows: row
                .port_rows
                .into_iter()
                .map(|port| PortRow {
                    interface: port.interface,
                    label: port.label,
                    far_ends: port.far_ends,
                    actions: actions(port.actions),
                })
                .collect(),
        }
    }
}

/// The helper and its start for an action of the backend on a row of `device`; none for one it
/// doesn't know.
pub(super) fn action(action: RowAction, device: u32) -> Option<Action> {
    let kind = match action.kind {
        ActionKind::Suggestion => Kind::Suggestion,
        ActionKind::Remove => Kind::Remove,
        ActionKind::AssignRole => Kind::AssignRole,
        ActionKind::Edit => Kind::Edit,
        ActionKind::Other => return None,
    };
    let interface = action.interface.map(|id| id as u32);
    let (helper, preset) = match action.helper {
        ActionHelper::Loopback => (HelperKind::Loopback, HelperPreset::Kind(HelperKind::Loopback)),
        ActionHelper::Connection => (
            HelperKind::Connection,
            match (interface, action.peer) {
                (Some(port), Some(peer)) => HelperPreset::Link(LinkPreset {
                    port,
                    peer_device: peer.device.id as u32,
                    peer: peer.id as u32,
                }),
                (Some(port), None) => HelperPreset::LinkFrom(port),
                (None, _) => HelperPreset::Kind(HelperKind::Connection),
            },
        ),
        ActionHelper::Disconnect => (HelperKind::Disconnect, HelperPreset::Disconnect(interface?)),
        ActionHelper::CustomerDisconnect => (
            HelperKind::CustomerDisconnect,
            HelperPreset::CustomerDisconnect(interface?),
        ),
        ActionHelper::PortVlan => (
            HelperKind::PortVlan,
            HelperPreset::PortVlan {
                port: interface,
                vlan: action.vlan.map(|vlan| vlan as u32),
            },
        ),
        ActionHelper::Poe => (HelperKind::Poe, HelperPreset::Poe(interface?)),
        ActionHelper::PoeRemove => (HelperKind::Poe, HelperPreset::PoeRemove(interface?)),
        ActionHelper::BridgeLeave => (HelperKind::BridgeLeave, HelperPreset::BridgeLeave(interface?)),
        ActionHelper::LeaseTakeover => {
            let lease = action.lease?;
            (
                HelperKind::LeaseTakeover,
                HelperPreset::LeaseTakeover {
                    address: action.address?,
                    mac: lease.mac,
                    name: lease.name,
                },
            )
        }
        ActionHelper::AddressTakeover => (
            HelperKind::AddressTakeover,
            HelperPreset::AddressTakeover {
                interface: interface?,
                address: action.address?,
            },
        ),
        ActionHelper::NewDevice => (
            HelperKind::NewDevice,
            HelperPreset::NewDevice {
                port: interface?,
                seen: action.seen.map(|seen| SeenPreset {
                    identity: seen.identity,
                    board: seen.board,
                    interface_name: seen.interface_name,
                }),
            },
        ),
        ActionHelper::Flags => (HelperKind::Flags, HelperPreset::Flags(interface?)),
        ActionHelper::CableTakeover => (
            HelperKind::CableTakeover,
            HelperPreset::CableTakeover {
                port: interface?,
                peer: action.peer?.id as u32,
            },
        ),
        ActionHelper::RadioDisconnect => (
            HelperKind::RadioDisconnect,
            HelperPreset::RadioDisconnect(interface?),
        ),
        ActionHelper::CustomerNet => (
            HelperKind::CustomerNet,
            HelperPreset::CustomerNet(match (action.vlan, interface) {
                (Some(vlan), _) => NetTarget::Vlan(vlan as u32),
                (None, Some(port)) => NetTarget::Interface(port),
                (None, None) => return None,
            }),
        ),
        ActionHelper::RadioBridge => (
            HelperKind::RadioBridge,
            match (interface, action.peer) {
                // from the router port: for the bridge device cabled to it
                (Some(router_port), Some(bridge_port)) => HelperPreset::RadioFrom(RadioPreset {
                    bridge: bridge_port.device.id as u32,
                    router_device: device,
                    router_port,
                }),
                // from a free router port: the bridge device is chosen first
                (Some(router_port), None) => HelperPreset::RadioChoose {
                    router_device: device,
                    router_port,
                },
                _ => HelperPreset::Kind(HelperKind::RadioBridge),
            },
        ),
        ActionHelper::Other => return None,
    };
    Some(Action {
        kind,
        helper,
        preset,
        interface,
    })
}
