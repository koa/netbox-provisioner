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
    /// NetBox' web UI, ending in `/`, for links to its objects: as `netboxUrl` of the query,
    /// here for a subscription, which has `topology` as its one root field.
    async fn netbox_url(&self) -> String {
        crate::netbox::web_url()
    }
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
    /// The devices and edges of the site `site_id`, or without it of every site, as OSPF sees
    /// them, for the network map; read anew after `network_state::read::LSDB_AGE`. Fast: only
    /// the LSDB, no device polled. Health, resources and PoE are `networkStateDetails`, asked
    /// separately so the overview doesn't wait on them.
    async fn network_state(&self, site_id: Option<u32>) -> crate::network_state::graphql::NetworkState {
        crate::network_state::graphql::network_state(self.map_devices(site_id).await).await
    }
    /// Health, resources and PoE edges of `network_state`'s devices with OSPF of their own,
    /// read from each device anew after `network_state::device_data::DEVICE_DATA_AGE`: slower
    /// than `networkState` (one round trip per device), asked separately so it doesn't hold up
    /// the overview.
    async fn network_state_details(&self, site_id: Option<u32>) -> crate::network_state::graphql::NetworkStateDetails {
        crate::network_state::graphql::network_state_details(self.map_devices(site_id).await).await
    }
}

impl TopologyHolder {
    /// `site_id`'s devices, or without it every device of every site, for `network_state` and
    /// `network_state_details`.
    async fn map_devices(&self, site_id: Option<u32>) -> Vec<DeviceAccess> {
        self.topo_lock()
            .await
            .as_ref()
            .map(|topo| match site_id {
                Some(id) => SiteAccess::create(topo.clone(), SiteId(id)).devices().collect(),
                None => topo.list_devices().collect(),
            })
            .unwrap_or_default()
    }
}
