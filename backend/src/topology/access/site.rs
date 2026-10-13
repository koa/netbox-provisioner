use crate::topology::{
    SiteData, SiteId, Topology,
    access::{AccessTopology, device::DeviceAccess},
};
use async_graphql::Object;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct SiteAccess {
    topology: Arc<Topology>,
    id: SiteId,
}

impl AccessTopology for SiteAccess {
    type Id = SiteId;
    type Data = SiteData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.sites.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        SiteAccess { topology, id }
    }
}

impl SiteAccess {
    pub fn name(&self) -> &str {
        self.data().map(|d| &*d.name).unwrap_or_default()
    }
    pub fn devices(&self) -> impl Iterator<Item = DeviceAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.devices.iter().copied())
            .map(self.create_access())
    }
}

#[Object]
impl SiteAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name()
    }
    /// Its devices with RouterOS, the ones the provisioner manages.
    #[graphql(name = "devices")]
    async fn api_devices(&self) -> Box<[DeviceAccess]> {
        let mut devices = self
            .devices()
            .filter(DeviceAccess::has_routeros)
            .collect::<Box<[_]>>();
        devices.sort_by(|a, b| a.name().cmp(b.name()));
        devices
    }
}
