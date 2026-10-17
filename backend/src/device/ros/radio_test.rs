//! Point-to-point radio bridges: a bridge of `ether1` and the radio, with the radio
//! configured from its NetBox RF role and wireless link.

use crate::{
    device::ros::{
        BaseDeviceDataTarget, SetupError, W60GDeviceDataCurrent, W60GDeviceDataTarget,
        WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
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
        EnabledDisabled, InterfaceW60GMode, InterfaceW60GRegion,
        InterfaceWirelessByDefaultName, InterfaceWirelessChannelWidth,
        InterfaceWirelessFrequencyMode, InterfaceWirelessMode, InterfaceWirelessTxPowerMode,
        InterfaceWirelessWdsMode, InterfaceWirelessWirelessProtocol,
    },
    value::Auto,
};

/// A bridge `bridge` of `ether1` and `radio_port`, with `radio` on the radio port,
/// on a device in a site whose topmost region is `country`.
async fn radio_bridge(
    radio_port: PhysicalPortId,
    radio: Option<Radio>,
    country: Option<&str>,
) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let device = builder.devices.next_id();
    builder.devices.insert(
        device,
        Device {
            name: "hegglin-schopf".into(),
            country: country.map(Into::into),
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

fn link(psk: Option<&str>) -> RadioLink {
    RadioLink {
        ssid: "hegglin-schopf".into(),
        encrypted: true,
        psk: psk.map(Into::into),
        country: None,
        frequency_mode: None,
    }
}

fn radio(role: RadioRole, link: RadioLink) -> Radio {
    Radio {
        role,
        frequency: None,
        channel_width: None,
        tx_power: None,
        link: Some(link),
    }
}

/// The 5 GHz radios of `model` as they currently are: the board's profile.
fn wireless_current(model: &[u8]) -> WirelessDeviceDataCurrent {
    WirelessDeviceDataCurrent {
        wireless_cap: Default::default(),
        wireless: WirelessDeviceDataTarget::new(model)
            .wireless
            .into_iter()
            .map(|(default_name, data)| InterfaceWirelessByDefaultName { default_name, data })
            .collect(),
    }
}

/// The 5 GHz radio of a DynaDish set up from NetBox.
async fn dynadish(
    radio: Option<Radio>,
    country: Option<&str>,
) -> Result<WirelessDeviceDataTarget, SetupError> {
    let device = radio_bridge(PhysicalPortId::Wlan(1), radio, country).await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3");
    target.generate_from(
        &device,
        &l2,
        Some(&wireless_current(b"RBDynaDishG-5HacDr3")),
    )?;
    Ok(target)
}

fn wlan1(target: &WirelessDeviceDataTarget) -> &mikrotik_model::model::InterfaceWirelessCfg {
    &target.wireless[&Some(AsciiString::from("wlan1"))]
}

#[tokio::test]
async fn test_60ghz_ap_joins_its_stations_into_the_bridge() {
    let device = radio_bridge(
        PhysicalPortId::W60g(1),
        Some(radio(RadioRole::Ap, link(Some("secret-key")))),
        Some("ch"),
    )
    .await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let current = W60GDeviceDataCurrent {
        radio: W60GDeviceDataTarget::new(b"RBwAPG-60ad-A")
            .radio
            .into_values()
            .collect(),
    };
    let mut target = W60GDeviceDataTarget::new(b"RBwAPG-60ad-A");
    target.generate_from(&device, &l2, Some(&current)).unwrap();

    let radio = &target.radio[&AsciiString::from("wlan60-1")].0;
    // found by its name, so it keeps it in the bridge too
    let (name, bridge) = l2.placement_of_port(PhysicalPortId::W60g(1)).unwrap();
    assert_eq!(name, &AsciiString::from("wlan60-1"));
    assert_eq!(&radio.name, name);
    assert_eq!(radio.mode, Some(InterfaceW60GMode::Bridge));
    assert_eq!(radio.ssid, Some(AsciiString::from("hegglin-schopf")));
    assert_eq!(radio.password, Some(AsciiString::from("secret-key")));
    assert_eq!(radio.frequency, Some(Auto::Auto));
    assert_eq!(radio.region, Some(InterfaceW60GRegion::Eu));
    assert_eq!(radio.put_stations_in_bridge.as_ref(), bridge);
    assert!(bridge.is_some(), "the radio is a bridge port");

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
            "isolate-stations",
            "mode",
            "password",
            "put-stations-in-bridge",
            "region",
            "ssid"
        ]
    );
}

