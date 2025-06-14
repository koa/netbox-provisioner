use crate::topology::{
    Topology, WlanData, WlanId,
    access::{AccessTopology, vlan::VlanAccess, wlan_group::WlanGroupAccess},
};
use async_graphql::Object;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct WlanAccess {
    topology: Arc<Topology>,
    id: WlanId,
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

#[Object]
impl WlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id().0
    }
}
