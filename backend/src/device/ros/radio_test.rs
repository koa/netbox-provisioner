//! Point-to-point radio bridges: a bridge of `ether1` and the radio, with the radio
//! configured from its NetBox RF role and wireless link.

use crate::{
    device::ros::{
        BaseDeviceDataTarget, W60GDeviceDataCurrent, W60GDeviceDataTarget, WirelessDeviceDataCurrent,
        WirelessDeviceDataTarget,
        l2::{EndpointNameGenerator, L2Setup},
    },
    topology::{
        Device, Interface, PhysicalPortId, Radio, RadioLink, RadioRole, TopologyHolder,
        access::device::DeviceAccess, test::TopologyBuilder,
    },
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        EnabledDisabled, InterfaceW60GByDefaultName, InterfaceW60GMode,
        InterfaceWirelessByDefaultName, InterfaceWirelessChannelWidth, InterfaceWirelessMode,
        InterfaceWirelessWirelessProtocol,
    },
    value::Auto,
};

/// A bridge `bridge` of `ether1` and `radio_port`, with `radio` on the radio port.
async fn radio_bridge(radio_port: PhysicalPortId, radio: Option<Radio>) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let device = builder.devices.next_id();
    builder.devices.insert(
        device,
        Device {
            name: "hegglin-schopf".into(),
            ..Default::default()
        },
    );
    let bridge = builder.interfaces.next_id();
    builder.interfaces.insert(
        bridge,
        Interface {
            name: "bridge".into(),
            device,
            ..Default::default()
        },
    );
    for (port, radio) in [(PhysicalPortId::Ethernet(1), None), (radio_port, radio)] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: port.to_string().into(),
                device,
                external: Some(port),
                bridge: Some(bridge),
                radio,
                ..Default::default()
            },
        );
    }
    let holder = Into::<TopologyHolder>::into(builder);
    holder.devices_by_id(device).await.expect("device")
}

fn link(psk: Option<&str>) -> Option<RadioLink> {
    Some(RadioLink {
        ssid: "hegglin-schopf".into(),
        psk: psk.map(Into::into),
    })
}

#[tokio::test]
async fn test_60ghz_ap_joins_its_stations_into_the_bridge() {
    let device = radio_bridge(
        PhysicalPortId::W60g(1),
        Some(Radio {
            role: RadioRole::Ap,
            frequency: None,
            channel_width: None,
            link: link(Some("secret-key")),
        }),
    )
    .await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut target = W60GDeviceDataTarget::new(b"RBwAPG-60ad-A");
    target.generate_from(&device, &l2).unwrap();

    let radio = &target.radio[&AsciiString::from("wlan60-1")];
    let (name, bridge) = l2.placement_of_port(PhysicalPortId::W60g(1)).unwrap();
    assert_eq!(&radio.name, name);
    assert_eq!(radio.mode, Some(InterfaceW60GMode::Bridge));
    assert_eq!(radio.ssid, Some(AsciiString::from("hegglin-schopf")));
    assert_eq!(radio.password, Some(AsciiString::from("secret-key")));
    assert_eq!(radio.frequency, Some(Auto::Auto));
    assert_eq!(radio.put_stations_in_bridge.as_ref(), bridge);
    assert!(bridge.is_some(), "the radio is a bridge port");

    // Only the managed fields change; region, scan list etc. stay as they are.
    let current = W60GDeviceDataCurrent {
        radio: W60GDeviceDataTarget::new(b"RBwAPG-60ad-A")
            .radio
            .into_iter()
            .map(|(default_name, data)| InterfaceW60GByDefaultName { default_name, data })
            .collect(),
    };
    let mutations = target.generate_mutations(&current).unwrap();
    let mut changed = mutations
        .iter()
        .flat_map(|m| m.fields.iter())
        .map(|field| String::from_utf8_lossy(field.key).into_owned())
        .collect::<Vec<_>>();
    changed.sort();
    assert_eq!(
        changed,
        [
            "frequency",
            "mode",
            "name",
            "password",
            "put-stations-in-bridge",
            "ssid"
        ]
    );
}

