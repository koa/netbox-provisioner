//! A Wireless LAN Group whose WLANs have different VLANs: the controller of `/interface/wifi`
//! sets each WLAN's VLAN, the CAPs' radios tag it (`wifi-qcom`); 802.11ac radios
//! (`wifi-qcom-ac`) can't, a wAP ac runs them as `wlan<N>` of the legacy CAPsMAN instead.
use crate::{
    device::ros::{
        CapsManDeviceDataCurrent, CapsManDeviceDataTarget, SetupError, WifiDeviceDataCurrent,
        WifiDeviceDataTarget,
        WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
        graphql::{provided_by_routeros, reprovisioned_menus},
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
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        CapsManDatapathVlanMode, InterfaceWifiConfigurationByName, InterfaceWifiDatapathByName,
        ReferenceType, WifiManagement,
    },
    resource::{ResourceMutation, ResourceMutationOperation},
    value::HasNone,
};
use std::net::{IpAddr, Ipv4Addr};

/// `[controller, ax CAP, wAP ac on wifi-qcom-ac, wAP ac on wireless]`, with a WLAN per VLAN
/// id given (SSID `wlan<vid>`), all in one VLAN group with a VXLAN.
async fn wlan_group(vlan_ids: &[u16]) -> [DeviceAccess; 4] {
    wlan_group_with(vlan_ids, false).await
}

