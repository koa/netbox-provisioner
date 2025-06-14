use crate::topology::{IpRangeData, IpRangeId, Topology, access::AccessTopology};
use std::sync::Arc;

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