#[tokio::test]
async fn test_5ghz_station_uses_nv2_with_the_link_key() {
    let mut station = radio(RadioRole::Station, link(Some("secret-key")));
    station.frequency = Some(5500);
    station.channel_width = Some(80);
    let target = dynadish(Some(station), Some("ch")).await.unwrap();

    let wlan = wlan1(&target);
    assert_eq!(wlan.mode, Some(InterfaceWirelessMode::StationBridge));
    assert_eq!(
        wlan.wireless_protocol,
        Some(InterfaceWirelessWirelessProtocol::Nv2Nstreme80211)
    );
    assert_eq!(wlan.nv_2_security, Some(EnabledDisabled::Enabled));
    assert_eq!(
        wlan.nv_2_preshared_key,
        Some(AsciiString::from("secret-key"))
    );
    assert_eq!(
        wlan.channel_width,
        Some(InterfaceWirelessChannelWidth::_204080MhzXxxx)
    );
    assert_eq!(
        wlan.frequency,
        Some(Auto::Auto),
        "a station scans for its AP"
    );
    assert_eq!(wlan.country, Some(AsciiString::from("switzerland")));
    assert_eq!(
        wlan.frequency_mode,
        Some(InterfaceWirelessFrequencyMode::RegulatoryDomain)
    );
    assert_eq!(wlan.antenna_gain, Some(25));
    assert!(
        target
            .generate_mutations(&wireless_current(b"RBDynaDishG-5HacDr3"))
            .is_ok()
    );
}

#[tokio::test]
async fn test_5ghz_ap_of_an_open_link_has_no_key() {
    let mut open = link(None);
    open.encrypted = false;
    let mut ap = radio(RadioRole::Ap, open);
    ap.frequency = Some(5500);
    let target = dynadish(Some(ap), Some("ch")).await.unwrap();

    let wlan = wlan1(&target);
    assert_eq!(wlan.mode, Some(InterfaceWirelessMode::Bridge));
    assert_eq!(
        wlan.wireless_protocol,
        Some(InterfaceWirelessWirelessProtocol::Nv2)
    );
    assert_eq!(wlan.frequency, Some(Auto::Value(5500)));
    assert_eq!(wlan.nv_2_security, Some(EnabledDisabled::Disabled));
    assert_eq!(wlan.nv_2_preshared_key, None);
}

