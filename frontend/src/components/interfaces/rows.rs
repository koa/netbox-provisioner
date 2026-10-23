//! The rows of `DeviceAccess.interfaceView` as the card shows them. The findings come over as
//! the type of the query `Findings`, so `components::findings` words them as everywhere else.
use crate::{
    error::FrontendError,
    graphql::authenticated::{
        findings::FindingsTopologyFindings,
        interface_view::{self, InterfaceTask},
    },
};
use serde::Serialize;

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
            InterfaceTask::LOOPBACK => Task::Loopback,
            InterfaceTask::UPLINK => Task::Uplink,
            InterfaceTask::DIRECT => Task::Direct,
            InterfaceTask::RADIO_LINK => Task::RadioLink,
            InterfaceTask::CUSTOMER => Task::Customer,
            InterfaceTask::UNKNOWN | InterfaceTask::Other(_) => Task::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: i64,
    pub name: String,
    pub site: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Iface {
    pub id: i64,
    pub name: String,
    pub device: Device,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vlan {
    pub id: i64,
    pub vid: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Address {
    pub id: i64,
    /// `172.17.1.1/30`
    pub address: String,
}

impl Address {
    pub fn is_v6(&self) -> bool {
        self.address.contains(':')
    }
}

/// `router_port ── bridge_port ┄ ssid ┄ peer_bridge_port ── peer_router_port`
#[derive(Debug, Clone, PartialEq)]
pub struct RadioPath {
    pub router_port: Option<Iface>,
    pub bridge_port: Option<Iface>,
    pub ssid: Option<String>,
    pub peer_bridge_port: Option<Iface>,
    pub peer_router_port: Option<Iface>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub task: Task,
    pub interface: Iface,
    pub label: Option<String>,
    pub ports: Vec<Iface>,
    pub addresses: Vec<Address>,
    pub ospf: bool,
    pub dhcp: bool,
    pub dhcp_client: bool,
    pub nat: bool,
    pub mtu: Option<i64>,
    pub untagged_vlan: Option<Vlan>,
    pub tagged_vlans: Vec<Vlan>,
    pub far_ends: Vec<Iface>,
    pub radio_path: Option<RadioPath>,
    pub gateways: Vec<Device>,
    pub free: bool,
    pub findings: Vec<FindingsTopologyFindings>,
}

/// The view of a device: its rows, the findings about the device (shown with the loopback) and
/// the ones on none of its interfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub netbox: String,
    pub rows: Vec<Row>,
    pub device_findings: Vec<FindingsTopologyFindings>,
    pub other_findings: Vec<FindingsTopologyFindings>,
}

impl View {
    pub fn of(data: interface_view::ResponseData) -> Result<View, FrontendError> {
        let view = data
            .topology
            .device_by_id
            .ok_or(FrontendError::MissingData)?
            .interface_view;
        Ok(View {
            netbox: data.netbox_url,
            rows: view.rows.into_iter().map(Row::of).collect::<Result<_, _>>()?,
            device_findings: findings(&view.device_findings)?,
            other_findings: findings(&view.other_findings)?,
        })
    }
}

impl Row {
    fn of(row: interface_view::InterfaceViewTopologyDeviceByIdInterfaceViewRows) -> Result<Row, FrontendError> {
        Ok(Row {
            task: Task::of(&row.task),
            interface: iface(row.interface),
            label: row.label,
            ports: row.ports.into_iter().map(iface).collect(),
            addresses: row
                .addresses
                .into_iter()
                .map(|a| Address {
                    id: a.id,
                    address: a.address,
                })
                .collect(),
            ospf: row.ospf,
            dhcp: row.dhcp,
            dhcp_client: row.dhcp_client,
            nat: row.nat,
            mtu: row.mtu,
            untagged_vlan: row.untagged_vlan.map(vlan),
            tagged_vlans: row.tagged_vlans.into_iter().map(vlan).collect(),
            far_ends: row.far_ends.into_iter().map(iface).collect(),
            radio_path: row.radio_path.map(|path| RadioPath {
                router_port: path.router_port.map(iface),
                bridge_port: path.bridge_port.map(iface),
                ssid: path.ssid,
                peer_bridge_port: path.peer_bridge_port.map(iface),
                peer_router_port: path.peer_router_port.map(iface),
            }),
            gateways: row.gateways.into_iter().map(device).collect(),
            free: row.free,
            findings: findings(&row.findings)?,
        })
    }
}

/// The findings as the query `Findings` has them: the same selection, another type.
fn findings<F: Serialize>(findings: &[F]) -> Result<Vec<FindingsTopologyFindings>, FrontendError> {
    findings
        .iter()
        .map(|finding| Ok(serde_json::from_value(serde_json::to_value(finding)?)?))
        .collect()
}

fn device(device: interface_view::Device) -> Device {
    Device {
        id: device.id,
        name: device.name,
        site: device.site,
    }
}

fn iface(interface: interface_view::Interface) -> Iface {
    Iface {
        id: interface.id,
        name: interface.name,
        device: device(interface.device),
    }
}

fn vlan(vlan: interface_view::Vlan) -> Vlan {
    Vlan {
        id: vlan.id,
        vid: vlan.vid,
        name: vlan.name,
    }
}
