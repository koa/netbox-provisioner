use crate::topology::{
    Topology, VxlanData, VxlanId,
    access::{AccessTopology, interface::InterfaceAccess, vlan::VlanAccess},
};
use async_graphql::Object;
use std::{
    collections::BTreeSet,
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    net::IpAddr,
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct VxlanAccess {
    topology: Arc<Topology>,
    id: VxlanId,
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

#[Object]
impl VxlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id().0
    }
    #[graphql(name = "vni")]
    async fn api_vni(&self) -> Option<u32> {
        self.vni()
    }
    #[graphql(name = "interfaceTerminations")]
    async fn api_interface_terminations(&self) -> Box<[InterfaceAccess]> {
        self.interface_terminations()
    }
    #[graphql(name = "vlanTerminations")]
    async fn api_vlan_terminations(&self) -> Box<[VlanAccess]> {
        self.vlan_terminations()
    }
}
