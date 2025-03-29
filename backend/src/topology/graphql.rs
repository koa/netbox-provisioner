use crate::device::AccessibleDevice;
use crate::topology::{DeviceAccess, DeviceId, TopologyHolder};
use async_graphql::{InputObject, Object};

#[derive(InputObject)]
struct DeviceListFilter {
    has_routeros: Option<bool>,
}

#[Object]
impl TopologyHolder {
    async fn all_devices(&self, filter: Option<DeviceListFilter>) -> Box<[DeviceAccess]> {
        self.topo_lock()
            .await
            .as_ref()
            .map(|topo| {
                topo.list_devices()
                    .filter(|d| {
                        filter
                            .as_ref()
                            .map(|filter| {
                                if let Some(flag) = filter.has_routeros {
                                    if d.has_routeros() != flag {
                                        return false;
                                    }
                                }
                                true
                            })
                            .unwrap_or(true)
                    })
                    .collect()
            })
            .unwrap_or_default()
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
    #[graphql(name = "serial")]
    async fn api_serial(&self) -> Option<String> {
        self.serial().map(ToString::to_string)
    }
    async fn access(&self) -> Option<AccessibleDevice> {
        self.clone().into()
    }
}