#[tokio::test]
async fn test_encrypted_link_without_key_is_an_error() {
    let result = dynadish(Some(radio(RadioRole::Ap, link(None))), Some("ch")).await;
    assert!(
        matches!(result, Err(SetupError::MissingPsk(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn test_radio_without_country_is_an_error() {
    let result = dynadish(Some(radio(RadioRole::Ap, link(Some("k")))), None).await;
    assert!(
        matches!(result, Err(SetupError::MissingCountry(_))),
        "{result:?}"
    );
    let result = dynadish(Some(radio(RadioRole::Ap, link(Some("k")))), Some("xx")).await;
    assert!(
        matches!(result, Err(SetupError::UnknownCountry(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn test_link_overrides_country_and_frequency_mode() {
    let mut overridden = link(Some("k"));
    overridden.country = Some("etsi 5.5-5.7 outdoor".into());
    overridden.frequency_mode = Some("manual-txpower".into());
    let mut ap = radio(RadioRole::Ap, overridden);
    ap.tx_power = Some(17);
    // the override needs no region
    let target = dynadish(Some(ap), None).await.unwrap();

    let wlan = wlan1(&target);
    assert_eq!(
        wlan.country,
        Some(AsciiString::from("etsi 5.5-5.7 outdoor"))
    );
    assert_eq!(
        wlan.frequency_mode,
        Some(InterfaceWirelessFrequencyMode::ManualTxpower)
    );
    assert_eq!(
        wlan.tx_power_mode,
        Some(InterfaceWirelessTxPowerMode::AllRatesFixed)
    );
    assert_eq!(wlan.tx_power, Some(17));

    let mut invalid = link(Some("k"));
    invalid.frequency_mode = Some("legal".into());
    let result = dynadish(Some(radio(RadioRole::Ap, invalid)), Some("ch")).await;
    assert!(
        matches!(result, Err(SetupError::InvalidFrequencyMode(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn test_rf_role_replaces_leftover_settings() {
    let device = radio_bridge(
        PhysicalPortId::Wlan(1),
        Some(radio(RadioRole::Ap, link(Some("k")))),
        Some("ch"),
    )
    .await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut current = wireless_current(b"RBDynaDishG-5HacDr3");
    current.wireless[0].data.wds_mode = Some(InterfaceWirelessWdsMode::Dynamic);
    current.wireless[0].data.distance = Some("2".into());
    let mut target = WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3");
    target.generate_from(&device, &l2, Some(&current)).unwrap();

    let wlan = wlan1(&target);
    assert_eq!(wlan.wds_mode, Some(InterfaceWirelessWdsMode::Disabled));
    assert_eq!(wlan.distance, Some(AsciiString::from("dynamic")));
}

#[tokio::test]
async fn test_radio_without_rf_role_keeps_its_configuration() {
    let device = radio_bridge(PhysicalPortId::Wlan(1), None, None).await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut current = wireless_current(b"RBDynaDishG-5HacDr3");
    current.wireless[0].data.mode = Some(InterfaceWirelessMode::ApBridge);
    current.wireless[0].data.wds_mode = Some(InterfaceWirelessWdsMode::Dynamic);
    let mut target = WirelessDeviceDataTarget::new(b"RBDynaDishG-5HacDr3");
    target.generate_from(&device, &l2, Some(&current)).unwrap();

    let wlan = wlan1(&target);
    assert_eq!(wlan.name, AsciiString::from("w01"));
    let mut expected = current.wireless[0].data.clone();
    expected.name = "w01".into();
    assert_eq!(wlan, &expected);
}

#[tokio::test]
async fn test_rf_role_on_a_board_without_radio_profile_is_an_error() {
    let device = radio_bridge(
        PhysicalPortId::Wlan(1),
        Some(radio(RadioRole::Ap, link(Some("k")))),
        Some("ch"),
    )
    .await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"CRS109-8G-1S-2HnD");
    let result = target.generate_from(&device, &l2, None);
    assert!(
        matches!(result, Err(SetupError::NoRadioProfile(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn test_radio_bridge_without_addresses_gets_a_dhcp_client() {
    let device = radio_bridge(PhysicalPortId::W60g(1), None, None).await;
    let l2 = L2Setup::new(&device, EndpointNameGenerator).unwrap();
    let (_, bridge) = l2.placement_of_port(PhysicalPortId::W60g(1)).unwrap();
    let mut target = BaseDeviceDataTarget::new(b"RBwAPG-60ad").unwrap();
    target
        .generate_from(&device, EndpointNameGenerator)
        .unwrap();
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
        assert_eq!(
            WirelessDeviceDataTarget::new(model).wireless.len(),
            wireless,
            "{name}"
        );
        assert_eq!(W60GDeviceDataTarget::new(model).radio.len(), w60g, "{name}");
    }
}
