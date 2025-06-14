use crate::topology::{
    RearPort, RearPortId, Topology,
    access::{
        AccessTopology, cable::CableAccess, device::DeviceAccess, front_port::FrontPortAccess,
    },
};
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct RearPortAccess {
    topology: Arc<Topology>,
    id: RearPortId,
}

impl AccessTopology for RearPortAccess {
    type Id = RearPortId;
    type Data = RearPort;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.rear_ports.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl RearPortAccess {
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn device(&self) -> Option<DeviceAccess> {
        self.data().map(|d| d.device).map(self.create_access())
    }
    pub fn front_port(&self) -> Option<FrontPortAccess> {
        self.data()
            .and_then(|d| d.front_port)
            .map(self.create_access())
    }
    pub fn cable(&self) -> Option<CableAccess> {
        self.data().and_then(|d| d.cable).map(self.create_access())
    }
}
