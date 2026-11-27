//! Scope: Tests of `customer_net`; nothing else.
use super::*;
use crate::helper::ObjectId;
use crate::topology::test::example_topology;

/// A network on a VLAN without interface: the preview has the new virtual interface in the
/// VLAN with the gateway and `dhcp`, its prefix scoped to the site.
#[test]
fn a_vlan_gets_its_interface_with_the_gateway() {
    let topology = example_topology();
    let (device, vlan) = topology
        .list_devices()
        .filter(|d| d.has_routeros() && d.site().is_some())
        .find_map(|device| {
            let vlan = topology.list_vlans().find(|vlan| {
                vlan.devices().iter().any(|d| d.id() == device.id())
                    && vlan_interface(&device, vlan).is_none()
            })?;
            Some((device, vlan))
        })
        .expect("a VLAN without interface");
    let site = device.site().unwrap().id();
    let name: Box<str> = vlan.name().unwrap().into();
    let mut plan = NetboxPlan::new(&device, None);
    plan.steps = vec![
        PlanStep::CreateInterface(CreateInterface {
            device: plan_device(&device),
            name: name.clone(),
            kind: "virtual".into(),
            vlan: Some(vlan.id.0),
            tags: vec![InterfaceTag::Dhcp].into(),
            mac: None,
        }),
        PlanStep::CreatePrefix(CreatePrefix {
            prefix: "198.51.100.0/24".into(),
            pool: "198.51.100.0/22".into(),
            description: "test".into(),
            site: Some(site.0),
            vlan: Some(vlan.id.0),
            dhcp_lease_time: None,
        }),
        PlanStep::CreateAddress(CreateAddress {
            address: "198.51.100.1/24".into(),
            device: plan_device(&device),
            interface: name.clone(),
            prefix: "198.51.100.0/24".into(),
            on: ObjectId::Planned(0),
        }),
    ];
    let after = plan.preview_topology().expect("preview");
    let device_after = after.get_device_by_id(&device.id()).unwrap();
    let vlan_after = after.list_vlans().find(|v| v.id == vlan.id).unwrap();
    let interface = vlan_interface(&device_after, &vlan_after).expect("the new interface");
    assert_eq!(interface.name(), &*name);
    assert!(interface.is_enable_dhcp_server());
    let address = ipv4(&interface).expect("the gateway");
    assert_eq!(address.to_string(), "198.51.100.1/24");
    let prefix = interface.ips()[0]
        .prefix()
        .and_then(|p| p.data().cloned())
        .unwrap();
    assert_eq!(prefix.site, Some(site));
}

/// The choices leave out ports with their networks, links to routers and bridge ports, and
/// offer only prefixes of the device's site that aren't a network themselves, by family.
#[test]
fn options_are_free_ports_and_site_prefixes() {
    let topology = example_topology();
    for device in topology.list_devices().filter(|d| d.has_routeros()) {
        let options = options(&device);
        for choice in &options.interfaces {
            let interface = topology
                .get_interface_by_id(InterfaceId(choice.id))
                .unwrap();
            assert!(
                open(&interface) && interface.bridge().is_none(),
                "{choice:?}"
            );
        }
        let site = device.site().map(|s| s.id());
        for (parents, v6) in [(&options.parents, false), (&options.parents_v6, true)] {
            for parent in parents {
                let data = topology.prefix_data(IpPrefixId(parent.id)).unwrap();
                assert_eq!(data.site, site);
                assert!(data.addresses.is_empty() && data.pool.is_none());
                assert_eq!(matches!(data.prefix, IpNet::V6(_)), v6, "{parent:?}");
            }
        }
    }
}

/// The range is the upper half unless chosen, never the network, gateway or broadcast.
#[test]
fn the_dhcp_range_is_the_upper_half_unless_chosen() {
    let range = |net: &str, first, last| {
        dhcp_range(net.parse().unwrap(), DhcpRangeInput { first, last }, "test")
            .map(|range| format!("{} {}", range.start, range.end))
    };
    assert_eq!(
        range("192.0.2.0/24", None, None).unwrap(),
        "192.0.2.128/24 192.0.2.254/24"
    );
    assert_eq!(
        range("192.0.4.0/22", None, None).unwrap(),
        "192.0.6.0/22 192.0.7.254/22"
    );
    assert_eq!(
        range("192.0.2.0/24", Some(100), Some(199)).unwrap(),
        "192.0.2.100/24 192.0.2.199/24"
    );
    assert_eq!(
        range("192.0.2.8/30", None, None).unwrap(),
        "192.0.2.10/30 192.0.2.10/30"
    );
    for (first, last) in [(Some(1), None), (None, Some(255)), (Some(200), Some(100))] {
        assert!(matches!(
            range("192.0.2.0/24", first, last),
            Err(Error::DhcpRangeOutside { length: 24, .. })
        ));
    }
}

/// The gateway is the first address: `.1` of an IPv4 network, `::1` of a `/64`.
#[test]
fn the_gateway_is_the_first_address() {
    let net = |net: &str| gateway(net.parse().unwrap()).to_string();
    assert_eq!(net("192.0.2.0/24"), "192.0.2.1/24");
    assert_eq!(net("2001:db8:0:7::/64"), "2001:db8:0:7::1/64");
}

/// A client network without IPv6 is open for one; one with both, or IPv4 without `dhcp`
/// (an uplink), isn't.
#[test]
fn a_client_network_without_ipv6_is_open() {
    let topology = example_topology();
    let client = |v6: bool| {
        topology
            .list_devices()
            .flat_map(|d| d.interfaces())
            .find(|interface| {
                interface.is_enable_dhcp_server()
                    && ipv4(interface).is_some()
                    && ipv6(interface).is_some() == v6
            })
    };
    if let Some(interface) = client(false) {
        assert!(open(&interface), "{}", interface.name());
    }
    if let Some(interface) = client(true) {
        assert!(!open(&interface), "{}", interface.name());
    }
    let uplink = topology
        .list_devices()
        .flat_map(|d| d.interfaces())
        .find(|interface| !interface.is_enable_dhcp_server() && ipv4(interface).is_some())
        .expect("an address without dhcp");
    assert!(!open(&uplink), "{}", uplink.name());
}
