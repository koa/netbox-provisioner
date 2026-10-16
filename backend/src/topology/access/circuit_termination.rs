use crate::topology::{
    CircuitTermination, CircuitTerminationId, Topology,
    access::{AccessTopology, cable::CableAccess},
};
use std::sync::Arc;

/// An end of a circuit; the cable path goes on at `peer`, the other end.
#[derive(Clone, PartialEq, Eq)]
pub struct CircuitTerminationAccess {
    topology: Arc<Topology>,
    id: CircuitTerminationId,
}

impl AccessTopology for CircuitTerminationAccess {
    type Id = CircuitTerminationId;
    type Data = CircuitTermination;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.circuit_terminations.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl CircuitTerminationAccess {
    /// the circuit's `cid`
    pub fn circuit(&self) -> Option<&str> {
        self.data().map(|d| d.circuit.as_ref())
    }
    pub fn peer(&self) -> Option<CircuitTerminationAccess> {
        self.data().and_then(|d| d.peer).map(self.create_access())
    }
    pub fn cable(&self) -> Option<CableAccess> {
        self.data().and_then(|d| d.cable).map(self.create_access())
    }
}
