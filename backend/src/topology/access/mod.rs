use crate::topology::{
    Cable, CableId, CablePort, Device, DeviceId, FrontPort, FrontPortId, Interface, InterfaceId,
    PhysicalPortId, RearPort, RearPortId, Topology, VlanData, VlanId, VxlanData, VxlanId, WlanData,
    WlanGroupData, WlanGroupId, WlanId,
};
use ipnet::IpNet;
use mikrotik_model::ascii::AsciiString;
use std::{
    collections::{BTreeSet, HashSet},
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

pub mod graphql;

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceAccess {
    topology: Arc<Topology>,
    id: DeviceId,
}

#[derive(Clone, PartialEq, Eq)]
pub struct InterfaceAccess {
    topology: Arc<Topology>,
    id: InterfaceId,
}

#[derive(Clone, PartialEq, Eq)]
pub struct WlanGroupAccess {
    topology: Arc<Topology>,
    id: WlanGroupId,
}

#[derive(Clone, PartialEq, Eq)]
pub struct VxlanAccess {
    topology: Arc<Topology>,
    id: VxlanId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct VlanAccess {
    topology: Arc<Topology>,
    pub id: VlanId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct WlanAccess {
    topology: Arc<Topology>,
    id: WlanId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct FrontPortAccess {
    topology: Arc<Topology>,
    id: FrontPortId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct RearPortAccess {
    topology: Arc<Topology>,
    id: RearPortId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct CableAccess {
    topology: Arc<Topology>,
    id: CableId,
}
#[derive(Clone, PartialEq, Eq)]
pub enum CablePortAccess {
    Interface(InterfaceAccess),
    FrontPort(FrontPortAccess),
    RearPort(RearPortAccess),
}

#[derive(Clone, PartialEq, Eq)]
pub struct CableConnection {
    near: CablePortAccess,
    far: CablePortAccess,
    cable: CableAccess,
}

#[derive(Clone, PartialEq, Eq)]
pub enum DeviceConnection {
    FrontNear {
        near: FrontPortAccess,
        far: RearPortAccess,
        device: DeviceAccess,
    },
    RearNear {
        near: RearPortAccess,
        far: FrontPortAccess,
        device: DeviceAccess,
    },
}

trait AccessTopology {
    type Id: Copy;
    type Data;
    fn topology(&self) -> Arc<Topology>;
    fn id(&self) -> Self::Id;
    fn data(&self) -> Option<&Self::Data>;
    fn create(topology: Arc<Topology>, id: Self::Id) -> Self;

    fn create_access<Id, Access>(&self) -> impl Fn(Id) -> Access
    where
        Access: AccessTopology<Id = Id>,
    {
        |id| Access::create(self.topology(), id)
    }
}

impl AccessTopology for DeviceAccess {
    type Id = DeviceId;
    type Data = Device;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.devices.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        DeviceAccess { topology, id }
    }
}

impl DeviceAccess {
    pub fn id(&self) -> DeviceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name()).unwrap_or_default()
    }
    pub fn serial(&self) -> Option<&str> {
        self.data().and_then(|d| d.serial.as_deref())
    }
    pub fn primary_ip(&self) -> Option<IpAddr> {
        self.data().and_then(Device::primary_ip)
    }
    pub fn primary_ip_v4(&self) -> Option<Ipv4Addr> {
        self.data().and_then(|d| d.primary_ip_v4)
    }
    pub fn primary_ip_v6(&self) -> Option<Ipv6Addr> {
        self.data().and_then(|d| d.primary_ip_v6)
    }

    pub fn loopback_ip(&self) -> Option<IpAddr> {
        self.data().and_then(|d| d.loopback_ip)
    }

    pub fn has_routeros(&self) -> bool {
        self.data().map(|d| d.has_routeros).unwrap_or(false)
    }
    pub fn credentials(&self) -> Option<&str> {
        self.data().and_then(|d| Device::credentials(d))
    }
    pub fn interfaces<'a>(&'a self) -> Box<[InterfaceAccess]> {
        self.topology
            .devices
            .get(&self.id)
            .map(|data| {
                data.ports
                    .iter()
                    .filter_map(|p| {
                        if let CablePort::Interface(p) = p {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .copied()
                    .map(self.create_access())
                    .collect::<Box<[_]>>()
            })
            .unwrap_or_default()
    }
    pub fn wlan_controller_of(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_controller_of)
            .map(self.create_access())
    }
    pub fn wlan_ap_of(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_ap_of)
            .map(self.create_access())
    }

    pub fn vlans(&self) -> impl Iterator<Item = VlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.vlans.iter().cloned())
            .map(self.create_access())
    }

    pub fn vxlan(&self) -> HashSet<VxlanAccess> {
        self.vlans().filter_map(|vl| vl.vxlan()).collect()
    }
}

impl AccessTopology for InterfaceAccess {
    type Id = InterfaceId;
    type Data = Interface;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.interfaces.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        InterfaceAccess { topology, id }
    }
}

