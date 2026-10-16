//! mac-telnet on the ports to own devices and the free ones, not towards customers
//! (`BaseDeviceDataTarget::setup_maintenance_ports`).
use crate::{
    device::ros::{BaseDeviceDataTarget, MAINTENANCE_LIST, l2::KeepNameGenerator},
    topology::{
        Cable, CablePort, Device, Interface, PhysicalPortId, PortType, TopologyHolder,
        test::TopologyBuilder,
    },
};
use mikrotik_model::ascii::AsciiString;

/// A router: `ether1` cabled to a switch with RouterOS, `ether2` to a customer's device,
/// `ether3` in NetBox without anything; `ether4` and `ether5` in one bridge, `ether5` cabled to
/// an AP with RouterOS. The board's other ports aren't in NetBox.
#[tokio::test]
async fn allows_mac_telnet_towards_own_devices_and_on_free_ports() {
    let mut builder = TopologyBuilder::default();
    let [core, switch, customer, ap] = [(); 4].map(|_| builder.devices.next_id());
    for (device, name, has_routeros) in [
        (core, "core", true),
        (switch, "switch", true),
        (customer, "customer", false),
        (ap, "ap", true),
    ] {
        builder.devices.insert(
            device,
            Device {
                name: name.into(),
                has_routeros,
                ..Default::default()
            },
        );
    }
    let bridge = builder.interfaces.next_id();
    builder.interfaces.insert(
        bridge,
        Interface {
            name: "bridge".into(),
            device: core,
            port_type: Some(PortType::Bridge),
            ..Default::default()
        },
    );
    let port = |builder: &mut TopologyBuilder, device, number, in_bridge: bool| {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{number}").into(),
                device,
                external: Some(PhysicalPortId::Ethernet(number)),
                bridge: in_bridge.then_some(bridge),
                ..Default::default()
            },
        );
        id
    };
    let core_ports = [1, 2, 3, 4, 5].map(|n| port(&mut builder, core, n, n >= 4));
    let far = [
        (core_ports[0], port(&mut builder, switch, 1, false)),
        (core_ports[1], port(&mut builder, customer, 1, false)),
        (core_ports[4], port(&mut builder, ap, 1, false)),
    ];
    for (a, b) in far {
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(a)]),
                port_b: Box::new([CablePort::Interface(b)]),
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    let core = topology.devices_by_id(core).await.unwrap();
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    target.generate_from(&core, KeepNameGenerator).unwrap();

    let list = AsciiString::from(MAINTENANCE_LIST);
    assert_eq!(target.mac_server.allowed_interface_list, list);
    assert_eq!(target.neighbor_discovery.discover_interface_list, list);
    assert!(target.interface_list.contains_key(&list));
    let members: Vec<_> = target
        .interface_list_member
        .keys()
        .map(|(interface, member_of)| {
            assert_eq!(*member_of, list);
            interface.to_string()
        })
        .collect();
    // the bridge port itself, not the bridge: the other ports of the bridge stay out
    for port in ["ether1", "ether5"] {
        assert!(members.iter().any(|m| m == port), "{port} missing in {members:?}");
    }
    // free: in NetBox without anything, or not there (renamed to the board's short names)
    for port in ["ether3", "e06", "e24", "s01", "s02"] {
        assert!(members.iter().any(|m| m == port), "{port} missing in {members:?}");
    }
    for port in ["ether2", "ether4", "bridge"] {
        assert!(!members.iter().any(|m| m == port), "{port} in {members:?}");
    }
}
