use crate::topology::{
    Topology, WlanGroupData, WlanGroupId,
    access::{AccessTopology, device::DeviceAccess, vlan::VlanAccess, wlan::WlanAccess},
};
use async_graphql::Object;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct WlanGroupAccess {
    topology: Arc<Topology>,
    id: WlanGroupId,
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

#[Object]
impl WlanGroupAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id().0
    }
    #[graphql(name = "wlanList")]
    async fn api_wlan_list(&self) -> Box<[WlanAccess]> {
        self.wlan().collect()
    }
    #[graphql(name = "controller")]
    async fn api_controller(&self) -> Option<DeviceAccess> {
        self.controller()
    }
    #[graphql(name = "aps")]
    async fn api_aps(&self) -> Box<[DeviceAccess]> {
        self.aps()
    }
}
