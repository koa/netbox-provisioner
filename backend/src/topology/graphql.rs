use crate::device::AccessibleDevice;
use crate::topology::{DeviceAccess, DeviceId, TopologyHolder};
use async_graphql::Object;

#[Object]
impl TopologyHolder {
    async fn all_devices(&self) -> Box<[DeviceAccess]> {
        self.devices().await
    }
    async fn device_by_id(&self, id: u32) -> Option<DeviceAccess> {
        if let Some(topo) = self.topo_lock().await.as_ref() {
            topo.get_device_by_id(&DeviceId(id))
        } else {
            None
        }
    }
}

#[Object]
impl DeviceAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name()
    }
    async fn management_address(&self) -> Option<String> {
        self.data()
            .and_then(|a| a.primary_ip)
            .map(|s| s.to_string())
    }
    async fn access(&self) -> Option<AccessibleDevice> {
        self.clone().into()
    }
}
