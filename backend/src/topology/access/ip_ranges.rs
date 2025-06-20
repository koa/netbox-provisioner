use crate::topology::{IpRangeData, IpRangeId, Topology, access::AccessTopology};
use ipnet::IpNet;
use std::{net::IpAddr, sync::Arc};

#[derive(Clone, PartialEq, Eq)]
pub struct IpRangeAccess {
    topology: Arc<Topology>,
    id: IpRangeId,
}

impl AccessTopology for IpRangeAccess {
    type Id = IpRangeId;
    type Data = IpRangeData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.ip_ranges.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        IpRangeAccess { topology, id }
    }
}
impl IpRangeAccess {
    pub fn is_dhcp(&self) -> bool {
        self.data().map(|d| d.is_dhcp).unwrap_or(false)
    }
    pub fn start(&self) -> Option<IpAddr> {
        self.data().map(|d| d.start)
    }
    pub fn end(&self) -> Option<IpAddr> {
        self.data().map(|d| d.end)
    }
    pub fn net(&self) -> Option<IpNet> {
        self.data().map(|d| d.net)
    }
}
