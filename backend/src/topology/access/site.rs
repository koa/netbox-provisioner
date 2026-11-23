//! Scope: Access to a site and its devices; nothing else.
use crate::topology::{
    SiteData, SiteId, Topology,
    access::{AccessTopology, device::DeviceAccess},
    findings::Finding,
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
    /// Custom field `auto_rollout`: its devices get NetBox' changes without an apply by hand.
    pub fn auto_rollout(&self) -> bool {
        self.data().is_some_and(|d| d.auto_rollout)
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
    /// The findings about its devices, the worst first.
    async fn findings(&self) -> Vec<Finding> {
        self.topology.site_findings(self.id)
    }
    /// With the custom field `auto_rollout`: what the automatic rollout did and where it
    /// stands; `null` for a site without.
    #[graphql(name = "autoRollout")]
    async fn api_auto_rollout(&self) -> Option<crate::auto_rollout::graphql::SiteAutoRollout> {
        self.auto_rollout()
            .then(|| crate::auto_rollout::graphql::of_site(&self.topology, self.id))
    }
}

#[cfg(test)]
mod test {
    use crate::topology::{
        SiteData, access::AccessTopology, access::device::DeviceAccess, interfaces::test::Net,
    };
    use std::sync::Arc;

    /// A device of a site with `auto_rollout` rolls out automatically, unless its own field
    /// is `false`; one of another site doesn't.
    #[test]
    fn a_site_rolls_out_its_devices_but_those_left_out() {
        let mut net = Net::new();
        let automatic = net.builder.sites.next_id();
        net.builder.sites.insert(
            automatic,
            SiteData {
                name: "auto".into(),
                devices: Box::new([net.r1, net.r2]),
                auto_rollout: true,
            },
        );
        let manual = net.builder.sites.next_id();
        net.builder.sites.insert(
            manual,
            SiteData {
                name: "manual".into(),
                devices: Box::new([net.b1]),
                auto_rollout: false,
            },
        );
        for (device, site) in [(net.r1, automatic), (net.r2, automatic), (net.b1, manual)] {
            net.builder.devices.get_mut(&device).unwrap().site = Some(site);
        }
        net.builder.devices.get_mut(&net.r2).unwrap().auto_rollout = Some(false);
        let topology = Arc::new(net.builder.build());
        let automatic = |id| DeviceAccess::create(topology.clone(), id).rolls_out_automatically();
        assert!(automatic(net.r1));
        assert!(!automatic(net.r2));
        assert!(!automatic(net.b1));
    }
}
