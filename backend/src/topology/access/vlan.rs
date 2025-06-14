use crate::topology::{
    Topology, VlanData, VlanId,
    access::{AccessTopology, vxlan::VxlanAccess, wlan::WlanAccess},
};
use async_graphql::Object;
use std::{
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct VlanAccess {
    topology: Arc<Topology>,
    pub id: VlanId,
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

#[Object]
impl VlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name().unwrap_or_default()
    }
    async fn api_vlan_id(&self) -> u16 {
        self.vlan_id().expect("vlan_id not set")
    }
}
