use super::*;
use crate::{
    device::neighbors::Neighbors,
    topology::interfaces::test::{Net, address},
};
use std::{collections::BTreeMap, sync::Arc};

fn row(fields: &[(&str, &str)]) -> Row {
    fields
        .iter()
        .map(|(k, v)| (Box::from(*k), Some(Box::from(*v))))
        .collect()
}

fn ospf(interface: &str, router_id: &str, address: &str, state: &str) -> Row {
    row(&[
        ("instance", "default-v2"),
        ("interface", interface),
        ("router-id", router_id),
        ("address", address),
        ("state", state),
    ])
}

fn live_row<'l>(live: &'l InterfaceLive, device: &DeviceAccess, name: &str) -> &'l LiveRow {
    let id = device
        .interfaces()
        .iter()
        .find(|i| i.name() == name)
        .unwrap()
        .id()
        .0;
    live.rows.iter().find(|row| row.interface == id).unwrap()
}

#[test]
fn rows_get_their_neighbors_ospf_and_bridges() {
    let mut net = Net::new();
    // b2 is reached at its loopback, b1 has none
    let loopback = address(&mut net.builder, "172.16.1.4/32");
    net.builder.devices.get_mut(&net.b2).unwrap().primary_ip_v4 = Some(loopback);
    let topology = Arc::new(net.builder.build());
    let r1 = topology.get_device_by_id(&net.r1).unwrap();
    let r2 = topology.get_device_by_id(&net.r2).unwrap();
    let far = r2.interfaces().iter().find(|i| i.name() == "ether1").unwrap().clone();
    let seen = SeenNeighbor {
        identity: "r2".into(),
        interface_name: "ether1".into(),
        board: "CCR".into(),
        device: Some(device_ref(&r2)),
        interface: Some(crate::topology::findings::interface_ref(&far)),
    };
    let neighbors = Neighbors {
        findings: Vec::new(),
        seen: BTreeMap::from([(net.link.0, vec![seen.clone()])]),
    };
    let ospf = [
        // the direct link, found by the router ID
        ospf("ether1", "172.16.1.2", "172.17.1.1", "Full"),
        // the radio link: r2 by its address, b2 by its loopback, but only in ExStart
        ospf("ether3", "10.0.0.9", "172.17.1.10", "Full"),
        ospf("ether3", "172.16.1.4", "172.17.1.12", "ExStart"),
    ];
    let answering = BTreeSet::from(["172.16.1.4".parse().unwrap()]);
    let rows = r1.interface_rows();
    let live = check(&r1, &rows, neighbors, &ospf, &[], &answering);

    let direct = live_row(&live, &r1, "ether1");
    assert_eq!(direct.seen, [seen]);
    assert_eq!(direct.ospf.len(), 1);
    assert!(direct.ospf[0].adjacent);
    assert_eq!(direct.ospf[0].device.as_ref().map(|d| &*d.name), Some("r2"));
    assert!(direct.ospf_missing.is_empty());
    assert!(direct.bridges.is_empty());

    let radio = live_row(&live, &r1, "ether3");
    let names = |devices: &[DeviceRef]| devices.iter().map(|d| d.name.to_string()).collect::<Vec<_>>();
    assert_eq!(names(&radio.ospf_missing), ["b1", "b2"]);
    assert_eq!(
        radio
            .bridges
            .iter()
            .map(|b| (&*b.device.name, b.address.as_deref(), b.answers))
            .collect::<Vec<_>>(),
        [("b1", None, false), ("b2", Some("172.16.1.4"), true)]
    );
    // the uplink: no OSPF expected, nothing seen
    let uplink = live_row(&live, &r1, "ether2");
    assert!(uplink.ospf_missing.is_empty() && uplink.seen.is_empty());
}

#[test]
fn findings_go_to_the_row_of_their_port() {
    let net = Net::new();
    let topology = Arc::new(net.builder.build());
    let r1 = topology.get_device_by_id(&net.r1).unwrap();
    let link = topology.get_interface_by_id(net.link).unwrap();
    let port = neighbors::PortRef {
        name: "ether1".into(),
        interface: Some(crate::topology::findings::interface_ref(&link)),
    };
    let finding = |kind| NeighborFinding {
        severity: crate::topology::findings::Severity::Hint,
        kind,
    };
    let neighbors = Neighbors {
        findings: vec![
            finding(NeighborFindingKind::NeighborsNotInNetbox(neighbors::NeighborsNotInNetbox {
                port,
                seen: Vec::new(),
            })),
            finding(NeighborFindingKind::DiscoveryOff(neighbors::DiscoveryOff {
                list: "*2000011".into(),
            })),
        ],
        seen: BTreeMap::new(),
    };
    let live = check(&r1, &r1.interface_rows(), neighbors, &[], &[], &BTreeSet::new());
    assert_eq!(live_row(&live, &r1, "ether1").findings.len(), 1);
    assert!(matches!(
        live.other_findings.as_slice(),
        [NeighborFinding { kind: NeighborFindingKind::DiscoveryOff(_), .. }]
    ));
}

