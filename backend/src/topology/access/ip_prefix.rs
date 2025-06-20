use crate::topology::{
    IpPrefixData, IpPrefixId, Topology,
    access::{AccessTopology, ip_addresses::IpAddressAccess, ip_ranges::IpRangeAccess},
};
use ipnet::IpNet;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct IpPrefixAccess {
    topology: Arc<Topology>,
    id: IpPrefixId,
}

impl AccessTopology for IpPrefixAccess {
    type Id = IpPrefixId;
    type Data = IpPrefixData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.ip_prefixes.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}
impl IpPrefixAccess {
    pub fn parent(&self) -> Option<IpPrefixAccess> {
        self.data().and_then(|d| d.parent).map(self.create_access())
    }
    pub fn children(&self) -> Box<[IpPrefixAccess]> {
        self.data()
            .map(|d| {
                d.children
                    .iter()
                    .copied()
                    .map(self.create_access())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn ips(&self) -> Box<[IpAddressAccess]> {
        self.data()
            .map(|d| {
                d.addresses
                    .iter()
                    .copied()
                    .map(self.create_access())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn ranges(&self) -> Box<[IpRangeAccess]> {
        self.data()
            .map(|d| d.ranges.iter().copied().map(self.create_access()).collect())
            .unwrap_or_default()
    }
    pub fn prefix(&self) -> Option<IpNet> {
        self.data().map(|d| d.prefix)
    }
}