impl InterfaceAccess {
    pub fn id(&self) -> InterfaceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name.as_ref()).unwrap_or_default()
    }
    pub fn cable(&self) -> Option<CableAccess> {
        self.data().and_then(|c| c.cable).map(self.create_access())
    }
    pub fn label(&self) -> Option<&str> {
        self.data()
            .map(|d| d.label.as_ref())
            .filter(|l| !l.is_empty())
    }

    pub fn use_ospf(&self) -> bool {
        self.data().map(|d| d.use_ospf).unwrap_or(false)
    }
    pub fn enable_dhcp_client(&self) -> bool {
        self.data().map(|d| d.enable_dhcp_client).unwrap_or(false)
    }
    pub fn external_port(&self) -> Option<PhysicalPortId> {
        self.data().and_then(|d| d.external)
    }
    pub fn is_ethernet_port(&self) -> bool {
        self.external_port()
            .map(|p| p.is_ethernet())
            .unwrap_or(false)
    }

    pub fn device(&self) -> Option<DeviceAccess> {
        self.data().map(|d| d.device).map(self.create_access())
    }
    pub fn connected_interfaces(&self) -> Box<[InterfaceAccess]> {
        let mut result = Vec::new();
        self.cable_port().walk_cable(
            &mut (|p| {
                if let CablePortAccess::Interface(a) = p.far_port() {
                    result.push(a.clone())
                }
            }),
        );
        result.into_boxed_slice()
    }

    pub fn cable_port(&self) -> CablePortAccess {
        CablePortAccess::Interface(self.clone())
    }

    pub fn ips(&self) -> &[IpNet] {
        self.data().map(|d| d.ips.as_ref()).unwrap_or_default()
    }
    pub fn interface_name(&self) -> Option<AsciiString> {
        self.external_port().map(|port| {
            if let Some(label) = self.label() {
                let mut name = port.short_name().0.to_vec();
                name.push(b'-');
                for char in label.chars() {
                    if char.is_ascii_alphanumeric() {
                        name.push(char as u8);
                    } else {
                        match char {
                            '-' | '.' => name.push(b'-'),
                            'ä' => name.extend_from_slice(b"ae"),
                            'ö' => name.extend_from_slice(b"oe"),
                            'ü' => name.extend_from_slice(b"ue"),
                            _ => {}
                        }
                    }
                }
                name.into_boxed_slice().into()
            } else {
                port.short_name()
            }
        })
    }
    pub fn untagged_vlan(&self) -> Option<VlanAccess> {
        self.data().and_then(|d| d.vlan).map(self.create_access())
    }
    pub fn tagged_vlans(&self) -> impl Iterator<Item = VlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|data| data.tagged_vlans.iter().copied().map(self.create_access()))
    }
    pub fn bridge(&self) -> Option<InterfaceAccess> {
        self.data().and_then(|d| d.bridge).map(self.create_access())
    }
    pub fn enable_poe(&self) -> bool {
        self.data().map(|d| d.enable_poe).unwrap_or(false)
    }
}
impl Debug for InterfaceAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let device = self.device();
        let device_name = device.as_ref().map(|d| d.name()).unwrap_or_default();
        let if_name = self.name();
        write!(f, "InterfaceAccess({:?};{device_name}:{if_name})", self.id)
    }
}