/// Like `wlan_group`, with the controller also an AP of its group if `controller_is_ap`.
async fn wlan_group_with(vlan_ids: &[u16], controller_is_ap: bool) -> [DeviceAccess; 4] {
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
            aps: if controller_is_ap { devices.into() } else { devices[1..].into() },
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
                wlan_ap_of: (idx != 0 || controller_is_ap).then_some(wlan_group),
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

fn wifi(
    device: &DeviceAccess,
    model: &[u8],
    package: RadioPackage,
) -> Result<WifiDeviceDataTarget, SetupError> {
    let setup = L2Setup::new(device, KeepNameGenerator)?;
    let current = WifiDeviceDataCurrent {
        provisioning: Box::new([]),
        datapath: Box::new([]),
        cap: Default::default(),
        capsman: Default::default(),
        wifi: build_wifi_ports(b"C52iG-5HaxD2HaxD"),
        configuration: Box::new([]),
        certificate: Box::new([]),
    };
    let mut target = WifiDeviceDataTarget::new(model);
    target.generate_from(device, &setup, Some(&current), &[], Some(package), &[])?;
    Ok(target)
}

/// The controller sets each WLAN's VLAN in its configuration.
#[tokio::test]
async fn controller_sets_the_vlan_of_each_wlan() {
    let [controller, ..] = wlan_group(&[104, 105]).await;
    let target = wifi(&controller, b"CCR2004-1G-12S+2XS", RadioPackage::WifiQcom).unwrap();
    for vid in [104, 105] {
        let configuration = &target.configuration[&AsciiString::from(format!("wlan{vid}"))].0;
        assert_eq!(configuration.datapath_vlan_id, Some(HasNone::Value(vid)));
    }
}

/// With one VLAN, the CAP sets it on its radios, and the controller none.
#[tokio::test]
async fn one_vlan_stays_on_the_cap() {
    let [controller, ap_ax, ..] = wlan_group(&[104]).await;
    let target = wifi(&controller, b"CCR2004-1G-12S+2XS", RadioPackage::WifiQcom).unwrap();
    let configuration = &target.configuration[&AsciiString::from("wlan104")].0;
    assert_eq!(configuration.datapath_vlan_id, None);
    let target = wifi(&ap_ax, b"C52iG-5HaxD2HaxD", RadioPackage::WifiQcom).unwrap();
    let datapath = &target.datapath.values().next().unwrap().0;
    assert_eq!(datapath.vlan_id, Some(HasNone::Value(104)));
}

/// The ax CAP's datapath gives the bridge only: the VLAN comes with each configuration.
#[tokio::test]
async fn ax_cap_tags_the_vlans_of_the_controller() {
    let [_, ap_ax, ..] = wlan_group(&[104, 105]).await;
    let target = wifi(&ap_ax, b"C52iG-5HaxD2HaxD", RadioPackage::WifiQcom).unwrap();
    let datapath = &target.datapath.values().next().unwrap().0;
    assert_eq!(datapath.bridge, Some(HasNone::Value("bridge-caps".into())));
    assert_eq!(datapath.vlan_id, None);
    // the radio refers to the local configuration, which CAPsMAN binds it through
    let configuration = &target.configuration[&AsciiString::from("capsman")].0;
    assert_eq!(configuration.manager, Some(WifiManagement::Capsman));
    assert_eq!(configuration.datapath, Some("bridge-caps".into()));
    let radio = &target.wifi[&Some(AsciiString::from("wifi1"))];
    assert_eq!(radio.name, AsciiString::from("wifi1"));
    assert_eq!(radio.configuration, Some("capsman".into()));
    assert_eq!(radio.datapath, None);
    assert_eq!(radio.datapath_vlan_id, None);
}

/// A radio CAPsMAN has bound can't be changed (7.23.8, also with the CAP off): it stays as it
/// is, with the configuration and datapath it uses, and a different configuration is a
/// warning. The configuration it uses is changed instead.
#[tokio::test]
async fn bound_radio_stays_and_its_configuration_is_changed() {
    let [_, ap_ax, ..] = wlan_group(&[104, 105]).await;
    let setup = L2Setup::new(&ap_ax, KeepNameGenerator).unwrap();
    let mut radios = build_wifi_ports(b"C52iG-5HaxD2HaxD");
    // wifi1 is the radio NetBox has, wifi2 is unknown to it
    radios[0].data.configuration = Some("own".into());
    radios[1].data.configuration = Some("capsman".into());
    // reported from the configuration's datapath; `switch` is no bridge of NetBox's
    for radio in radios.iter_mut() {
        radio.data.datapath_bridge = Some("switch".into());
    }
    let configuration = |name: &str, datapath: &str| {
        let mut configuration = InterfaceWifiConfigurationByName::default();
        configuration.0.name = name.into();
        configuration.0.manager = Some(WifiManagement::Capsman);
        configuration.0.datapath = Some(datapath.into());
        configuration
    };
    let datapath = |name: &str| {
        let mut datapath = InterfaceWifiDatapathByName::default();
        datapath.0.name = name.into();
        datapath.0.bridge = Some(HasNone::Value("switch".into()));
        datapath
    };
    let current = WifiDeviceDataCurrent {
        provisioning: Box::new([]),
        datapath: Box::new([datapath("dp-caps"), datapath("dp-own")]),
        cap: Default::default(),
        capsman: Default::default(),
        wifi: radios.clone(),
        configuration: Box::new([configuration("capsman", "dp-caps"), configuration("own", "dp-own")]),
        certificate: Box::new([]),
    };
    let bound = [AsciiString::from("wifi1"), AsciiString::from("wifi2")];
    let mut target = WifiDeviceDataTarget::new(b"C52iG-5HaxD2HaxD");
    let warnings = target
        .generate_from(&ap_ax, &setup, Some(&current), &[], Some(RadioPackage::WifiQcom), &bound)
        .unwrap();
    assert_eq!(target.wifi[&Some(AsciiString::from("wifi1"))], radios[0].data);
    assert_eq!(target.wifi[&Some(AsciiString::from("wifi2"))], radios[1].data);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("wifi1"), "{warnings:?}");
    // `capsman` gets the datapath NetBox wants, `own` stays with its datapath
    let capsman = &target.configuration[&AsciiString::from("capsman")].0;
    assert_eq!(capsman.datapath, Some("bridge-caps".into()));
    assert_eq!(target.configuration[&AsciiString::from("own")].0.datapath, Some("dp-own".into()));
    assert!(target.datapath.contains_key(&AsciiString::from("dp-own")));
    assert!(!target.datapath.contains_key(&AsciiString::from("dp-caps")));

    // the old datapath goes only after the configuration left it
    let mutations = target.generate_mutations(&current).unwrap();
    let sorted = ResourceMutation::sort_mutations_with_provided_dependencies(
        &mutations,
        provided_by_routeros()
            .into_iter()
            .chain([(ReferenceType::Interface, b"bridge-caps".into())]),
    )
    .unwrap();
    let position = |found: &dyn Fn(&ResourceMutation) -> bool| sorted.iter().position(|m| found(m)).unwrap();
    let configuration_set = position(&|m| {
        m.resource == b"interface/wifi/configuration" && m.fields.iter().any(|f| f.key == b"datapath")
    });
    let datapath_removal = position(&|m| {
        m.resource == b"interface/wifi/datapath"
            && matches!(&m.operation, ResourceMutationOperation::RemoveByKey(key) if key.value.as_ref() == b"dp-caps")
    });
    assert!(configuration_set < datapath_removal, "{sorted:?}");
    let mut script = String::new();
    let mut generator = mikrotik_model::generator::Generator::new(&mut script);
    for mutation in sorted.iter() {
        generator.append_mutation(mutation).unwrap();
    }
    println!("--- Szenario: gebundene Radios, ihre Konfiguration bekommt den Datapath ---\n{script}");
    // nothing set on the bound radios
    assert!(
        !sorted
            .iter()
            .any(|m| m.resource == b"interface/wifi" && !m.fields.is_empty()),
        "{sorted:?}"
    );
}

