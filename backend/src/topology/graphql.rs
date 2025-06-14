use crate::topology::{DeviceId, TopologyHolder, access::device::DeviceAccess};
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
