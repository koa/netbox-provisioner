use crate::topology::access::{
    cable::{CableAccess, CablePortAccess},
    device::DeviceAccess,
    front_port::FrontPortAccess,
    rear_port::RearPortAccess,
};

#[derive(Clone, PartialEq, Eq)]
pub struct CableConnection {
    pub near: CablePortAccess,
    pub far: CablePortAccess,
    pub cable: CableAccess,
}

#[derive(Clone, PartialEq, Eq)]
pub enum DeviceConnection {
    FrontNear {
        near: FrontPortAccess,
        far: RearPortAccess,
        device: DeviceAccess,
    },
    RearNear {
        near: RearPortAccess,
        far: FrontPortAccess,
        device: DeviceAccess,
    },
}

pub struct CablePath {
    pub start_port: CablePortAccess,
    pub cable_segments: Box<[CableConnection]>,
    pub end_port: Option<CablePortAccess>,
}

impl CablePath {
    pub fn far_port(&self) -> &CablePortAccess {
        if let Some(end_port) = self.end_port.as_ref() {
            end_port
        } else if let Some(last_seg) = self.cable_segments.last() {
            &last_seg.far
        } else {
            &self.start_port
        }
    }
}
