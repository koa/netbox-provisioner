//! Scope: Tests of `graphql::resident`; nothing else.
use super::{create_resident_schema, resident};
use crate::role::Role;
use crate::topology::{
    OwnerId, TopologyHolder,
    test::{OwnedExample, owned_example, topology_from},
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
    let meier = resident(&topology, OwnerId(1)).expect("owner 1");
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
    let huber = resident(&topology, OwnerId(2)).expect("owner 2");
    assert!(huber.ports.is_empty());
    assert_eq!(huber.networks.len(), 1);
    assert_ne!(huber.networks[0].id, vlan);
    assert!(resident(&topology, OwnerId(3)).is_none());
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
        owners: Box::new([OwnerId(2)]),
    };
    assert_eq!(
        ask(resident_role).await,
        json!({"me": {"role": "RESIDENT"}, "residents": [{"id": 2, "name": "Huber AG"}]})
    );
    assert_eq!(
        ask(Role::Admin).await,
        json!({"me": {"role": "ADMIN"}, "residents": [
            {"id": 1, "name": "Familie Meier"}, {"id": 2, "name": "Huber AG"}
        ]})
    );
    assert_eq!(
        ask(Role::Nobody).await,
        json!({"me": {"role": "NOBODY"}, "residents": []})
    );
}
