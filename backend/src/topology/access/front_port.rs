use crate::topology::{
    FrontPort, FrontPortId, Topology,
    access::{AccessTopology, cable::CableAccess, device::DeviceAccess, rear_port::RearPortAccess},
};
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct FrontPortAccess {
    topology: Arc<Topology>,
    id: FrontPortId,
}

impl AccessTopology for FrontPortAccess {
    type Id = FrontPortId;
    type Data = FrontPort;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.front_ports.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl FrontPortAccess {
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn device(&self) -> Option<DeviceAccess> {
        self.data().map(|d| d.device).map(self.create_access())
    }
    pub fn cable(&self) -> Option<CableAccess> {
        self.data().and_then(|d| d.cable).map(self.create_access())
    }
    pub fn rear_port(&self) -> Option<RearPortAccess> {
        self.data()
            .and_then(|d| d.rear_port)
            .map(self.create_access())
    }
}
