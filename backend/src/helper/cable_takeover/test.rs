use super::*;
use crate::{
    helper::{PlanStep, connection},
    topology::{PrefixPool, access::ip_addresses::IpAddressAccess, test::example_topology},
};

/// Two links between routers in the transfer pool, direct cables at four different ports.
fn two_links(topology: &Arc<Topology>) -> [(InterfaceAccess, InterfaceAccess); 2] {
    let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
    let in_pool = |ip: &IpAddressAccess| {
        ip.net()
            .is_some_and(|net| pools.iter().any(|(_, pool)| pool.contains(&net.addr())))
    };
    let mut links = topology
        .list_devices()
        .filter(|d| d.has_routeros())
        .flat_map(|d| d.interfaces().into_vec())
        .filter_map(|port| {
            let far = port.connected_interfaces().first()?.clone();
            far.device().filter(|d| d.has_routeros())?;
            if patched(&port) || patched(&far) {
                return None;
            }
            let own = port.ips().iter().find(|ip| in_pool(ip))?.net()?;
            far.ips()
                .iter()
                .any(|ip| ip.net().is_some_and(|net| net.trunc() == own.trunc()))
                .then_some((port, far))
        });
    let first = links.next().expect("a link");
    let ends = |link: &(InterfaceAccess, InterfaceAccess)| {
        [&link.0, &link.1].map(|end| end.device().unwrap().id())
    };
    let second = links
        .find(|link| ends(link).iter().all(|d| !ends(&first).contains(d)))
        .expect("a link between two other devices");
    [first, second]
}

/// NetBox has `port` – `far` and `seen_far` – `seen`; the device sees `seen` at `port`: both
/// links go, cables and addresses, before the new one.
#[test]
fn both_wrong_links_go() {
    let topology = example_topology();
    let [(port, far), (seen, seen_far)] = two_links(&topology);
    let input = CableTakeoverInput {
        port: port.id().0,
        peer: seen.id().0,
    };
    let plan = clear(&topology, &input).expect("clear").expect("links to clear");
    let cables = plan
        .steps
        .iter()
        .filter(|step| matches!(step, PlanStep::DeleteCable(_)))
        .count();
    assert_eq!(cables, 2, "{:?}", plan.steps);
    for end in [&port, &far, &seen, &seen_far] {
        assert!(
            plan.steps.iter().any(|step| {
                matches!(step, PlanStep::DeleteAddress(delete) if delete.interface.id == end.id().0)
            }),
            "{} {}: {:?}",
            end.device().unwrap().name(),
            end.name(),
            plan.steps
        );
    }
    let after = plan.preview_topology().expect("preview");
    for end in [&port, &seen] {
        let end = after.get_interface_by_id(end.id()).unwrap();
        assert!(end.cable().is_none() && end.ips().is_empty(), "{}", end.name());
    }
}

/// A port cabled to a patch panel is refused: the panels document the building.
#[test]
fn a_patched_port_is_refused() {
    let topology = example_topology();
    let port = topology
        .list_devices()
        .flat_map(|d| d.interfaces().into_vec())
        .find(patched)
        .expect("a port cabled to a patch panel");
    let [(_, far), _] = two_links(&topology);
    let input = CableTakeoverInput {
        port: port.id().0,
        peer: far.id().0,
    };
    assert!(matches!(clear(&topology, &input), Err(Error::PortPatched(_))));
}

/// A link NetBox has already is no takeover: the plan is the one of „Verbindung“.
#[tokio::test]
async fn a_link_in_place_stays() {
    let topology = example_topology();
    let [(port, far), _] = two_links(&topology);
    let input = CableTakeoverInput {
        port: port.id().0,
        peer: far.id().0,
    };
    assert!(clear(&topology, &input).expect("clear").is_none());
    let connect = ConnectionInput {
        interface: port.id().0,
        peer: far.id().0,
        mtu: None,
    };
    assert_eq!(
        plan(&topology, &input).await.expect("plan").steps,
        connection::plan(&topology, &connect).await.expect("plan").steps
    );
}

/// Steps put after others reference the steps they referenced before.
#[test]
fn shifted_steps_keep_their_references() {
    use crate::helper::{CreateAddress, Target};
    let topology = example_topology();
    let device = topology.list_devices().next().unwrap();
    let mut step = PlanStep::CreateAddress(CreateAddress {
        address: "10.0.0.0/31".into(),
        device: crate::helper::device_ref(&device),
        interface: "ether1".into(),
        prefix: "10.0.0.0/31".into(),
        on: Target::Step(1),
    });
    step.shift(3);
    assert!(matches!(step, PlanStep::CreateAddress(CreateAddress { on: Target::Step(4), .. })));
}
