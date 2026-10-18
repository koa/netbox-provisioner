//! A Wireless LAN Group with a CAP of the legacy CAPsMAN (a radio `wlan1` in NetBox) next to
//! the controller, which has the WLAN's VLAN (VXLAN of its group) with an address.
use crate::{
    device::ros::{
        BaseDeviceDataTarget, CapsManDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
        hw_facts::RadioPackage,
        l2::{EndpointNameGenerator, KeepNameGenerator, KeptNames, L2Port, L2Setup, NameGenerator},
        needed_radio_package,
        warning::{ApplyWarning, LegacyRadioManaged},
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
    model::{
        CapsAuthenticationTypes, CapsManDatapathVlanMode, CapsManProvisioningNameFormat, InterfaceBridgeProtocolMode,
        InterfaceWirelessByDefaultName,
    },
    value::HasNone,
};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
};

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

/// The legacy CAP forwards its clients itself: like the CAP of `/interface/wifi`, it has the
/// WLAN's VLAN in its bridge and is a VTEP of the VXLAN carrying it to the controller.
#[tokio::test]
async fn legacy_cap_has_the_wlan_vlan() {
    let (controller, legacy_cap, wifi_cap) = wlan_group().await;
    assert!(legacy_cap.legacy_cap_of().is_some() && legacy_cap.wifi_cap_of().is_none());
    assert!(wifi_cap.wifi_cap_of().is_some());

    let setup = L2Setup::new(&legacy_cap, KeepNameGenerator).unwrap();
    assert_eq!(setup.tagged_bridges.len(), 1, "a bridge for the WLAN's VLAN");
    assert_eq!(setup.tagged_bridges[0].name, AsciiString::from("bridge-caps"));

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
            IpAddr::V4(Ipv4Addr::new(192, 168, 100, 6)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 100, 7)),
        ]
    );
}

/// The legacy CAP's radios join the controller's CAPsMAN at its loopback, their clients the
/// CAP's bridge.
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
    assert_eq!(cap.bridge, HasNone::Value("bridge-caps".into()));
}

/// The controller runs the legacy CAPsMAN (and needs `wireless` for it); the CAPs forward
/// the WLAN's clients themselves, tagged with its VLAN.
#[tokio::test]
async fn controller_runs_the_legacy_capsman() {
    let (controller, _, _) = wlan_group().await;
    assert_eq!(
        needed_radio_package(&controller, b"CCR2004-1G-12S+2XS"),
        Some(RadioPackage::Wireless)
    );
    let mut target = CapsManDeviceDataTarget::new(b"CCR2004-1G-12S+2XS");
    target.generate_from(&controller).unwrap();

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
    assert_eq!(datapath.bridge, None);
    assert_eq!(datapath.vlan_mode, Some(CapsManDatapathVlanMode::UseTag));
    assert_eq!(datapath.vlan_id, Some(104));
    assert_eq!(datapath.local_forwarding, Some(true));
    let provisioning = target.provisioning.values().next().unwrap();
    assert_eq!(provisioning.master_configuration, name);
    assert_eq!(provisioning.name_format, CapsManProvisioningNameFormat::PrefixIdentity);
    assert_eq!(provisioning.name_prefix, "cap".into());
}

/// ap-klr-solar: CAPsMAN manages the radio, still named `wlan1`; RouterOS refuses to rename
/// it ("interface managed by CAPsMAN"), so it keeps its name and settings everywhere (CAP,
/// bridge), and NetBox's name is a warning.
#[tokio::test]
async fn managed_legacy_radio_keeps_its_name() {
    let (_, legacy_cap, _) = wlan_group().await;
    let wlan1 = AsciiString::from("wlan1");
    let mut target = WirelessDeviceDataTarget::new(b"SXT G-5HPacD");
    let mut data = target.wireless[&Some(wlan1.clone())].clone();
    data.name = wlan1.clone();
    data.mtu = Some(1500);
    let mut current = WirelessDeviceDataCurrent {
        wireless_cap: Default::default(),
        wireless: Box::new([InterfaceWirelessByDefaultName {
            default_name: Some(wlan1.clone()),
            data: data.clone(),
        }]),
    };
    current.wireless_cap.enabled = true;
    current.wireless_cap.interfaces = [wlan1.clone()].into();

    let kept = current.managed_radios();
    assert_eq!(kept, HashMap::from([(wlan1.clone(), wlan1.clone())]));
    let mut names = EndpointNameGenerator;
    let interface = legacy_cap
        .interfaces()
        .iter()
        .find(|i| i.external_port() == Some(PhysicalPortId::Wlan(1)))
        .unwrap()
        .clone();
    let netbox_name: Box<str> = names.generate_interface_name(&interface).into();
    assert_ne!(&*netbox_name, "wlan1");
    let setup = L2Setup::new(&legacy_cap, KeptNames { inner: names, kept }).unwrap();
    if let Some((name, _)) = setup.placement_of_port(PhysicalPortId::Wlan(1)) {
        assert_eq!(name, &wlan1);
    }
    target
        .generate_from(&legacy_cap, &setup, Some(&current))
        .unwrap();
    let warnings = target.keep_managed_radios(
        &current,
        &HashMap::from([(wlan1.clone(), netbox_name.clone())]),
    );

    assert_eq!(target.wireless[&Some(wlan1.clone())], data);
    assert_eq!(target.wireless_cap.interfaces, [wlan1.clone()].into());
    assert_eq!(
        warnings,
        [ApplyWarning::LegacyRadioManaged(LegacyRadioManaged {
            radio: "wlan1".into(),
            expected: netbox_name,
        })]
    );
}

/// ap-buero-1og: a CAP's `bridge-caps` reaches other bridges only through the WLAN group's
/// VXLAN, a segment shared with all of them, where STP blocked its port; it runs no STP.
#[tokio::test]
async fn caps_bridge_runs_no_stp() {
    let (_, legacy_cap, wifi_cap) = wlan_group().await;
    for (device, model) in [(&legacy_cap, b"SXT G-5HPacD".as_slice()), (&wifi_cap, b"C52iG-5HaxD2HaxD")] {
        let mut target = BaseDeviceDataTarget::new(model).unwrap();
        target.generate_from(device, KeepNameGenerator).unwrap();
        let bridge = &target.bridge[&AsciiString::from("bridge-caps")].0;
        assert_eq!(bridge.protocol_mode, InterfaceBridgeProtocolMode::None, "{}", device.name());
        assert_eq!(bridge.priority, None);
        assert_eq!(bridge.forward_reserved_addresses, Some(false));
    }
}
