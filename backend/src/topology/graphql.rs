use crate::topology::{
    DeviceId, SiteId, TopologyHolder,
    access::{AccessTopology, device::DeviceAccess, site::SiteAccess},
};
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
    /// Sorted by name, the ones with a device with RouterOS.
    async fn all_sites(&self) -> Box<[SiteAccess]> {
        let Some(topo) = self.topo_lock().await.as_ref().cloned() else {
            return Box::default();
        };
        let mut sites = topo
            .sites
            .keys()
            .map(|id| SiteAccess::create(topo.clone(), *id))
            .filter(|site| site.devices().any(|d| d.has_routeros()))
            .collect::<Box<[_]>>();
        sites.sort_by(|a, b| a.name().cmp(b.name()));
        sites
    }
    async fn site_by_id(&self, id: u32) -> Option<SiteAccess> {
        let topo = self.topo_lock().await.as_ref().cloned()?;
        topo.sites
            .contains_key(&SiteId(id))
            .then(|| SiteAccess::create(topo.clone(), SiteId(id)))
    }
    async fn device_by_id(&self, id: u32) -> Option<DeviceAccess> {
        if let Some(topo) = self.topo_lock().await.as_ref() {
            topo.get_device_by_id(&DeviceId(id))
        } else {
            None
        }
    }
}
