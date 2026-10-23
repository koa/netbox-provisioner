use super::*;
use crate::topology::interfaces::test::Net;

fn steps(plan: &NetboxPlan) -> Vec<String> {
    plan.steps
        .iter()
        .map(|step| match step {
            PlanStep::DeleteAddress(d) => {
                format!("delete {} {} {}", d.interface.device.name, d.interface.name, d.address)
            }
            PlanStep::DeletePrefix(d) => format!("delete prefix {}", d.prefix),
            PlanStep::UpdateInterface(u) => {
                format!("update {} {} -{:?}", u.interface.device.name, u.interface.name, u.tags_removed)
            }
            PlanStep::UpdatePortVlan(u) => format!(
                "vlan {} {} {:?} -> {:?}",
                u.interface.device.name,
                u.interface.name,
                u.vlan_before.as_ref().map(|v| v.vid),
                u.vlan.as_ref().map(|v| v.vid)
            ),
            PlanStep::DeleteInterface(d) => {
                format!("delete interface {} {}", d.interface.device.name, d.interface.name)
            }
            other => format!("{other:?}"),
        })
        .collect()
}

fn interface(topology: &Arc<Topology>, device: DeviceId, name: &str) -> InterfaceAccess {
    topology
        .get_device_by_id(&device)
        .unwrap()
        .interfaces()
        .into_vec()
        .into_iter()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("{name}"))
}

/// r1's customer bridge with VLAN 10, an address and `dhcp`: the address, `dhcp` and the VLAN
/// of the bridge and its port go, the bridge stays.
#[test]
fn a_customer_bridge_loses_its_network_and_vlans() {
    let net = Net::new();
    let r1 = net.r1;
    let topology = Arc::new(net.builder.build());
    let bridge = interface(&topology, r1, "bridge-clients");
    let input = CustomerDisconnectInput {
        device: r1.0,
        interface: bridge.id().0,
    };
    assert_eq!(
        steps(&plan(&topology, &input).expect("plan")),
        [
            "delete r1 bridge-clients 10.10.0.1/24",
            "delete prefix 10.10.0.0/24",
            "update r1 bridge-clients -[Dhcp]",
            "vlan r1 bridge-clients Some(10) -> None",
        ]
    );
}

/// A VLAN interface with nothing else goes, after its address; the customer ports with its VLAN
/// lose it.
#[test]
fn a_vlan_interface_goes_with_the_vlan_of_its_ports() {
    let mut net = Net::new();
    let r1 = net.r1;
    let bridge = net
        .builder
        .interfaces
        .iter()
        .find(|(_, i)| i.device == r1 && &*i.name == "bridge-clients")
        .map(|(id, _)| *id)
        .unwrap();
    // the bridge becomes a VLAN interface, its port a port with the VLAN of its own
    let data = net.builder.interfaces.get_mut(&bridge).unwrap();
    data.name = "vlan-clients".into();
    data.port_type = None;
    let vlan = data.vlan;
    for (_, port) in net.builder.interfaces.iter_mut().filter(|(_, i)| i.bridge == Some(bridge)) {
        port.bridge = None;
        port.vlan = vlan;
    }
    let topology = Arc::new(net.builder.build());
    let vlan_interface = interface(&topology, r1, "vlan-clients");
    let input = CustomerDisconnectInput {
        device: r1.0,
        interface: vlan_interface.id().0,
    };
    assert_eq!(
        steps(&plan(&topology, &input).expect("plan")),
        [
            "delete r1 vlan-clients 10.10.0.1/24",
            "delete prefix 10.10.0.0/24",
            "vlan r1 ether4 Some(10) -> None",
            "delete interface r1 vlan-clients",
        ]
    );
}

/// Taken out, the bridge has nothing left to take.
#[test]
fn nothing_left_is_an_error_of_its_own() {
    let mut net = Net::new();
    let r1 = net.r1;
    let (_, bridge) = net
        .builder
        .interfaces
        .iter_mut()
        .find(|(_, i)| i.device == r1 && &*i.name == "bridge-clients")
        .unwrap();
    bridge.ips = Box::new([]);
    bridge.vlan = None;
    bridge.enable_dhcp_server = false;
    bridge.tagged_vlans = Box::new([]);
    let topology = Arc::new(net.builder.build());
    let bridge = interface(&topology, r1, "bridge-clients");
    let input = CustomerDisconnectInput {
        device: r1.0,
        interface: bridge.id().0,
    };
    assert!(matches!(plan(&topology, &input), Err(Error::NoCustomerNet(_))));
}

/// The customer port of r2 with its L3 on r1: only its VLAN goes.
#[test]
fn a_port_with_its_gateway_elsewhere_loses_its_vlan() {
    let net = Net::new();
    let r2 = net.r2;
    let topology = Arc::new(net.builder.build());
    let port = interface(&topology, r2, "ether3");
    let input = CustomerDisconnectInput {
        device: r2.0,
        interface: port.id().0,
    };
    assert_eq!(
        steps(&plan(&topology, &input).expect("plan")),
        ["vlan r2 ether3 Some(10) -> None"]
    );
}
