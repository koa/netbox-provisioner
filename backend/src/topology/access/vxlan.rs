use crate::topology::{
    Topology, VxlanData, VxlanId,
    access::{AccessTopology, vlan::VlanAccess},
};
use async_graphql::Object;
use std::cmp::Ordering;
use std::{
    collections::BTreeSet,
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    net::IpAddr,
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct VxlanAccess {
    topology: Arc<Topology>,
    id: VxlanId,
}

impl AccessTopology for VxlanAccess {
    type Id = VxlanId;
    type Data = VxlanData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.vxlans.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        VxlanAccess { topology, id }
    }
}

impl VxlanAccess {
    pub fn data(&self) -> Option<&VxlanData> {
        self.topology.vxlans.get(&self.id)
    }
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn vni(&self) -> Option<u32> {
        self.data().map(|d| d.vni)
    }
    /// All VLANs of the VLAN group, tunneled or not.
    pub fn vlans(&self) -> Box<[VlanAccess]> {
        self.data()
            .map(|d| d.vlans.iter().copied().map(self.create_access()).collect())
            .unwrap_or_default()
    }

    /// Addresses of every device using one of the VLANs actually tunneled over this
    /// VXLAN (see `VlanAccess::tunnel`) - the head-end replication list.
    pub fn vteps(&self) -> Box<[IpAddr]> {
        Box::from_iter(
            self.vlans()
                .iter()
                .filter(|vlan| vlan.tunnel().is_some())
                .flat_map(|vlan| vlan.devices())
                .filter_map(|dev| dev.primary_ip_v4().and_then(|ip| ip.addr()))
                .collect::<BTreeSet<IpAddr>>(),
        )
    }
}

impl Debug for VxlanAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VxlanAccess({};{})",
            self.id.0,
            self.name().unwrap_or_default()
        )
    }
}

impl Hash for VxlanAccess {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl PartialOrd for VxlanAccess {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(Ord::cmp(self, other))
    }
}
impl Ord for VxlanAccess {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

#[Object]
impl VxlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id().0
    }
    #[graphql(name = "vni")]
    async fn api_vni(&self) -> Option<u32> {
        self.vni()
    }
    #[graphql(name = "vlans")]
    async fn api_vlans(&self) -> Box<[VlanAccess]> {
        self.vlans()
    }
}
