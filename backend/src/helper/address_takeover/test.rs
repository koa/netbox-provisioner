use super::*;
use crate::topology::interfaces::test::Net;

fn input(net: &Net, interface: &str, address: &str) -> AddressTakeoverInput {
    AddressTakeoverInput {
        device: net.r1.0,
        interface: net.interface(net.r1, interface).0,
        address: address.into(),
    }
}

/// A network NetBox has nothing of becomes a prefix of the device's site, then the address.
#[test]
fn a_new_network_becomes_a_prefix() {
    let net = Net::new();
    let input = input(&net, "ether2", "192.168.5.1/24");
    let topology = Arc::new(net.builder.build());
    let plan = plan(&topology, &input).expect("plan");
    assert!(
        matches!(&plan.steps[..], [PlanStep::CreatePrefix(prefix), PlanStep::CreateAddress(address)]
            if &*prefix.prefix == "192.168.5.0/24" && prefix.pool.is_empty()
                && &*address.address == "192.168.5.1/24" && &*address.interface == "ether2"
                && address.on == Target::Existing(input.interface)),
        "{:?}",
        plan.steps
    );
}

/// A host length or a network NetBox has addresses in gets no prefix.
#[test]
fn a_host_or_a_used_network_gets_no_prefix() {
    let net = Net::new();
    let inputs = [input(&net, "ether2", "192.168.5.1/32"), input(&net, "bridge-clients", "10.10.0.2/24")];
    let topology = Arc::new(net.builder.build());
    for input in inputs {
        let plan = plan(&topology, &input).expect("plan");
        assert!(matches!(&plan.steps[..], [PlanStep::CreateAddress(_)]), "{:?}", plan.steps);
    }
}

/// An address NetBox has, on any device, or one of another device's interface, is refused.
#[test]
fn what_netbox_has_or_another_device_is_refused() {
    let net = Net::new();
    let taken = input(&net, "ether2", "172.17.1.10/24");
    let foreign = AddressTakeoverInput {
        interface: net.interface(net.r2, "ether3").0,
        ..input(&net, "ether2", "192.168.5.1/24")
    };
    let topology = Arc::new(net.builder.build());
    assert!(matches!(plan(&topology, &taken), Err(Error::AddressInNetbox(_))));
    assert!(matches!(plan(&topology, &foreign), Err(Error::UnknownInterface(_))));
}

/// In the preview of the stored export, the interface has the address, in its new prefix.
#[test]
fn the_preview_has_the_address() {
    let topology = crate::topology::test::example_topology();
    let interface = topology
        .list_devices()
        .filter(|d| d.has_routeros())
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| i.is_ethernet_port())
        .expect("a port");
    let input = AddressTakeoverInput {
        device: interface.device().unwrap().id().0,
        interface: interface.id().0,
        address: "198.51.100.7/24".into(),
    };
    let plan = plan(&topology, &input).expect("plan");
    assert!(matches!(plan.steps[0], PlanStep::CreatePrefix(_)), "{:?}", plan.steps);
    let after = plan.preview_topology().expect("preview");
    let ips = after.get_interface_by_id(InterfaceId(input.interface)).unwrap().ips();
    assert!(ips.iter().any(|ip| ip.addr() == Some("198.51.100.7".parse().unwrap())));
}