impl AccessTopology for WlanGroupAccess {
    type Id = WlanGroupId;
    type Data = WlanGroupData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.wlan_groups.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        WlanGroupAccess { topology, id }
    }
}

impl WlanGroupAccess {
    pub fn controller(&self) -> Option<DeviceAccess> {
        self.data().map(|d| d.controller).map(self.create_access())
    }
    pub fn aps(&self) -> Box<[DeviceAccess]> {
        self.data()
            .iter()
            .flat_map(|data| data.aps.iter().copied())
            .map(self.create_access())
            .collect()
    }
    pub fn mgmt_vlan(&self) -> Option<VlanAccess> {
        self.data()
            .and_then(|d| d.mgmt_vlan)
            .map(self.create_access())
    }
    pub fn wlan(&self) -> impl Iterator<Item = WlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.wlans.iter().cloned())
            .map(self.create_access())
    }
}

impl AccessTopology for VxlanAccess {
    type Id = VxlanId;
    type Data = VxlanData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.vxlans.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        VxlanAccess { topology, id }
    }
}

impl VxlanAccess {
    pub fn data(&self) -> Option<&VxlanData> {
        self.topology.vxlans.get(&self.id)
    }
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn vni(&self) -> Option<u32> {
        self.data().map(|d| d.vni)
    }
    pub fn interface_terminations(&self) -> Box<[InterfaceAccess]> {
        self.data()
            .map(|d| {
                d.interface_terminations
                    .iter()
                    .copied()
                    .map(self.create_access())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn vlan_terminations(&self) -> Box<[VlanAccess]> {
        self.data()
            .map(|d| {
                d.vlan_terminations
                    .iter()
                    .copied()
                    .map(self.create_access())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn vteps(&self) -> Box<[IpAddr]> {
        Box::from_iter(
            self.interface_terminations()
                .iter()
                .filter_map(InterfaceAccess::device)
                .chain(
                    self.vlan_terminations()
                        .iter()
                        .flat_map(|vl| vl.wlan())
                        .filter_map(|wlan| wlan.wlan_group())
                        .flat_map(|group| group.aps().into_iter().chain(group.controller())),
                )
                .filter_map(|dev| dev.primary_ip_v4())
                .map(IpAddr::V4)
                .collect::<BTreeSet<IpAddr>>(),
        )
    }
}
impl Debug for VxlanAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VxlanAccess({};{})",
            self.id.0,
            self.name().unwrap_or_default()
        )
    }
}
impl Hash for VxlanAccess {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl AccessTopology for VlanAccess {
    type Id = VlanId;
    type Data = VlanData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&VlanData> {
        self.topology.vlans.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        VlanAccess { topology, id }
    }
}

impl VlanAccess {
    pub fn name(&self) -> Option<&str> {
        self.data().map(|v| v.name.as_ref())
    }
    pub fn vlan_id(&self) -> Option<u16> {
        self.data().map(|v| v.vlan_id)
    }
    pub fn vxlan(&self) -> Option<VxlanAccess> {
        self.data().and_then(|v| v.vxlan).map(self.create_access())
    }
    pub fn wlan(&self) -> impl Iterator<Item = WlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.wlans.iter().cloned())
            .map(self.create_access())
    }
}
impl Debug for VlanAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VlanAccess({};{}({}))",
            self.id.0,
            self.name().unwrap_or_default(),
            self.vlan_id().unwrap_or_default()
        )
    }
}
impl Hash for VlanAccess {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl AccessTopology for WlanAccess {
    type Id = WlanId;
    type Data = WlanData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.wlans.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        WlanAccess { topology, id }
    }
}
impl WlanAccess {
    pub fn vlan(&self) -> Option<VlanAccess> {
        self.data().and_then(|d| d.vlan).map(self.create_access())
    }
    pub fn wlan_group(&self) -> Option<WlanGroupAccess> {
        self.data().map(|d| d.wlan_group).map(self.create_access())
    }
}