/// A wAP ac on `wifi-qcom-ac` can't tag the VLANs, so it can't be in the group.
#[tokio::test]
async fn ac_cap_without_tagging_is_refused() {
    let [_, _, ap_ac, _] = wlan_group(&[104, 105]).await;
    assert_eq!(
        needed_radio_package(&ap_ac, b"RBwAPG-5HacD2HnD"),
        Some(RadioPackage::WifiQcomAc)
    );
    match wifi(&ap_ac, b"RBwAPG-5HacD2HnD", RadioPackage::WifiQcomAc) {
        Err(SetupError::WlanVlansNotTagged(name)) => assert_eq!(&*name, "ap-ac"),
        other => panic!("{:?}", other.map(|_| ())),
    }
}

/// A controller also marked as AP of its group, but without radios in NetBox, has nothing
/// to tag: it runs `wireless` for the legacy CAPsMAN, which isn't refused.
#[tokio::test]
async fn controller_without_radios_as_ap_is_not_refused() {
    let [controller, ..] = wlan_group_with(&[104, 105], true).await;
    assert!(controller.wifi_cap_of().is_some());
    let package = needed_radio_package(&controller, b"CCR2004-1G-12S+2XS");
    assert_eq!(package, Some(RadioPackage::Wireless));
    let setup = L2Setup::new(&controller, KeepNameGenerator).unwrap();
    let mut target = WifiDeviceDataTarget::new(b"CCR2004-1G-12S+2XS");
    target
        .generate_from(&controller, &setup, None, &[], package, &[])
        .unwrap();
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
    wifi.generate_from(&ap_legacy, &setup, Some(&current), &[], Some(RadioPackage::Wireless), &[])
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

/// A new WLAN on the controller reprovisions the radios of both CAPsMANs; a change to the
/// CAP's own settings doesn't.
#[tokio::test]
async fn new_wlan_reprovisions_the_caps() {
    let [controller, ..] = wlan_group(&[104, 105]).await;
    let wifi = wifi(&controller, b"CCR2004-1G-12S+2XS", RadioPackage::WifiQcom).unwrap();
    let wifi_current = WifiDeviceDataCurrent {
        provisioning: Box::new([]),
        datapath: Box::new([]),
        cap: Default::default(),
        capsman: Default::default(),
        wifi: build_wifi_ports(b"C52iG-5HaxD2HaxD"),
        configuration: Box::new([]),
        certificate: Box::new([]),
    };
    let mut capsman = CapsManDeviceDataTarget::new(b"CCR2004-1G-12S+2XS");
    capsman.generate_from(&controller).unwrap();
    let capsman_current = CapsManDeviceDataCurrent {
        manager: Default::default(),
        security: Box::new([]),
        datapath: Box::new([]),
        configuration: Box::new([]),
        provisioning: Box::new([]),
    };
    let mutations = [
        wifi.generate_mutations(&wifi_current).unwrap(),
        capsman.generate_mutations(&capsman_current).unwrap(),
    ]
    .concat();
    assert_eq!(
        reprovisioned_menus(&mutations),
        ["/interface/wifi/radio", "/caps-man/radio"]
    );
    let cap_only = mutations
        .iter()
        .filter(|mutation| mutation.resource == b"interface/wifi/cap")
        .cloned()
        .collect::<Vec<_>>();
    assert!(!cap_only.is_empty());
    assert!(reprovisioned_menus(&cap_only).is_empty());
}

/// With three WLANs the provisioning has two slaves; the sort finds each configuration
/// (it once waited for one named after the whole list).
#[tokio::test]
async fn provisioning_with_two_slaves_sorts() {
    let [controller, ..] = wlan_group(&[104, 105, 106]).await;
    let wifi = wifi(&controller, b"CCR2004-1G-12S+2XS", RadioPackage::WifiQcom).unwrap();
    let current = WifiDeviceDataCurrent {
        provisioning: Box::new([]),
        datapath: Box::new([]),
        cap: Default::default(),
        capsman: Default::default(),
        wifi: build_wifi_ports(b"C52iG-5HaxD2HaxD"),
        configuration: Box::new([]),
        certificate: Box::new([]),
    };
    let mutations = wifi.generate_mutations(&current).unwrap();
    ResourceMutation::sort_mutations_with_provided_dependencies(
        &mutations,
        [(ReferenceType::Interface, b"lo".into())],
    )
    .unwrap();
}

/// The radios of a CAP that CAPsMAN manages report `configuration=capsman` (a cAP ax,
/// 7.23.7), which no `/interface/wifi/configuration` provides: RouterOS does
/// (`provided_by_routeros`), so the radios' mutations still sort.
#[tokio::test]
async fn cap_radios_managed_by_capsman_sort() {
    let [_, ap_ax, ..] = wlan_group(&[104, 105]).await;
    let setup = L2Setup::new(&ap_ax, KeepNameGenerator).unwrap();
    let mut radios = build_wifi_ports(b"C52iG-5HaxD2HaxD");
    for radio in radios.iter_mut() {
        radio.data.configuration = Some("capsman".into());
    }
    let current = WifiDeviceDataCurrent {
        provisioning: Box::new([]),
        datapath: Box::new([]),
        cap: Default::default(),
        capsman: Default::default(),
        wifi: radios,
        configuration: Box::new([]),
        certificate: Box::new([]),
    };
    let mut target = WifiDeviceDataTarget::new(b"C52iG-5HaxD2HaxD");
    target
        .generate_from(&ap_ax, &setup, Some(&current), &[], Some(RadioPackage::WifiQcom), &[])
        .unwrap();
    let mutations = target.generate_mutations(&current).unwrap();
    assert!(
        mutations
            .iter()
            .any(|mutation| mutation.resource == b"interface/wifi"),
        "{mutations:?}"
    );
    // the bridge comes with the base config
    let bridge = (ReferenceType::Interface, b"bridge-caps".into());
    ResourceMutation::sort_mutations_with_provided_dependencies(
        &mutations,
        provided_by_routeros().into_iter().chain([bridge]),
    )
    .unwrap();
}

/// A radio leaving its datapath: the radio's `datapath` is marked `blocks-removal`, so the
/// datapath is removed after the radio drops it (RouterOS refuses while a radio uses it).
#[test]
fn datapath_is_removed_after_the_radio_drops_it() {
    use mikrotik_model::resource::{Updatable, generate_add_update_remove_by_key};
    let target_radios = build_wifi_ports(b"C52iG-5HaxD2HaxD");
    let mut current_radios = target_radios.clone();
    for radio in current_radios.iter_mut() {
        radio.data.datapath = Some("bridge-caps".into());
    }
    let mut datapath = InterfaceWifiDatapathByName::default();
    datapath.0.name = "bridge-caps".into();
    let current_datapaths = [datapath];
    let mutations = generate_add_update_remove_by_key(
        &current_datapaths,
        std::iter::empty::<std::borrow::Cow<InterfaceWifiDatapathByName>>(),
    )
    .chain(
        target_radios
            .iter()
            .zip(&current_radios)
            .map(|(target, current)| target.calculate_update(current)),
    )
    .collect::<Vec<_>>();
    let sorted = ResourceMutation::sort_mutations(&mutations).unwrap();
    assert_eq!(
        sorted.iter().map(|m| m.resource).collect::<Vec<_>>(),
        [
            b"interface/wifi".as_slice(),
            b"interface/wifi",
            b"interface/wifi/datapath"
        ]
    );
}
