//! A Wireless LAN Group whose WLANs have different VLANs: 802.11ac radios (`wifi-qcom-ac`)
//! can't tag them, a wAP ac runs them as `wlan<N>` of the legacy CAPsMAN instead.
use crate::{
    device::ros::{
        CapsManDeviceDataTarget, WifiDeviceDataCurrent, WifiDeviceDataTarget,
        WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
        hw_facts::{RadioPackage, build_wifi_ports, build_wireless_ports},
        l2::{KeepNameGenerator, L2Setup},
        needed_radio_package,
    },
    topology::{
        Device, Interface, IpAddressData, PhysicalPortId, TopologyHolder, VlanData,
        VlanGroupData, VxlanData, WlanAuth, WlanData, WlanGroupData, WlanWpaSettings,
        access::device::DeviceAccess, test::TopologyBuilder,
    },
};
use ipnet::IpNet;
use mikrotik_model::{ascii::AsciiString, model::CapsManDatapathVlanMode};
use std::net::{IpAddr, Ipv4Addr};

/// `[controller, ax CAP, wAP ac on wifi-qcom-ac, wAP ac on wireless]`, with a WLAN per VLAN
/// id given (SSID `wlan<vid>`), all in one VLAN group with a VXLAN.
async fn wlan_group(vlan_ids: &[u16]) -> [DeviceAccess; 4] {
    let mut builder = TopologyBuilder::default();
    let devices = [(); 4].map(|_| builder.devices.next_id());
    let wlan_group = builder.wlan_groups.next_id();
    let vxlan = builder.vxlans.next_id();
    let vlan_group = builder.vlan_groups.next_id();
    let mut vlans = Vec::new();
    let mut wlans = Vec::new();
    for vlan_id in vlan_ids {
        let vlan = builder.vlans.next_id();
        let wlan = builder.wlans.next_id();
        builder.vlans.insert(
            vlan,
            VlanData {
                name: format!("vlan{vlan_id}").into(),
                vlan_id: *vlan_id,
                group: vlan_group,
                terminations: Box::new([]),
                vxlan: Some(vxlan),
                wlans: Box::new([wlan]),
            },
        );
        builder.wlans.insert(
            wlan,
            WlanData {
                ssid: format!("wlan{vlan_id}").into(),
                vlan: Some(vlan),
                wlan_auth: WlanAuth::Wpa(WlanWpaSettings::new("preshared-key")),
                wlan_group,
            },
        );
        vlans.push(vlan);
        wlans.push(wlan);
    }
    builder.vlan_groups.insert(
        vlan_group,
        VlanGroupData {
            vlans: vlans.clone().into(),
        },
    );
    builder.vxlans.insert(
        vxlan,
        VxlanData {
            name: "wlan".into(),
            vni: 1,
            vlans: vlans.clone().into(),
        },
    );
    builder.wlan_groups.insert(
        wlan_group,
        WlanGroupData {
            name: "Berg".into(),
            mgmt_vlan: None,
            controller: devices[0],
            aps: devices[1..].into(),
            wlans: wlans.into(),
        },
    );
    let interface = |builder: &mut TopologyBuilder, device, name: &str, external| {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external,
                ..Default::default()
            },
        );
        id
    };
    // the controller ends every WLAN's network
    for vlan in &vlans {
        let id = interface(&mut builder, devices[0], "wlan", None);
        builder.interfaces.get_mut(&id).unwrap().vlan = Some(*vlan);
    }
    for (device, radio) in [
        (devices[1], PhysicalPortId::Wifi(1)),
        (devices[2], PhysicalPortId::Wifi(1)),
        (devices[3], PhysicalPortId::Wlan(1)),
    ] {
        interface(&mut builder, device, "ether1", Some(PhysicalPortId::Ethernet(1)));
        interface(&mut builder, device, &radio.to_string(), Some(radio));
    }
    for (idx, (device, name)) in devices
        .iter()
        .zip(["controller", "ap-ax", "ap-ac", "ap-ac-legacy"])
        .enumerate()
    {
        let loopback = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            loopback,
            IpAddressData {
                ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 100, idx as u8 + 1)), 32)
                    .unwrap(),
                interface: None,
                prefix: None,
            },
        );
        builder.devices.insert(
            *device,
            Device {
                name: name.into(),
                primary_ip_v4: Some(loopback),
                loopback_ip: Some(loopback),
                wlan_controller_of: (idx == 0).then_some(wlan_group),
                wlan_ap_of: (idx != 0).then_some(wlan_group),
                country: Some("ch".into()),
                ..Default::default()
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    let mut result = Vec::new();
    for device in devices {
        result.push(topology.devices_by_id(device).await.unwrap());
    }
    result.try_into().unwrap_or_else(|_| unreachable!())
}

/// A wAP ac with `wlan1` in NetBox runs `wireless`, as a CAP of the legacy CAPsMAN, which
/// gets a datapath per WLAN, tagged with its VLAN and forwarded by the CAP.
#[tokio::test]
async fn ac_cap_on_wireless_joins_the_legacy_capsman() {
    let [controller, _, _, ap_legacy] = wlan_group(&[104, 105]).await;
    assert_eq!(
        needed_radio_package(&ap_legacy, b"RBwAPG-5HacD2HnD"),
        Some(RadioPackage::Wireless)
    );
    assert!(ap_legacy.legacy_cap_of().is_some());
    let mut target = CapsManDeviceDataTarget::new(b"CCR2004-1G-12S+2XS");
    target.generate_from(&controller).unwrap();
    for vid in [104, 105] {
        let datapath = &target.datapath[&AsciiString::from(format!("wlan{vid}"))].0;
        assert_eq!(datapath.vlan_mode, Some(CapsManDatapathVlanMode::UseTag));
        assert_eq!(datapath.vlan_id, Some(vid));
        assert_eq!(datapath.local_forwarding, Some(true));
    }
}

/// After `wifi-qcom-ac` gave way to `wireless`, its radios stay, inactive, under the names L2
/// gave them; they get their default names back for the `wlan<N>`, which `wireless` installs
/// disabled.
#[tokio::test]
async fn ac_cap_moving_to_wireless_frees_the_names() {
    let [_, _, _, ap_legacy] = wlan_group(&[104, 105]).await;
    let setup = L2Setup::new(&ap_legacy, KeepNameGenerator).unwrap();
    let mut left = build_wifi_ports(b"C52iG-5HaxD2HaxD");
    for (idx, port) in left.iter_mut().enumerate() {
        port.data.name = format!("w0{}", idx + 1).into();
    }
    let current = WifiDeviceDataCurrent {
        provisioning: Box::new([]),
        datapath: Box::new([]),
        cap: Default::default(),
        capsman: Default::default(),
        wifi: left,
        configuration: Box::new([]),
        certificate: Box::new([]),
    };
    let mut wifi = WifiDeviceDataTarget::new(b"RBwAPG-5HacD2HnD");
    wifi.generate_from(&ap_legacy, &setup, Some(&current), &[], Some(RadioPackage::Wireless))
        .unwrap();
    for name in ["wifi1", "wifi2"] {
        assert_eq!(wifi.wifi[&Some(AsciiString::from(name))].name, AsciiString::from(name));
    }

    let mut radios = build_wireless_ports(b"RB912UAG-2HPnD");
    radios[0].data.disabled = true;
    let current = WirelessDeviceDataCurrent {
        wireless_cap: Default::default(),
        wireless: radios,
    };
    let mut wireless = WirelessDeviceDataTarget::new(b"RBwAPG-5HacD2HnD");
    wireless
        .generate_from(&ap_legacy, &setup, Some(&current))
        .unwrap();
    assert!(!wireless.wireless[&Some(AsciiString::from("wlan1"))].disabled);
}