impl AccessTopology for FrontPortAccess {
    type Id = FrontPortId;
    type Data = FrontPort;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.front_ports.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl FrontPortAccess {
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn device(&self) -> Option<DeviceAccess> {
        self.data().map(|d| d.device).map(self.create_access())
    }
    pub fn cable(&self) -> Option<CableAccess> {
        self.data().and_then(|d| d.cable).map(self.create_access())
    }
    pub fn rear_port(&self) -> Option<RearPortAccess> {
        self.data()
            .and_then(|d| d.rear_port)
            .map(self.create_access())
    }
}

impl AccessTopology for RearPortAccess {
    type Id = RearPortId;
    type Data = RearPort;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.rear_ports.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl RearPortAccess {
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn device(&self) -> Option<DeviceAccess> {
        self.data().map(|d| d.device).map(self.create_access())
    }
    pub fn front_port(&self) -> Option<FrontPortAccess> {
        self.data()
            .and_then(|d| d.front_port)
            .map(self.create_access())
    }
    pub fn cable(&self) -> Option<CableAccess> {
        self.data().and_then(|d| d.cable).map(self.create_access())
    }
}

impl AccessTopology for CableAccess {
    type Id = CableId;
    type Data = Cable;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.cables.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl CableAccess {
    pub fn port_a(&self) -> impl Iterator<Item = CablePortAccess> {
        self.data()
            .map(|d| &d.port_a)
            .into_iter()
            .flat_map(move |ports| self.create_port_accesses(ports))
    }

    fn create_port_accesses(&self, ports: &[CablePort]) -> impl Iterator<Item = CablePortAccess> {
        ports
            .iter()
            .copied()
            .map(|port| self.create_port_access(port))
    }

    fn create_port_access(&self, port: CablePort) -> CablePortAccess {
        match port {
            CablePort::Interface(id) => {
                CablePortAccess::Interface(InterfaceAccess::create(self.topology(), id))
            }
            CablePort::FrontPort(id) => {
                CablePortAccess::FrontPort(FrontPortAccess::create(self.topology(), id))
            }
            CablePort::RearPort(id) => {
                CablePortAccess::RearPort(RearPortAccess::create(self.topology(), id))
            }
        }
    }

    pub fn port_b(&self) -> impl Iterator<Item = CablePortAccess> {
        self.data()
            .map(|d| &d.port_b)
            .into_iter()
            .flat_map(move |ports| self.create_port_accesses(ports))
    }
    pub fn connections_from_port(&self, port: CablePort) -> impl Iterator<Item = CableConnection> {
        self.data()
            .map(move |cable_data| {
                if cable_data.port_a.contains(&port) {
                    Some(self.port_b())
                } else {
                    None
                }
                .into_iter()
                .flatten()
                .chain(
                    if cable_data.port_b.contains(&port) {
                        Some(self.port_a())
                    } else {
                        None
                    }
                    .into_iter()
                    .flatten(),
                )
                .map(move |far| CableConnection {
                    near: self.create_port_access(port),
                    far,
                    cable: self.clone(),
                })
            })
            .into_iter()
            .flatten()
    }
}
impl CablePortAccess {
    pub fn name(&self) -> Option<&str> {
        match self {
            CablePortAccess::Interface(a) => Some(a.name()),
            CablePortAccess::FrontPort(a) => a.name(),
            CablePortAccess::RearPort(a) => a.name(),
        }
    }

