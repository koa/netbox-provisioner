use crate::topology::{
    DeviceId,
    findings::Finding, SiteId, TopologyHolder,
    natural::naturalize,
    access::{AccessTopology, device::DeviceAccess, site::SiteAccess},
};
use crate::{error_info::ApiError, helper::Helper};
use async_graphql::{InputObject, Object};

#[derive(InputObject)]
struct DeviceListFilter {
    has_routeros: Option<bool>,
}

#[Object]
impl TopologyHolder {
    /// In NetBox' order of names.
    async fn all_devices(&self, filter: Option<DeviceListFilter>) -> Box<[DeviceAccess]> {
        let mut devices: Box<[DeviceAccess]> = self
            .topo_lock()
            .await
            .as_ref()
            .map(|topo| {
                topo.list_devices()
                    .filter(|d| {
                        filter
                            .as_ref()
                            .map(|filter| {
                                if let Some(flag) = filter.has_routeros
                                    && d.has_routeros() != flag
                                {
                                    return false;
                                }
                                true
                            })
                            .unwrap_or(true)
                    })
                    .collect()
            })
            .unwrap_or_default();
        devices.sort_by_cached_key(|device| naturalize(device.name()));
        devices
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
    /// The findings about the devices of the site `site_id` or about the device `device_id`,
    /// without either those about the whole network (the prefix pools); the worst first. With
    /// `after`, as NetBox would be after that helper's plan.
    async fn findings(
        &self,
        site_id: Option<u32>,
        device_id: Option<u32>,
        after: Option<Helper>,
    ) -> Result<Vec<Finding>, ApiError> {
        let Some(mut topo) = self.topo_lock().await.as_ref().cloned() else {
            return Ok(Vec::new());
        };
        if let Some(helper) = after {
            topo = helper.plan(&topo).await?.preview_topology()?;
        }
        Ok(match (site_id, device_id) {
            (Some(site), _) => topo.site_findings(SiteId(site)),
            (None, Some(device)) => topo
                .findings()
                .iter()
                .filter(|finding| finding.devices.contains(&DeviceId(device)))
                .cloned()
                .collect(),
            (None, None) => topo
                .findings()
                .iter()
                .filter(|finding| finding.devices.is_empty())
                .cloned()
                .collect(),
        })
    }
    async fn device_by_id(&self, id: u32) -> Option<DeviceAccess> {
        if let Some(topo) = self.topo_lock().await.as_ref() {
            topo.get_device_by_id(&DeviceId(id))
        } else {
            None
        }
    }
}
