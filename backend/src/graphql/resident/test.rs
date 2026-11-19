//! Scope: Tests of `graphql::resident`; nothing else.
use super::{create_resident_schema, resident};
use crate::role::Role;
use crate::topology::{
    OwnerId, TopologyHolder,
    test::{HUBER, MEIER, OwnedExample, owned_example, topology_from},
};
use serde_json::json;

/// A resident's view: its ports by site, device and name, the port of the other device too,
/// its network with the WLAN; the other resident's network isn't in it.
#[test]
fn a_resident_sees_its_ports_and_networks() {
    let OwnedExample {
        raw,
        device,
        port,
        vlan,
        wlan,
    } = owned_example();
    let topology = topology_from(raw);
    let meier = resident(&topology, MEIER).expect("Meier");
    assert_eq!(&*meier.name, "Familie Meier");
    assert!(meier.ports.iter().any(|p| p.id == port && !p.on_own_device));
    assert!(
        meier
            .ports
            .iter()
            .filter(|p| p.device.id == device)
            .all(|p| p.on_own_device)
    );
    assert!(meier.ports.iter().any(|p| p.device.id == device));
    assert!(
        meier.ports.iter().all(|p| p.name.as_ref() != "lo"),
        "no loopback among the sockets"
    );
    let keys: Vec<_> = meier
        .ports
        .iter()
        .map(|p| {
            (
                p.site.as_ref().map(|site| site.name.clone()),
                p.device.name.clone(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "ports by site and device");
    assert_eq!(meier.networks.len(), 1);
    assert_eq!(meier.networks[0].id, vlan);
    assert_eq!(meier.networks[0].wlans.len(), 1);
    assert_eq!(meier.networks[0].wlans[0].id, wlan);
    let huber = resident(&topology, HUBER).expect("Huber");
    assert!(huber.ports.is_empty());
    assert_eq!(huber.networks.len(), 1);
    assert_ne!(huber.networks[0].id, vlan);
    assert!(resident(&topology, OwnerId(9003)).is_none());
}

/// Over the schema a resident gets only its own, an admin every resident, nobody nothing.
#[tokio::test]
async fn the_schema_gives_each_role_its_residents() {
    let topology = topology_from(owned_example().raw);
    let holder = TopologyHolder::from(topology);
    let schema = create_resident_schema(holder);
    let ask = async |role: Role| {
        let response = schema
            .execute(
                async_graphql::Request::new("{ me { role } residents { id name } }").data(role),
            )
            .await;
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        response.data.into_json().expect("json")
    };
    let resident_role = Role::Resident {
        owners: Box::new([HUBER]),
    };
    assert_eq!(
        ask(resident_role).await,
        json!({"me": {"role": "RESIDENT"}, "residents": [{"id": HUBER.0, "name": "Huber AG"}]})
    );
    let admin = ask(Role::Admin).await;
    assert_eq!(admin["me"]["role"], "ADMIN");
    let names: Vec<_> = admin["residents"]
        .as_array()
        .expect("residents")
        .iter()
        .filter_map(|resident| resident["name"].as_str())
        .collect();
    assert!(
        names.contains(&"Familie Meier") && names.contains(&"Huber AG"),
        "{names:?}"
    );
    assert_eq!(
        ask(Role::Nobody).await,
        json!({"me": {"role": "NOBODY"}, "residents": []})
    );
}

/// A socket in the flat (front port), its rear port cabled to a switch port with the
/// resident's VLAN, the resident's AP plugged in: the socket shows the AP and the VLAN, the AP
/// shows its other port only (its connection is the socket's).
#[test]
fn a_socket_shows_what_is_plugged_in_and_the_network_behind_the_wall() {
    use crate::topology::{
        Cable, CablePort, Device, FrontPort, Interface, OwnerData, PortType, RearPort, VlanData,
        VlanGroupData, builder::TopologyBuilder,
    };
    use std::sync::Arc;
    let mut builder = TopologyBuilder::default();
    let device = |builder: &mut TopologyBuilder, name: &str| {
        let id = builder.devices.next_id();
        let data = Device {
            name: name.into(),
            ..Device::default()
        };
        builder.devices.insert(id, data);
        id
    };
    let switch = device(&mut builder, "keller-switch");
    let wall = device(&mut builder, "wohnung-meier");
    let ap = device(&mut builder, "meier-ap");
    let group = builder.vlan_groups.next_id();
    let vlan = builder.vlans.next_id();
    builder.vlan_groups.insert(
        group,
        VlanGroupData {
            name: "haus".into(),
            vlans: Box::new([vlan]),
        },
    );
    builder.vlans.insert(
        vlan,
        VlanData {
            name: "meier".into(),
            vlan_id: 120,
            group,
            terminations: Box::new([]),
            vxlan: None,
            wlans: Box::new([]),
        },
    );
    let port = |builder: &mut TopologyBuilder, device, name: &str, vlan| {
        let id = builder.interfaces.next_id();
        let data = Interface {
            name: name.into(),
            device,
            port_type: Some(PortType::Ethernet),
            vlan,
            ..Interface::default()
        };
        builder.interfaces.insert(id, data);
        id
    };
    let switch_port = port(&mut builder, switch, "ether3", Some(vlan));
    let uplink = port(&mut builder, ap, "ether1", None);
    let lan = port(&mut builder, ap, "ether2", None);
    let front = builder.front_ports.next_id();
    let rear = builder.rear_ports.next_id();
    builder.front_ports.insert(
        front,
        FrontPort {
            name: "1".into(),
            label: "Wohnzimmer".into(),
            device: wall,
            rear_port: Some(rear),
            cable: None,
        },
    );
    builder.rear_ports.insert(
        rear,
        RearPort {
            name: "1".into(),
            device: wall,
            front_port: Some(front),
            cable: None,
        },
    );
    for (a, b) in [
        (CablePort::Interface(uplink), CablePort::FrontPort(front)),
        (CablePort::RearPort(rear), CablePort::Interface(switch_port)),
    ] {
        let id = builder.cables.next_id();
        let cable = Cable {
            port_a: Box::new([a]),
            port_b: Box::new([b]),
            kind: None,
        };
        builder.cables.insert(id, cable);
    }
    builder.owners.insert(
        OwnerId(1),
        OwnerData {
            name: "Familie Meier".into(),
            devices: Box::new([ap]),
            interfaces: Box::new([]),
            front_ports: Box::new([front]),
            vlans: Box::new([vlan]),
        },
    );
    let topology = Arc::new(builder.build());
    let meier = resident(&topology, OwnerId(1)).expect("owner 1");
    let ids: Vec<_> = meier.ports.iter().map(|p| (p.kind, p.id)).collect();
    assert_eq!(
        ids,
        vec![
            (super::PortKind::Interface, lan.0),
            (super::PortKind::Socket, front.0)
        ],
        "the AP's connection is the socket's"
    );
    let socket = &meier.ports[1];
    assert_eq!(socket.label.as_deref(), Some("Wohnzimmer"));
    assert_eq!(&*socket.device.name, "wohnung-meier");
    assert_eq!(socket.network.as_ref().map(|n| n.id), Some(vlan.0));
    assert!(!socket.foreign_network);
    assert_eq!(socket.far_ends.len(), 1);
    assert_eq!(&*socket.far_ends[0].device.name, "meier-ap");
    assert_eq!(&*socket.far_ends[0].port, "ether1");
    assert!(socket.far_ends[0].own_device);
}

/// Writes every owner of the stored export as its resident sees it to
/// `target/netbox-example/residents.txt`, for review.
#[test]
fn residents_of_netbox_example() {
    let topology = crate::topology::test::example_topology();
    let mut text = String::new();
    for (owner, _) in topology.owners() {
        let Some(resident) = resident(&topology, owner) else {
            continue;
        };
        text.push_str(&format!("{} ({})\n", resident.name, owner.0));
        for port in &resident.ports {
            let far: Vec<String> = port
                .far_ends
                .iter()
                .map(|end| format!("{} {}", end.device.name, end.port))
                .collect();
            let network = match (&port.network, port.foreign_network) {
                (Some(network), _) => network.name.to_string(),
                (None, true) => "(fremdes Netz)".into(),
                (None, false) => "-".into(),
            };
            text.push_str(&format!(
                "  {:?} {} / {} [{}] -> {} | {}\n",
                port.kind,
                port.device.name,
                port.name,
                port.label.as_deref().unwrap_or_default(),
                far.join(", "),
                network
            ));
        }
        for network in &resident.networks {
            text.push_str(&format!("  Netz {} ({})\n", network.name, network.vid));
        }
    }
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/netbox-example");
    std::fs::create_dir_all(dir).expect("target/netbox-example");
    std::fs::write(format!("{dir}/residents.txt"), text).expect("residents.txt");
}