#[tokio::test]
async fn test_5ghz_station_uses_nv2_with_the_link_key() {
    let device = radio_bridge(
        PhysicalPortId::Wlan(1),
        Some(Radio {
            role: RadioRole::Station,
            frequency: Some(5500),
            channel_width: Some(80),
            link: link(Some("secret-key")),
        }),
    )
    .await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3");
    target.generate_from(&device, &l2).unwrap();

    let wlan = &target.wireless[&Some(AsciiString::from("wlan1"))];
    assert_eq!(wlan.mode, Some(InterfaceWirelessMode::StationBridge));
    assert_eq!(
        wlan.wireless_protocol,
        Some(InterfaceWirelessWirelessProtocol::Nv2Nstreme80211)
    );
    assert_eq!(wlan.nv_2_security, Some(EnabledDisabled::Enabled));
    assert_eq!(wlan.nv_2_preshared_key, Some(AsciiString::from("secret-key")));
    assert_eq!(
        wlan.channel_width,
        Some(InterfaceWirelessChannelWidth::_204080MhzXxxx)
    );
    assert_eq!(wlan.frequency, None, "a station follows the AP");

    let current = WirelessDeviceDataCurrent {
        wireless_cap: Default::default(),
        wireless: WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3")
            .wireless
            .into_iter()
            .map(|(default_name, data)| InterfaceWirelessByDefaultName { default_name, data })
            .collect(),
    };
    assert!(target.generate_mutations(&current).is_ok());
}

#[tokio::test]
async fn test_5ghz_ap_without_psk_leaves_the_key_alone() {
    let device = radio_bridge(
        PhysicalPortId::Wlan(1),
        Some(Radio {
            role: RadioRole::Ap,
            frequency: Some(5500),
            channel_width: None,
            link: link(None),
        }),
    )
    .await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3");
    target.generate_from(&device, &l2).unwrap();

    let wlan = &target.wireless[&Some(AsciiString::from("wlan1"))];
    assert_eq!(wlan.mode, Some(InterfaceWirelessMode::Bridge));
    assert_eq!(
        wlan.wireless_protocol,
        Some(InterfaceWirelessWirelessProtocol::Nv2)
    );
    assert_eq!(wlan.frequency, Some(Auto::Value(5500)));
    assert_eq!(wlan.nv_2_preshared_key, None);
    assert_eq!(wlan.nv_2_security, None);
}

#[tokio::test]
async fn test_radio_without_rf_role_is_only_renamed() {
    let device = radio_bridge(PhysicalPortId::Wlan(1), None).await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3");
    target.generate_from(&device, &l2).unwrap();

    let wlan = &target.wireless[&Some(AsciiString::from("wlan1"))];
    assert_eq!(wlan.name, AsciiString::from("w01"));
    assert_eq!(wlan.mode, None);
    assert_eq!(wlan.ssid, None);
}

#[tokio::test]
async fn test_radio_bridge_without_addresses_gets_a_dhcp_client() {
    let device = radio_bridge(PhysicalPortId::W60g(1), None).await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let (_, bridge) = l2.placement_of_port(PhysicalPortId::W60g(1)).unwrap();
    let mut target = BaseDeviceDataTarget::new(b"RBwAPG-60ad").unwrap();
    target.generate_from(&device, EndpointNameGenerator).unwrap();
    assert_eq!(
        target.dhcp_v_4_client.keys().collect::<Vec<_>>(),
        [&(bridge.unwrap().clone(),)]
    );
}

#[tokio::test]
async fn test_known_radio_boards_have_their_ports() {
    for (model, wireless, w60g) in [
        (b"RBDynaDishG-5HacDr3".as_ref(), 1, 0),
        (b"SXT G-5HPacD", 1, 0),
        (b"RBwAPG-60ad", 0, 1),
        (b"RBwAPG-60ad-A", 0, 1),
        (b"RBLHGG-60ad", 0, 1),
    ] {
        let name = String::from_utf8_lossy(model);
        assert!(BaseDeviceDataTarget::new(model).is_ok(), "{name}: ethernet");
        assert_eq!(WirelessDeviceDataTarget::new(model).wireless.len(), wireless, "{name}");
        assert_eq!(W60GDeviceDataTarget::new(model).radio.len(), w60g, "{name}");
    }
}
