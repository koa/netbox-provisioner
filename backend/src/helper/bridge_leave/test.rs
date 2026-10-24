use super::*;
use crate::topology::{access::interface::InterfaceAccess, interfaces::test::Net};

fn input(net: &Net, name: &str) -> BridgeLeaveInput {
    BridgeLeaveInput {
        device: net.r1.0,
        port: net.interface(net.r1, name).0,
    }
}

fn port(topology: &Arc<Topology>, id: u32) -> InterfaceAccess {
    topology.get_interface_by_id(InterfaceId(id)).unwrap()
}

/// The last port of the customer bridge leaves it: free again, the bridge named as emptied.
#[test]
fn the_last_port_leaves_and_the_bridge_is_named() {
    let net = Net::new();
    let input = input(&net, "ether4");
    let topology = Arc::new(net.builder.build());
    let plan = plan(&topology, &input).expect("plan");
    assert!(
        matches!(&plan.steps[..], [PlanStep::UpdatePort(update)]
            if update.leaves_bridge.as_deref() == Some("bridge-clients") && update.bridge_id.is_none()),
        "{:?}",
        plan.steps
    );
    assert_eq!(
        plan.emptied_bridge.as_ref().map(|b| &*b.name),
        Some("bridge-clients")
    );
}

/// In the preview of the stored export, a bridge's port is in none anymore.
#[test]
fn the_preview_has_the_port_without_bridge() {
    let topology = crate::topology::test::example_topology();
    let port = topology
        .list_devices()
        .filter(|d| d.has_routeros())
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| i.bridge().is_some() && i.is_ethernet_port())
        .expect("a bridge port");
    let input = BridgeLeaveInput {
        device: port.device().unwrap().id().0,
        port: port.id().0,
    };
    let after = plan(&topology, &input).expect("plan").preview_topology().expect("preview");
    assert!(self::port(&after, input.port).bridge().is_none());
}

/// A port with a VLAN of its own loses it too; another port stays, the bridge isn't emptied.
#[test]
fn its_vlan_goes_and_another_port_stays() {
    let mut net = Net::new();
    let bridge = net.interface(net.r1, "bridge-clients");
    let ether5 = net.interface(net.r1, "ether5");
    let vlan = net.builder.interfaces[&bridge].vlan;
    let data = net.builder.interfaces.get_mut(&ether5).unwrap();
    data.bridge = Some(bridge);
    data.vlan = vlan;
    let input = input(&net, "ether5");
    let topology = Arc::new(net.builder.build());
    let plan = plan(&topology, &input).expect("plan");
    assert!(
        matches!(&plan.steps[..], [PlanStep::UpdatePort(_), PlanStep::UpdatePortVlan(update)]
            if update.vlan.is_none() && update.vlan_before.as_ref().map(|v| v.vid) == Some(10)),
        "{:?}",
        plan.steps
    );
    assert!(plan.emptied_bridge.is_none());
}

/// A port in no bridge has nothing to leave.
#[test]
fn a_port_without_bridge_is_refused() {
    let net = Net::new();
    let input = input(&net, "ether5");
    let topology = Arc::new(net.builder.build());
    assert!(matches!(plan(&topology, &input), Err(Error::NoBridge(_))));
}