    pub fn device(&self) -> Option<DeviceAccess> {
        match self {
            CablePortAccess::Interface(a) => a.device(),
            CablePortAccess::FrontPort(a) => a.device(),
            CablePortAccess::RearPort(a) => a.device(),
        }
    }
    pub fn port(&self) -> CablePort {
        match self {
            CablePortAccess::Interface(a) => CablePort::Interface(a.id),
            CablePortAccess::FrontPort(a) => CablePort::FrontPort(a.id),
            CablePortAccess::RearPort(a) => CablePort::RearPort(a.id),
        }
    }
    pub fn cable(&self) -> Option<CableAccess> {
        match self {
            CablePortAccess::Interface(a) => a.cable(),
            CablePortAccess::FrontPort(a) => a.cable(),
            CablePortAccess::RearPort(a) => a.cable(),
        }
    }
    pub fn next_device_port_id(&self) -> Option<CablePortAccess> {
        match self {
            CablePortAccess::Interface(_) => None,
            CablePortAccess::FrontPort(a) => a.rear_port().map(CablePortAccess::RearPort),
            CablePortAccess::RearPort(a) => a.front_port().map(CablePortAccess::FrontPort),
        }
    }
    pub fn attached_cable_segments(&self) -> Box<[CableConnection]> {
        self.cable()
            .map(move |cable| {
                cable
                    .connections_from_port(self.port())
                    .collect::<Box<[_]>>()
            })
            .unwrap_or_default()
    }
    fn append_cable_segments(
        &self,
        parent_chain: Vec<CableConnection>,
        result: &mut impl FnMut(Box<[CableConnection]>, Option<CablePortAccess>),
    ) {
        let remaining_segments = self.attached_cable_segments();
        if let Some((last_connection, remaining_connections)) = remaining_segments.split_last() {
            for next_segment in remaining_connections {
                self.append_cable_segment(parent_chain.clone(), next_segment, result);
            }
            self.append_cable_segment(parent_chain, last_connection, result);
        } else {
            result(parent_chain.into_boxed_slice(), Some(self.clone()));
        }
    }
    fn append_cable_segment(
        &self,
        mut parent_chain: Vec<CableConnection>,
        connection: &CableConnection,
        result: &mut impl FnMut(Box<[CableConnection]>, Option<CablePortAccess>),
    ) {
        if let Some(next_port) = connection.far.next_device_port_id() {
            parent_chain.push(connection.clone());
            next_port.append_cable_segments(parent_chain, result);
        } else {
            parent_chain.push(connection.clone());
            result(parent_chain.into_boxed_slice(), None);
        }
    }
    pub fn walk_cable(&self, result: &mut impl FnMut(CablePath)) {
        self.append_cable_segments(
            Vec::new(),
            &mut (|cable_segments, end_port| {
                if end_port.as_ref() != Some(self) {
                    result(CablePath {
                        start_port: self.clone(),
                        cable_segments,
                        end_port,
                    });
                }
            }),
        );
    }
    pub fn collect_cables(&self) -> Box<[CablePath]> {
        let mut result = Vec::new();
        self.walk_cable(&mut |cable_path| result.push(cable_path));
        result.into_boxed_slice()
    }
}
pub struct CablePath {
    pub start_port: CablePortAccess,
    pub cable_segments: Box<[CableConnection]>,
    pub end_port: Option<CablePortAccess>,
}
impl CablePath {
    pub fn far_port(&self) -> &CablePortAccess {
        if let Some(end_port) = self.end_port.as_ref() {
            end_port
        } else if let Some(last_seg) = self.cable_segments.last() {
            &last_seg.far
        } else {
            &self.start_port
        }
    }
}

impl Topology {
    pub fn list_devices(self: &Arc<Self>) -> impl Iterator<Item = DeviceAccess> {
        let topo = self;
        self.devices
            .keys()
            .copied()
            .map(move |id| DeviceAccess::create(topo.clone(), id))
    }
    pub fn get_device_by_id(self: &Arc<Self>, id: &DeviceId) -> Option<DeviceAccess> {
        if self.devices.contains_key(id) {
            Some(DeviceAccess::create(self.clone(), *id))
        } else {
            None
        }
    }
}
