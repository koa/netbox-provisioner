use crate::topology::{
    Cable, CableId, CablePort, Topology,
    access::{
        AccessTopology, connections::CableConnection, connections::CablePath, device::DeviceAccess,
        front_port::FrontPortAccess, interface::InterfaceAccess, rear_port::RearPortAccess,
    },
};
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub struct CableAccess {
    topology: Arc<Topology>,
    id: CableId,
}

#[derive(Clone, PartialEq, Eq)]
pub enum CablePortAccess {
    Interface(InterfaceAccess),
    FrontPort(FrontPortAccess),
    RearPort(RearPortAccess),
}

impl AccessTopology for CableAccess {
    type Id = CableId;
    type Data = Cable;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.cables.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        Self { topology, id }
    }
}

impl CableAccess {
    pub fn port_a(&self) -> impl Iterator<Item = CablePortAccess> {
        self.data()
            .map(|d| &d.port_a)
            .into_iter()
            .flat_map(move |ports| self.create_port_accesses(ports))
    }

    fn create_port_accesses(&self, ports: &[CablePort]) -> impl Iterator<Item = CablePortAccess> {
        ports
            .iter()
            .copied()
            .map(|port| self.create_port_access(port))
    }

    fn create_port_access(&self, port: CablePort) -> CablePortAccess {
        match port {
            CablePort::Interface(id) => {
                CablePortAccess::Interface(InterfaceAccess::create(self.topology(), id))
            }
            CablePort::FrontPort(id) => {
                CablePortAccess::FrontPort(FrontPortAccess::create(self.topology(), id))
            }
            CablePort::RearPort(id) => {
                CablePortAccess::RearPort(RearPortAccess::create(self.topology(), id))
            }
        }
    }

    pub fn port_b(&self) -> impl Iterator<Item = CablePortAccess> {
        self.data()
            .map(|d| &d.port_b)
            .into_iter()
            .flat_map(move |ports| self.create_port_accesses(ports))
    }
    pub fn connections_from_port(&self, port: CablePort) -> impl Iterator<Item = CableConnection> {
        self.data()
            .map(move |cable_data| {
                if cable_data.port_a.contains(&port) {
                    Some(self.port_b())
                } else {
                    None
                }
                .into_iter()
                .flatten()
                .chain(
                    if cable_data.port_b.contains(&port) {
                        Some(self.port_a())
                    } else {
                        None
                    }
                    .into_iter()
                    .flatten(),
                )
                .map(move |far| CableConnection {
                    near: self.create_port_access(port),
                    far,
                    cable: self.clone(),
                })
            })
            .into_iter()
            .flatten()
    }
}

impl CablePortAccess {
    pub fn name(&self) -> Option<&str> {
        match self {
            CablePortAccess::Interface(a) => Some(a.name()),
            CablePortAccess::FrontPort(a) => a.name(),
            CablePortAccess::RearPort(a) => a.name(),
        }
    }

    pub fn device(&self) -> Option<DeviceAccess> {
        match self {
            CablePortAccess::Interface(a) => a.device(),
            CablePortAccess::FrontPort(a) => a.device(),
            CablePortAccess::RearPort(a) => a.device(),
        }
    }
    pub fn port(&self) -> CablePort {
        match self {
            CablePortAccess::Interface(a) => CablePort::Interface(a.id()),
            CablePortAccess::FrontPort(a) => CablePort::FrontPort(a.id()),
            CablePortAccess::RearPort(a) => CablePort::RearPort(a.id()),
        }
    }
    pub fn cable(&self) -> Option<CableAccess> {
        match self {
            CablePortAccess::Interface(a) => a.cable(),
            CablePortAccess::FrontPort(a) => a.cable(),
            CablePortAccess::RearPort(a) => a.cable(),
        }
    }
    pub fn next_device_port_id(&self) -> Option<CablePortAccess> {
        match self {
            CablePortAccess::Interface(_) => None,
            CablePortAccess::FrontPort(a) => a.rear_port().map(CablePortAccess::RearPort),
            CablePortAccess::RearPort(a) => a.front_port().map(CablePortAccess::FrontPort),
        }
    }
    pub fn attached_cable_segments(&self) -> Box<[CableConnection]> {
        self.cable()
            .map(move |cable| {
                cable
                    .connections_from_port(self.port())
                    .collect::<Box<[_]>>()
            })
            .unwrap_or_default()
    }
    fn append_cable_segments(
        &self,
        parent_chain: Vec<CableConnection>,
        result: &mut impl FnMut(Box<[CableConnection]>, Option<CablePortAccess>),
    ) {
        let remaining_segments = self.attached_cable_segments();
        if let Some((last_connection, remaining_connections)) = remaining_segments.split_last() {
            for next_segment in remaining_connections {
                self.append_cable_segment(parent_chain.clone(), next_segment, result);
            }
            self.append_cable_segment(parent_chain, last_connection, result);
        } else {
            result(parent_chain.into_boxed_slice(), Some(self.clone()));
        }
    }
    fn append_cable_segment(
        &self,
        mut parent_chain: Vec<CableConnection>,
        connection: &CableConnection,
        result: &mut impl FnMut(Box<[CableConnection]>, Option<CablePortAccess>),
    ) {
        if let Some(next_port) = connection.far.next_device_port_id() {
            parent_chain.push(connection.clone());
            next_port.append_cable_segments(parent_chain, result);
        } else {
            parent_chain.push(connection.clone());
            result(parent_chain.into_boxed_slice(), None);
        }
    }
    pub fn walk_cable(&self, result: &mut impl FnMut(CablePath)) {
        self.append_cable_segments(
            Vec::new(),
            &mut (|cable_segments, end_port| {
                if end_port.as_ref() != Some(self) {
                    result(CablePath {
                        start_port: self.clone(),
                        cable_segments,
                        end_port,
                    });
                }
            }),
        );
    }
    pub fn collect_cables(&self) -> Box<[CablePath]> {
        let mut result = Vec::new();
        self.walk_cable(&mut |cable_path| result.push(cable_path));
        result.into_boxed_slice()
    }
}
