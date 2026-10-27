//! Applying a config can switch PoE out off, which NetBox doesn't show as a change.
use crate::{
    device::ros::{
        BaseDeviceDataTarget,
        l2::KeepNameGenerator,
        test::empty_current,
        warning::{ApplyWarning, PoeCut},
    },
    topology::{Device, Interface, PhysicalPortId, TopologyHolder, builder::TopologyBuilder},
};
use mikrotik_model::{ascii::AsciiString, model::InterfaceEthernetPoeOut};

/// A PoE switch with `ether1` feeding (`poe_mode=pse`), `ether2` in NetBox without PoE and
/// `ether3` not in NetBox, all three with PoE out on at the device as given, and `powered`
/// the ports powering something.
async fn warnings(current_poe_out: InterfaceEthernetPoeOut, powered: &[&str]) -> Vec<ApplyWarning> {
    let mut builder = TopologyBuilder::default();
    let device = builder.devices.next_id();
    builder.devices.insert(
        device,
        Device {
            name: "np".into(),
            ..Default::default()
        },
    );
    for (port, enable_poe) in [(1, true), (2, false)] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{port}").into(),
                device,
                external: Some(PhysicalPortId::Ethernet(port)),
                poe: enable_poe.then_some(crate::topology::PoeMode::Pse),
                ..Default::default()
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    let device = topology.devices_by_id(device).await.unwrap();
    let mut target = BaseDeviceDataTarget::new(b"CRS318-16P-2S+").unwrap();
    target.generate_from(&device, KeepNameGenerator).unwrap();
    let mut current = empty_current(&target);
    for port in &mut current.ethernet {
        if port.data.poe_out.is_some() {
            port.data.poe_out = Some(current_poe_out);
        }
    }
    assert_eq!(
        target.ethernet[&AsciiString::from("ether2")].poe_out,
        Some(InterfaceEthernetPoeOut::Off)
    );
    let powered = powered.iter().map(|name| AsciiString::from(*name)).collect();
    target.poe_warnings(&current, &powered)
}

/// Only the port NetBox doesn't give PoE to and that has it on loses it; a port NetBox doesn't
/// know keeps the board's default.
#[tokio::test]
async fn warns_where_poe_is_switched_off() {
    let warnings = warnings(InterfaceEthernetPoeOut::AutoOn, &["ether1", "ether2", "ether3"]).await;
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(
        warnings[0],
        ApplyWarning::PoeCut(PoeCut {
            port: "ether2".into(),
            default_name: "ether2".into(),
        })
    );
}

#[tokio::test]
async fn no_warning_where_poe_is_already_off() {
    assert!(warnings(InterfaceEthernetPoeOut::Off, &[]).await.is_empty());
}

/// PoE out on, but nothing to power (`waiting-for-load`): switching it off cuts nothing.
#[tokio::test]
async fn no_warning_where_poe_powers_nothing() {
    assert!(warnings(InterfaceEthernetPoeOut::AutoOn, &["ether1"]).await.is_empty());
}

/// A `pse` port gets `auto-on`, with a passive `poe_type` `forced-on` (a UISP Wave Nano isn't
/// detected as a load), a port without PoE mode `off`.
#[tokio::test]
async fn a_passive_port_feeds_without_detecting() {
    let mut builder = TopologyBuilder::default();
    let device = builder.devices.next_id();
    builder.devices.insert(
        device,
        Device {
            name: "np".into(),
            ..Default::default()
        },
    );
    for (port, poe, passive_poe) in [(1, true, false), (2, true, true), (3, false, true)] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{port}").into(),
                device,
                external: Some(PhysicalPortId::Ethernet(port)),
                poe: poe.then_some(crate::topology::PoeMode::Pse),
                passive_poe,
                ..Default::default()
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    let device = topology.devices_by_id(device).await.unwrap();
    let mut target = BaseDeviceDataTarget::new(b"CRS318-16P-2S+").unwrap();
    target.generate_from(&device, KeepNameGenerator).unwrap();
    let poe_out = |port: &str| target.ethernet[&AsciiString::from(port)].poe_out;
    assert_eq!(poe_out("ether1"), Some(InterfaceEthernetPoeOut::AutoOn));
    assert_eq!(poe_out("ether2"), Some(InterfaceEthernetPoeOut::ForcedOn));
    assert_eq!(poe_out("ether3"), Some(InterfaceEthernetPoeOut::Off));
}
