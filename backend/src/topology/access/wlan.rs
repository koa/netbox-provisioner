use crate::topology::{
    Topology, WlanAuth, WlanData, WlanId,
    access::{AccessTopology, vlan::VlanAccess, wlan_group::WlanGroupAccess},
};
use async_graphql::Object;
use std::{cmp::Ordering, fmt::Debug, sync::Arc};

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
    pub fn ssid(&self) -> Option<&str> {
        self.data().map(|d| d.ssid.as_ref())
    }
    pub fn auth(&self) -> Option<&WlanAuth> {
        self.data().map(|d| &d.wlan_auth)
    }
}
impl Debug for WlanAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WlanAccess({};{}))",
            self.id.0,
            self.ssid().unwrap_or_default()
        )
    }
}

impl PartialOrd for WlanAccess {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        AccessTopology::partial_cmp(self, other)
    }
}

impl Ord for WlanAccess {
    fn cmp(&self, other: &Self) -> Ordering {
        AccessTopology::cmp(self, other)
    }
}

#[Object]
impl WlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id().0
    }
}
