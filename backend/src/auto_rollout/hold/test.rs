//! Scope: Tests of `auto_rollout::hold`; nothing else.
use super::*;
use crate::device::neighbors::{
    CableElsewhere, CableMissing, NeighborNotSeen, NeighborsNotInNetbox, PortRef, SeenNeighbor,
};
use crate::topology::findings::{DeviceRef, InterfaceRef, Severity};

fn device(id: u32) -> DeviceRef {
    DeviceRef {
        id,
        name: format!("d{id}").into(),
        site: None,
    }
}

fn interface(device_id: u32, name: &str) -> InterfaceRef {
    InterfaceRef {
        id: device_id * 100,
        name: name.into(),
        device: device(device_id),
    }
}

fn port(name: &str) -> PortRef {
    PortRef {
        name: name.into(),
        interface: Some(interface(1, name)),
    }
}

fn seen(device_id: u32) -> SeenNeighbor {
    SeenNeighbor {
        identity: format!("d{device_id}").into(),
        interface_name: "ether1".into(),
        board: "".into(),
        device: Some(device(device_id)),
        interface: Some(interface(device_id, "ether1")),
    }
}

fn finding(kind: NeighborFindingKind) -> NeighborFinding {
    NeighborFinding {
        severity: Severity::Warning,
        kind,
    }
}

fn missing(at: &str, device_id: u32) -> NeighborFinding {
    finding(NeighborFindingKind::CableMissing(CableMissing {
        port: port(at),
        seen: vec![seen(device_id)],
    }))
}

fn not_seen(at: &str, device_id: u32) -> NeighborFinding {
    finding(NeighborFindingKind::NeighborNotSeen(NeighborNotSeen {
        port: port(at),
        far_end: interface(device_id, "ether1"),
    }))
}

/// The cable moved in NetBox, not yet on site: seen at the old port, not at the new one.
#[test]
fn a_neighbor_seen_at_another_port_than_its_cable_is_rewiring() {
    assert!(rewired(&[missing("ether2", 7), not_seen("ether3", 7)]));
}

/// A link NetBox lacks, or a neighbor switched off, is no rewiring.
#[test]
fn a_missing_cable_or_a_silent_neighbor_alone_is_none() {
    assert!(!rewired(&[missing("ether2", 7)]));
    assert!(!rewired(&[not_seen("ether3", 7)]));
    assert!(!rewired(&[missing("ether2", 7), not_seen("ether3", 8)]));
    assert!(!rewired(&[finding(
        NeighborFindingKind::NeighborsNotInNetbox(NeighborsNotInNetbox {
            port: port("ether2"),
            seen: vec![seen(7)],
        })
    )]));
}

/// Another device where NetBox's cable goes is rewiring.
#[test]
fn another_device_at_the_cable_is_rewiring() {
    assert!(rewired(&[finding(NeighborFindingKind::CableElsewhere(
        CableElsewhere {
            port: port("ether2"),
            far_end: interface(7, "ether1"),
            seen: vec![seen(8)],
        }
    ))]));
}
