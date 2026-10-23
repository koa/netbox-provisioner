use super::*;
use crate::topology::{
    interfaces::test::{Net, address},
    test::example_topology,
};

/// The interface `name` of `device`.
fn named(topology: &Arc<Topology>, device: DeviceId, name: &str) -> InterfaceAccess {
    topology
        .get_device_by_id(&device)
        .unwrap()
        .interfaces()
        .into_vec()
        .into_iter()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("{name}"))
}

/// The test network with addresses on the bridges of b1 and b2, in the segment of r1 ether3 and
/// r2 ether2.
fn topology() -> (Arc<Topology>, Net) {
    let mut net = Net::new();
    for (device, ip) in [(net.b1, "172.17.1.11/29"), (net.b2, "172.17.1.12/29")] {
        let id = address(&mut net.builder, ip);
        let bridge = net
            .builder
            .interfaces
            .iter()
            .find(|(_, i)| i.device == device && &*i.name == "bridge")
            .map(|(id, _)| *id)
            .unwrap();
        net.builder.interfaces.get_mut(&bridge).unwrap().ips = Box::new([id]);
    }
    let builder = std::mem::take(&mut net.builder);
    (Arc::new(builder.build()), net)
}

fn steps(plan: &NetboxPlan) -> Vec<String> {
    plan.steps
        .iter()
        .map(|step| match step {
            PlanStep::DeleteAddress(d) => {
                format!("delete {} {} {}", d.interface.device.name, d.interface.name, d.address)
            }
            PlanStep::UpdateInterface(u) => format!(
                "update {} {} label {:?} -{:?}",
                u.interface.device.name, u.interface.name, u.label, u.tags_removed
            ),
            PlanStep::DeleteWirelessLink(d) => format!("delete link {}", d.id),
            PlanStep::DeletePrefix(d) => format!("delete prefix {}", d.prefix),
            other => format!("{other:?}"),
        })
        .collect()
}

/// From either radio: the segment's four addresses, `ospf` and the labels at both cables go;
/// the bridge devices are cut off, the routers have another link.
#[test]
fn the_segment_goes_with_ospf_and_labels() {
    let (topology, net) = topology();
    let radio = named(&topology, net.b1, "wlan1");
    let input = RadioDisconnectInput {
        device: net.r1.0,
        radio: radio.id().0,
        wireless_link: false,
    };
    let plan = plan(&topology, &input).expect("plan");
    assert_eq!(
        steps(&plan),
        [
            "delete r1 ether3 172.17.1.9/29",
            "delete b1 bridge 172.17.1.11/29",
            "delete r2 ether2 172.17.1.10/29",
            "delete b2 bridge 172.17.1.12/29",
            "delete prefix 172.17.1.8/29",
            "update r1 ether3 label Some(\"\") -[Ospf]",
            "update b1 bridge label None -[Ospf]",
            "update r2 ether2 label Some(\"\") -[Ospf]",
            "update b2 bridge label None -[Ospf]",
            "update b1 ether1 label Some(\"\") -[]",
            "update b2 ether1 label Some(\"\") -[]",
        ]
    );
    assert_eq!(
        plan.cut_off.iter().map(|d| &*d.name).collect::<Vec<_>>(),
        ["b1", "b2"]
    );

    let other = named(&topology, net.b2, "wlan1");
    let reverse = RadioDisconnectInput {
        radio: other.id().0,
        ..input
    };
    assert_eq!(super::plan(&topology, &reverse).expect("plan").steps.len(), plan.steps.len());
}

/// Asked, the wireless link goes too; cables and radios stay.
#[test]
fn the_wireless_link_goes_when_asked() {
    let (topology, net) = topology();
    let radio = named(&topology, net.b1, "wlan1");
    let input = RadioDisconnectInput {
        device: net.b1.0,
        radio: radio.id().0,
        wireless_link: true,
    };
    let plan = plan(&topology, &input).expect("plan");
    assert_eq!(steps(&plan).last().map(String::as_str), Some("delete link 1"));
    assert!(!plan.steps.iter().any(|s| matches!(s, PlanStep::DeleteCable(_))));
}

/// schopf-burri – burri-schopf in the export: the preview has the routers' ends without the
/// segment's addresses and the link without its network, the wireless link stays.
#[test]
fn the_preview_has_the_link_without_its_network() {
    let topology = example_topology();
    let device = topology
        .list_devices()
        .find(|d| d.name() == "schopf-burri")
        .unwrap();
    let radio = device
        .interfaces()
        .into_vec()
        .into_iter()
        .find(|i| i.wireless_link().is_some())
        .expect("a radio");
    let input = RadioDisconnectInput {
        device: device.id().0,
        radio: radio.id().0,
        wireless_link: false,
    };
    let plan = plan(&topology, &input).expect("plan");
    let deleted = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::DeleteAddress(d) => Some(d.id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!deleted.is_empty(), "{:#?}", plan.steps);
    let after = plan.preview_topology().expect("preview");
    assert!(
        after.list_devices().all(|d| d
            .interfaces()
            .iter()
            .all(|i| i.ips().iter().all(|ip| !deleted.contains(&ip.id().0)))),
        "the addresses are gone"
    );
    let radio_after = after.get_interface_by_id(radio.id()).unwrap();
    assert_eq!(radio_after.wireless_peer().map(|p| p.id()), radio.wireless_peer().map(|p| p.id()));
}
