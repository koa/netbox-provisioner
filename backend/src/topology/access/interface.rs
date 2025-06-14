use crate::topology::{
    Interface, InterfaceId, PhysicalPortId, Topology,
    access::{
        AccessTopology,
        cable::{CableAccess, CablePortAccess},
        device::DeviceAccess,
        graphql::IpNetGraphql,
        vlan::VlanAccess,
    },
};
use async_graphql::Object;
use ipnet::IpNet;
use mikrotik_model::ascii::AsciiString;
use std::{
    fmt::{Debug, Formatter},
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct InterfaceAccess {
    topology: Arc<Topology>,
    id: InterfaceId,
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

#[Object]
impl InterfaceAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name()
    }
    #[graphql(name = "ips")]
    async fn api_ips(&self) -> Box<[IpNetGraphql]> {
        self.ips().iter().copied().map(IpNet::into).collect()
    }
}
