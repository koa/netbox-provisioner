//! Scope: Tests of `device::ros::outward`; nothing else.
use super::{Now, announces};
use crate::device::ros::{
    BaseDeviceDataCurrent, BaseDeviceDataTarget,
    l2::{EndpointNameGenerator, L2Setup},
    test::empty_current,
};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        IpAddressByAddress, IpAddressCfg, RoutingOriginateDefault, RoutingOspfInstanceByName,
        RoutingOspfInstanceCfg, RoutingOspfInterfaceTemplateById, RoutingOspfInterfaceTemplateCfg,
    },
};
use std::collections::BTreeSet;

fn net(network: &str) -> IpNet {
    network.parse().unwrap()
}

/// `ether15` faces outward.
fn outward() -> BTreeSet<AsciiString> {
    [AsciiString::from("ether15")].into()
}

fn aggregates() -> [IpNet; 1] {
    [net("192.168.96.0/19")]
}

/// The device as it is: an uplink address on `ether15`, one inside on `ether3`, both in the
/// shared OSPF template, OSPF originating nothing.
fn current(target: &BaseDeviceDataTarget) -> BaseDeviceDataCurrent {
    let mut current = empty_current(target);
    current.ipv_4_address = [
        ("172.17.1.82/30", "ether15"),
        ("192.168.101.14/31", "ether3"),
    ]
    .into_iter()
    .map(|(address, interface)| {
        IpAddressByAddress(IpAddressCfg {
            address: address.parse().unwrap(),
            interface: interface.into(),
            comment: None,
        })
    })
    .collect();
    current.ospf_interface = Box::new([RoutingOspfInterfaceTemplateById {
        id: "*1".into(),
        data: template(&["ether15", "ether3"]),
    }]);
    current.ospf_instance = Box::new([instance(RoutingOriginateDefault::Never)]);
    current
}

/// As the device has it, with its area.
fn template(ports: &[&str]) -> RoutingOspfInterfaceTemplateCfg {
    RoutingOspfInterfaceTemplateCfg {
        area: "backbone-v2".into(),
        ..target_template(ports)
    }
}

/// As `setup_ospf` makes it: the area in the key only.
fn target_template(ports: &[&str]) -> RoutingOspfInterfaceTemplateCfg {
    RoutingOspfInterfaceTemplateCfg {
        interfaces: ports.iter().map(|&port| AsciiString::from(port)).collect(),
        use_bfd: Some(false),
        ..Default::default()
    }
}

/// As the device has it, with its name.
fn instance(originate: RoutingOriginateDefault) -> RoutingOspfInstanceByName {
    RoutingOspfInstanceByName(RoutingOspfInstanceCfg {
        name: "default-v2".into(),
        ..target_instance(originate).0
    })
}

/// As `setup_ospf` makes it: the name in the key only.
fn target_instance(originate: RoutingOriginateDefault) -> RoutingOspfInstanceByName {
    RoutingOspfInstanceByName(RoutingOspfInstanceCfg {
        originate_default: Some(originate),
        ..Default::default()
    })
}

/// The target as the device is now.
fn target() -> BaseDeviceDataTarget {
    let mut target = BaseDeviceDataTarget::new(b"RB750Gr3").unwrap();
    target.set_ip_address(net("172.17.1.82/30"), "ether15");
    target.set_ip_address(net("192.168.101.14/31"), "ether3");
    target.ospf_interface.insert(
        ("backbone-v2".into(), None),
        target_template(&["ether15", "ether3"]),
    );
    target.ospf_instance.insert(
        "default-v2".into(),
        target_instance(RoutingOriginateDefault::Never),
    );
    target
}

fn announces_outward(target: &BaseDeviceDataTarget) -> bool {
    let now = Now {
        addresses: current(target),
        ospf: current(target),
    };
    announces(target, &now, &outward(), &aggregates())
}

/// In line: nothing goes out, also with the uplink's address outside the aggregates.
#[test]
fn in_line_announces_nothing() {
    assert!(!announces_outward(&target()));
}

/// A new address inside the aggregates, on a port inside and in the OSPF template, stays in.
#[test]
fn an_own_address_inside_stays_in() {
    let mut target = target();
    target.set_ip_address(net("192.168.101.16/31"), "ether5");
    target.ospf_interface.insert(
        ("backbone-v2".into(), None),
        target_template(&["ether15", "ether3", "ether5"]),
    );
    assert!(!announces_outward(&target));
}

/// A new address outside the aggregates goes out over OSPF.
#[test]
fn an_address_outside_the_aggregates_goes_out() {
    let mut target = target();
    target.set_ip_address(net("10.9.0.1/24"), "ether5");
    assert!(announces_outward(&target));
}

/// The uplink's address changes, or goes.
#[test]
fn the_uplinks_address_changes() {
    let mut moved = target();
    moved
        .ipv_4_address
        .remove(&"172.17.1.82/30".parse().unwrap());
    assert!(announces_outward(&moved));
    let mut other = target();
    other.set_ip_address(net("192.168.101.20/31"), "ether15");
    assert!(announces_outward(&other));
}

/// OSPF originates the default route.
#[test]
fn a_default_route_goes_out() {
    let mut target = target();
    target.ospf_instance.insert(
        "default-v2".into(),
        target_instance(RoutingOriginateDefault::IfInstalled),
    );
    assert!(announces_outward(&target));
}

/// The uplink's OSPF settings change, or it leaves OSPF.
#[test]
fn the_uplinks_ospf_changes() {
    let mut bfd = target();
    let mut changed = target_template(&["ether15", "ether3"]);
    changed.use_bfd = Some(true);
    bfd.ospf_interface
        .insert(("backbone-v2".into(), None), changed);
    assert!(announces_outward(&bfd));
    let mut left = target();
    left.ospf_interface
        .insert(("backbone-v2".into(), None), target_template(&["ether3"]));
    assert!(announces_outward(&left));
}

/// Production's ports facing outward: the internet uplink, and the OSPF ports whose network no
/// other RouterOS device has (the lab behind srv-router-berg).
#[test]
fn outward_ports_of_the_netbox_example() {
    let topology = crate::topology::test::example_topology();
    let mut found = Vec::new();
    for device in topology.list_devices().filter(|d| d.has_routeros()) {
        let Ok(l2) = L2Setup::new(&device, EndpointNameGenerator) else {
            continue;
        };
        for port in super::outward_ports(&device, &l2) {
            found.push(format!("{}:{port}", device.name()));
        }
    }
    found.sort();
    assert_eq!(
        found,
        [
            "router-schmidrueti:s03-oto-dose-schmidrueti",
            "srv-router-berg:e06-lab",
            "switch-mimi:e04-aussen",
        ]
    );
}
