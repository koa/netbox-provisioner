//! Scope: Tests of fixed port speeds; nothing else.
//!
//! The speed NetBox gives a port: without auto-negotiation, named with the medium of its cable.
use crate::{
    device::ros::{
        BaseDeviceDataTarget, SetupError, l2::KeepNameGenerator, speed::after_negotiation_on,
        test::empty_current,
    },
    topology::{
        Cable, CablePort, Device, Duplex, Interface, PhysicalPortId, TopologyHolder,
        access::device::DeviceAccess, builder::TopologyBuilder,
    },
};
use mikrotik_model::{ascii::AsciiString, model::EthernetSpeed};

/// klr-sw-1 (CRS354) `qsfpplus1-1` cabled to `sfp-sfpplus1` of a CRS326, each end with the
/// speed given, the cable of type `kind`; and `ether1` of the CRS326 with `ether`.
async fn topology(
    speeds: [Option<u32>; 2],
    kind: Option<&str>,
    ether: Option<(u32, Duplex)>,
) -> [DeviceAccess; 2] {
    let mut builder = TopologyBuilder::default();
    let devices = [(); 2].map(|_| builder.devices.next_id());
    let ports = [
        ("qsfpplus1-1", PhysicalPortId::QsfpPlus(1, 1)),
        ("sfp-sfpplus1", PhysicalPortId::SfpSfpPlus(1)),
    ];
    let mut ends = Vec::new();
    for ((device, (name, port)), speed) in devices.iter().zip(ports).zip(speeds) {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device: *device,
                external: Some(port),
                speed,
                ..Default::default()
            },
        );
        ends.push(id);
    }
    let cable = builder.cables.next_id();
    builder.cables.insert(
        cable,
        Cable {
            port_a: Box::new([CablePort::Interface(ends[0])]),
            port_b: Box::new([CablePort::Interface(ends[1])]),
            kind: kind.map(Into::into),
        },
    );
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: "ether1".into(),
            device: devices[1],
            external: Some(PhysicalPortId::Ethernet(1)),
            speed: ether.map(|(speed, _)| speed),
            duplex: ether.map(|(_, duplex)| duplex),
            ..Default::default()
        },
    );
    for (device, name) in devices.iter().zip(["klr-sw-1", "klr-sw02"]) {
        builder.devices.insert(
            *device,
            Device {
                name: name.into(),
                has_routeros: true,
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

/// auto-negotiation, speed and whether it advertises anything, of `port` on `model`
fn port(
    device: &DeviceAccess,
    model: &[u8],
    port: &str,
) -> Result<(bool, Option<EthernetSpeed>, bool), SetupError> {
    let mut target = BaseDeviceDataTarget::new(model).unwrap();
    target.generate_from(device, KeepNameGenerator)?;
    let port = &target.ethernet[&AsciiString::from(port)];
    Ok((
        port.auto_negotiation,
        port.speed,
        !port.advertise.is_empty(),
    ))
}

const CRS354: &[u8] = b"CRS354-48G-4S+2Q+";
const CRS326: &[u8] = b"CRS326-24G-2S+";

#[tokio::test]
async fn a_port_without_speed_negotiates() {
    let [sw1, sw2] = topology([None, None], Some("dac-passive"), None).await;
    assert_eq!(
        port(&sw1, CRS354, "qsfpplus1-1").unwrap(),
        (true, None, true)
    );
    assert_eq!(port(&sw2, CRS326, "ether1").unwrap(), (true, None, true));
}

/// klr-sw-1's breakout lane to klr-sw02: `auto-negotiation=no speed=10G-baseCR`.
#[tokio::test]
async fn a_dac_at_10g_runs_at_10g_base_cr() {
    let [sw1, sw2] = topology([Some(10_000_000); 2], Some("dac-passive"), None).await;
    let fixed = (false, Some(EthernetSpeed::_10GBaseCr), false);
    assert_eq!(port(&sw1, CRS354, "qsfpplus1-1").unwrap(), fixed);
    assert_eq!(port(&sw2, CRS326, "sfp-sfpplus1").unwrap(), fixed);
    let [sw1, _] = topology([Some(10_000_000); 2], Some("smf-os2"), None).await;
    assert_eq!(
        port(&sw1, CRS354, "qsfpplus1-1").unwrap(),
        (false, Some(EthernetSpeed::_10GBaseSrLr), false)
    );
}

#[tokio::test]
async fn a_cage_needs_the_cable_type() {
    let [sw1, _] = topology([Some(10_000_000); 2], None, None).await;
    assert!(matches!(
        port(&sw1, CRS354, "qsfpplus1-1"),
        Err(SetupError::SpeedNeedsCableType { .. })
    ));
}

/// A fixed speed at one end only, or another one, gives no link.
#[tokio::test]
async fn both_ends_need_the_same_speed() {
    let [sw1, _] = topology([Some(10_000_000), None], Some("dac-passive"), None).await;
    assert!(matches!(
        port(&sw1, CRS354, "qsfpplus1-1"),
        Err(SetupError::SpeedMismatch {
            far_speed: None,
            ..
        })
    ));
}

#[tokio::test]
async fn rj45_takes_the_duplex() {
    let [_, sw2] = topology([None, None], None, Some((100_000, Duplex::Half))).await;
    assert_eq!(
        port(&sw2, CRS326, "ether1").unwrap(),
        (false, Some(EthernetSpeed::_100MBaseTHalf), false)
    );
}

/// 40G on an SFP+ cage: RouterOS has the speed, the board doesn't offer it there.
#[tokio::test]
async fn a_speed_the_port_lacks_is_an_error() {
    let [_, sw2] = topology([Some(40_000_000); 2], Some("dac-passive"), None).await;
    assert!(matches!(
        port(&sw2, CRS326, "sfp-sfpplus1"),
        Err(SetupError::SpeedNotOnPort { .. })
    ));
}

/// A port going back to auto-negotiation (lab-hex `ether2`, 7.24.6): RouterOS refuses
/// `speed=""` and `!speed` is a syntax error in a script, so the `set` leaves the speed out;
/// the port doesn't report it once it negotiates.
#[tokio::test]
async fn negotiating_again_leaves_the_speed_out() {
    let [sw1, _] = topology([None, None], Some("dac-passive"), None).await;
    let mut target = BaseDeviceDataTarget::new(CRS354).unwrap();
    target.generate_from(&sw1, KeepNameGenerator).unwrap();
    let mut current = empty_current(&target);
    let lane = current
        .ethernet
        .iter_mut()
        .find(|port| port.default_name == AsciiString::from("qsfpplus1-1"))
        .unwrap();
    // as klr-sw-1 reports the fixed lane
    lane.data.auto_negotiation = false;
    lane.data.speed = Some(EthernetSpeed::_10GBaseCr);
    lane.data.advertise = Default::default();
    after_negotiation_on(&target, &mut current);
    let mutations = target.generate_mutations(&current).unwrap();
    let fields = |mutation: &mikrotik_model::resource::ResourceMutation| {
        let mut fields = mutation
            .fields
            .iter()
            .map(|field| String::from_utf8_lossy(field.key).into_owned())
            .filter(|key| key != "name")
            .collect::<Vec<_>>();
        fields.sort();
        fields
    };
    let lane = mutations
        .iter()
        .filter(|m| m.resource == b"interface/ethernet")
        .map(fields)
        .find(|fields| fields.iter().any(|key| key == "auto-negotiation"))
        .expect("the lane negotiates again");
    assert_eq!(lane, ["advertise", "auto-negotiation"]);
}

/// A port that stays fixed keeps its speed in the diff: a new one is set.
#[tokio::test]
async fn a_fixed_port_changes_its_speed() {
    let [sw1, _] = topology([Some(10_000_000); 2], Some("dac-passive"), None).await;
    let mut target = BaseDeviceDataTarget::new(CRS354).unwrap();
    target.generate_from(&sw1, KeepNameGenerator).unwrap();
    let mut current = empty_current(&target);
    let lane = current
        .ethernet
        .iter_mut()
        .find(|port| port.default_name == AsciiString::from("qsfpplus1-1"))
        .unwrap();
    lane.data.speed = Some(EthernetSpeed::_10GBaseSrLr);
    after_negotiation_on(&target, &mut current);
    let lane = &current
        .ethernet
        .iter()
        .find(|port| port.default_name == AsciiString::from("qsfpplus1-1"))
        .unwrap()
        .data;
    assert_eq!(lane.speed, Some(EthernetSpeed::_10GBaseSrLr));
}
