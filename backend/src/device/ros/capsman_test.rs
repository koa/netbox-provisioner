//! A Wireless LAN Group with a CAP of the legacy CAPsMAN (a radio `wlan1` in NetBox) next to
//! the controller, which has the WLAN's VLAN (VXLAN of its group) with an address.
use crate::{
    device::ros::{
        CapsManDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
        hw_facts::RadioPackage,
        l2::{KeepNameGenerator, L2Port, L2Setup},
        needed_radio_package,
    },
    topology::{
        Device, Interface, IpAddressData, PhysicalPortId, TopologyHolder, VlanData,
        VlanGroupData, VxlanData, WlanAuth, WlanData, WlanGroupData, WlanWpaSettings,
        access::device::DeviceAccess, test::TopologyBuilder,
    },
};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString,
    model::{CapsAuthenticationTypes, CapsManDatapathVlanMode},
    value::HasNone,
};
use std::net::{IpAddr, Ipv4Addr};

/// `(controller, legacy CAP, wifi CAP)`
async fn wlan_group() -> (DeviceAccess, DeviceAccess, DeviceAccess) {
    let mut builder = TopologyBuilder::default();
    let controller = builder.devices.next_id();
    let legacy_cap = builder.devices.next_id();
    let wifi_cap = builder.devices.next_id();
    let wlan_group = builder.wlan_groups.next_id();
    let wlan = builder.wlans.next_id();
    let vxlan = builder.vxlans.next_id();
    let vlan_group = builder.vlan_groups.next_id();
    let vlan = builder.vlans.next_id();
    let address = |builder: &mut TopologyBuilder, ip: [u8; 4], len| {
        let id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            id,
            IpAddressData {
                ip: IpNet::new(IpAddr::V4(Ipv4Addr::from(ip)), len).unwrap(),
                interface: None,
                prefix: None,
            },
        );
        id
    };
    let loopbacks = [
        address(&mut builder, [192, 168, 100, 1], 32),
        address(&mut builder, [192, 168, 100, 7], 32),
        address(&mut builder, [192, 168, 100, 6], 32),
    ];
    let gateway = address(&mut builder, [192, 168, 104, 1], 24);

    builder.vlans.insert(
        vlan,
        VlanData {
            name: "lab-wlan".into(),
            vlan_id: 104,
            group: vlan_group,
            terminations: Box::new([]),
            vxlan: Some(vxlan),
            wlans: Box::new([wlan]),
        },
    );
    builder.vlan_groups.insert(
        vlan_group,
        VlanGroupData {
            vlans: Box::new([vlan]),
        },
    );
    builder.vxlans.insert(
        vxlan,
        VxlanData {
            name: "lab-wlan".into(),
            vni: 1,
            vlans: Box::new([vlan]),
        },
    );
    builder.wlans.insert(
        wlan,
        WlanData {
            ssid: "lab wlan".into(),
            vlan: Some(vlan),
            wlan_auth: WlanAuth::Wpa(WlanWpaSettings::new("lab-wlan-preshared-key")),
            wlan_group,
        },
    );
    builder.wlan_groups.insert(
        wlan_group,
        WlanGroupData {
            name: "lab-wlan".into(),
            mgmt_vlan: None,
            controller,
            aps: Box::new([legacy_cap, wifi_cap]),
            wlans: Box::new([wlan]),
        },
    );
    let mut interface = |device, name: &str, external, vlan, ips: Box<[_]>| {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external,
                vlan,
                ips,
                ..Default::default()
            },
        );
    };
    interface(controller, "wlan", None, Some(vlan), Box::new([gateway]));
    interface(legacy_cap, "ether1", Some(PhysicalPortId::Ethernet(1)), None, Box::new([]));
    interface(legacy_cap, "wlan1", Some(PhysicalPortId::Wlan(1)), None, Box::new([]));
    interface(wifi_cap, "ether1", Some(PhysicalPortId::Ethernet(1)), None, Box::new([]));
    interface(wifi_cap, "wifi1", Some(PhysicalPortId::Wifi(1)), None, Box::new([]));
    for (device, name, loopback) in [
        (controller, "lab-core", loopbacks[0]),
        (legacy_cap, "lab-ap1", loopbacks[1]),
        (wifi_cap, "lab-wap", loopbacks[2]),
    ] {
        builder.devices.insert(
            device,
            Device {
                name: name.into(),
                primary_ip_v4: Some(loopback),
                loopback_ip: Some(loopback),
                wlan_controller_of: (device == controller).then_some(wlan_group),
                wlan_ap_of: (device != controller).then_some(wlan_group),
                country: Some("ch".into()),
                ..Default::default()
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    (
        topology.devices_by_id(controller).await.unwrap(),
        topology.devices_by_id(legacy_cap).await.unwrap(),
        topology.devices_by_id(wifi_cap).await.unwrap(),
    )
}

/// The legacy CAP has no VXLAN or VLAN of the WLAN (CAPsMAN forwards its clients), and is
/// no VTEP of the controller's VXLAN, unlike the CAP of `/interface/wifi`.
#[tokio::test]
async fn legacy_cap_has_no_wlan_vlan() {
    let (controller, legacy_cap, wifi_cap) = wlan_group().await;
    assert!(legacy_cap.legacy_cap_of().is_some() && legacy_cap.wifi_cap_of().is_none());
    assert!(wifi_cap.wifi_cap_of().is_some());

    let setup = L2Setup::new(&legacy_cap, KeepNameGenerator).unwrap();
    assert!(setup.tagged_bridges.is_empty(), "no bridge for the WLAN's VLAN");

    let controller_setup = L2Setup::new(&controller, KeepNameGenerator).unwrap();
    let vteps = controller_setup
        .tagged_bridges
        .iter()
        .flat_map(|bridge| bridge.ports.values())
        .flat_map(|entry| &entry.ports)
        .find_map(|port| match port {
            L2Port::VxLan { vxlan, .. } => Some(vxlan.vteps()),
            _ => None,
        })
        .expect("the controller has the WLAN's VXLAN");
    assert_eq!(
        &*vteps,
        [
            IpAddr::V4(Ipv4Addr::new(192, 168, 100, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 100, 6))
        ]
    );
}

/// The legacy CAP's radios join the controller's CAPsMAN at its loopback.
#[tokio::test]
async fn legacy_cap_joins_the_controller() {
    let (_, legacy_cap, _) = wlan_group().await;
    assert_eq!(
        needed_radio_package(&legacy_cap, b"SXT G-5HPacD"),
        Some(RadioPackage::Wireless)
    );
    let setup = L2Setup::new(&legacy_cap, KeepNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"SXT G-5HPacD");
    let current = WirelessDeviceDataCurrent {
        wireless_cap: Default::default(),
        wireless: Box::default(),
    };
    target
        .generate_from(&legacy_cap, &setup, Some(&current))
        .unwrap();
    let cap = &target.wireless_cap;
    assert!(cap.enabled);
    assert_eq!(
        cap.caps_man_addresses,
        [IpAddr::V4(Ipv4Addr::new(192, 168, 100, 1))].into()
    );
    assert_eq!(cap.interfaces, [AsciiString::from("wlan1")].into());
    assert_eq!(cap.bridge, HasNone::NoneValue, "CAPsMAN forwards the clients");
}

/// The controller runs the legacy CAPsMAN (and needs `wireless` for it), its WLAN going
/// into the bridge of the WLAN's VLAN.
#[tokio::test]
async fn controller_runs_the_legacy_capsman() {
    let (controller, _, _) = wlan_group().await;
    assert_eq!(
        needed_radio_package(&controller, b"CCR2004-1G-12S+2XS"),
        Some(RadioPackage::Wireless)
    );
    let setup = L2Setup::new(&controller, KeepNameGenerator).unwrap();
    let bridge = setup.tagged_bridges[0].name.clone();
    let mut target = CapsManDeviceDataTarget::new(b"CCR2004-1G-12S+2XS");
    target.generate_from(&controller, &setup).unwrap();

    assert!(target.manager.enabled);
    let name = AsciiString::from("lab_wlan");
    let configuration = &target.configuration[&name].0;
    assert_eq!(configuration.ssid, Some("lab wlan".into()));
    assert_eq!(configuration.security, Some(name.clone()));
    assert_eq!(configuration.datapath, Some(name.clone()));
    let security = &target.security[&name].0;
    assert_eq!(
        security.authentication_types,
        [CapsAuthenticationTypes::Wpa2Psk].into()
    );
    assert_eq!(security.passphrase, Some("lab-wlan-preshared-key".into()));
    let datapath = &target.datapath[&name].0;
    assert_eq!(datapath.bridge, Some(bridge));
    assert_eq!(datapath.vlan_mode, Some(CapsManDatapathVlanMode::UseTag));
    assert_eq!(datapath.vlan_id, Some(104));
    assert_eq!(datapath.local_forwarding, Some(false));
    let provisioning = target.provisioning.values().next().unwrap();
    assert_eq!(provisioning.master_configuration, name);
}