#[test]
fn an_ospf_neighbor_goes_to_the_interface_with_the_address() {
    let net = Net::new();
    let topology = Arc::new(net.builder.build());
    let r1 = topology.get_device_by_id(&net.r1).unwrap();
    let neighbors = Neighbors {
        findings: Vec::new(),
        seen: BTreeMap::new(),
    };
    // L2 names the customer bridge otherwise than NetBox
    let ospf = [ospf("bridge-clients-10", "172.16.1.2", "10.10.0.2", "Full")];
    let addresses = [row(&[("address", "10.10.0.1/24"), ("interface", "bridge-clients-10")])];
    let live = check(&r1, &r1.interface_rows(), neighbors, &ospf, &addresses, &BTreeSet::new());
    let customer = live_row(&live, &r1, "bridge-clients");
    assert_eq!(customer.ospf.len(), 1);
    assert_eq!(customer.ospf[0].device.as_ref().map(|d| &*d.name), Some("r2"));
}

/// NetBox has r1 ether1 – r2 ether1, r1 sees r2 ether3 there: „Kabel übernehmen“ to r2 ether3.
/// One seeing a radio bridge device gets no action.
#[test]
fn another_far_port_suggests_taking_over_the_cable() {
    let net = Net::new();
    let topology = Arc::new(net.builder.build());
    let r1 = topology.get_device_by_id(&net.r1).unwrap();
    let r2 = topology.get_device_by_id(&net.r2).unwrap();
    let b1 = topology.get_device_by_id(&net.b1).unwrap();
    let link = topology.get_interface_by_id(net.link).unwrap();
    let far = link.connected_interfaces()[0].clone();
    let interface_ref = crate::topology::findings::interface_ref;
    let seen = |device: &DeviceAccess, name: &str| {
        let interface = device.interfaces().iter().find(|i| i.name() == name).unwrap().clone();
        SeenNeighbor {
            identity: device.name().into(),
            interface_name: name.into(),
            board: "CCR".into(),
            device: Some(device_ref(device)),
            interface: Some(interface_ref(&interface)),
        }
    };
    let finding = |seen: SeenNeighbor| NeighborFinding {
        severity: crate::topology::findings::Severity::Warning,
        kind: NeighborFindingKind::OtherFarPort(neighbors::OtherFarPort {
            port: neighbors::PortRef {
                name: "ether1".into(),
                interface: Some(interface_ref(&link)),
            },
            far_end: interface_ref(&far),
            seen,
        }),
    };
    let actions = |seen: SeenNeighbor| {
        let neighbors = Neighbors {
            findings: vec![finding(seen)],
            seen: BTreeMap::new(),
        };
        let live = check(&r1, &r1.interface_rows(), neighbors, &[], &[], &BTreeSet::new());
        live_row(&live, &r1, "ether1").actions.clone()
    };
    let to_r2 = seen(&r2, "ether3");
    assert_eq!(
        actions(to_r2.clone()),
        [RowAction {
            kind: ActionKind::Suggestion,
            helper: ActionHelper::CableTakeover,
            interface: Some(link.id().0),
            peer: to_r2.interface,
            vlan: None,
            seen: None,
        }]
    );
    let b1_port = b1.interfaces().iter().find(|i| i.radio().is_none()).unwrap().name().to_string();
    assert!(actions(seen(&b1, &b1_port)).is_empty());
}

/// r1 sees a MikroTik NetBox doesn't know at its free ether5: „Neues Gerät“ with its board and
/// port; one without a board (not a MikroTik) gets none.
#[test]
fn an_unknown_mikrotik_suggests_a_new_device() {
    let net = Net::new();
    let ether5 = net.interface(net.r1, "ether5");
    let topology = Arc::new(net.builder.build());
    let r1 = topology.get_device_by_id(&net.r1).unwrap();
    let ether5 = topology.get_interface_by_id(ether5).unwrap();
    let actions = |board: &str| {
        let finding = NeighborFinding {
            severity: crate::topology::findings::Severity::Hint,
            kind: NeighborFindingKind::NeighborsNotInNetbox(neighbors::NeighborsNotInNetbox {
                port: neighbors::PortRef {
                    name: "ether5".into(),
                    interface: Some(crate::topology::findings::interface_ref(&ether5)),
                },
                seen: vec![SeenNeighbor {
                    identity: "MikroTik".into(),
                    interface_name: "ether1".into(),
                    board: board.into(),
                    device: None,
                    interface: None,
                }],
            }),
        };
        let neighbors = Neighbors {
            findings: vec![finding],
            seen: BTreeMap::new(),
        };
        let live = check(&r1, &r1.interface_rows(), neighbors, &[], &[], &BTreeSet::new());
        live_row(&live, &r1, "ether5").actions.clone()
    };
    assert_eq!(
        actions("RB962UiGS-5HacT2HnT"),
        [RowAction {
            kind: ActionKind::Suggestion,
            helper: ActionHelper::NewDevice,
            interface: Some(ether5.id().0),
            peer: None,
            vlan: None,
            seen: Some(SeenDevice {
                identity: "MikroTik".into(),
                board: "RB962UiGS-5HacT2HnT".into(),
                interface_name: "ether1".into(),
            }),
        }]
    );
    assert!(actions("").is_empty());
}
