//! The L3 MTU NetBox gives a physical port, with the L2 MTU raised to carry it.
use crate::{
    device::ros::{
        BaseDeviceDataTarget, SetupError, WirelessDeviceDataTarget, l2::KeepNameGenerator,
        l2::L2Setup,
    },
    topology::{
        Cable, CablePort, Device, Interface, PhysicalPortId, TopologyHolder,
        access::device::DeviceAccess, test::TopologyBuilder,
    },
};
use mikrotik_model::ascii::AsciiString;

/// Two routers with `ether1` cabled, each end with the MTU given, and an SXT with `wlan1`.
async fn topology(mtu_a: Option<u16>, mtu_b: Option<u16>) -> [DeviceAccess; 3] {
    let mut builder = TopologyBuilder::default();
    let devices = [(); 3].map(|_| builder.devices.next_id());
    let mut ends = Vec::new();
    for (device, mtu) in [(devices[0], mtu_a), (devices[1], mtu_b)] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: "ether1".into(),
                device,
                external: Some(PhysicalPortId::Ethernet(1)),
                mtu,
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
            kind: None,
        },
    );
    let wlan = builder.interfaces.next_id();
    builder.interfaces.insert(
        wlan,
        Interface {
            name: "wlan1".into(),
            device: devices[2],
            external: Some(PhysicalPortId::Wlan(1)),
            mtu: Some(2000),
            ..Default::default()
        },
    );
    for (device, name) in devices.iter().zip(["a", "b", "sxt"]) {
        builder.devices.insert(
            *device,
            Device {
                name: name.into(),
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

fn ether1(device: &DeviceAccess) -> Result<(u16, Option<u16>), SetupError> {
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    target.generate_from(device, KeepNameGenerator)?;
    let port = &target.ethernet[&AsciiString::from("ether1")];
    Ok((port.mtu, port.l_2_mtu))
}

#[tokio::test]
async fn ports_keep_the_boards_mtu_without_one_in_netbox() {
    let [a, ..] = topology(None, None).await;
    assert_eq!(ether1(&a).unwrap(), (1500, Some(1592)));
}

/// The L2 MTU leaves room for a VLAN tag.
#[tokio::test]
async fn both_ends_get_the_mtu_of_netbox() {
    let [a, b, _] = topology(Some(2000), Some(2000)).await;
    assert_eq!(ether1(&a).unwrap(), (2000, Some(2004)));
    assert_eq!(ether1(&b).unwrap(), (2000, Some(2004)));
}

/// RouterOS 7's OSPF doesn't form an adjacency over a link with different MTUs.
#[tokio::test]
async fn ends_with_different_mtus_are_refused() {
    let [a, b, _] = topology(Some(2000), None).await;
    for device in [a, b] {
        assert!(matches!(
            ether1(&device),
            Err(SetupError::MtuMismatch { mtu: 2000 | 1500, far_mtu: 1500 | 2000, .. })
        ));
    }
}

#[tokio::test]
async fn radios_get_the_mtu_of_netbox() {
    let [_, _, sxt] = topology(None, None).await;
    let l2 = L2Setup::new(&sxt, KeepNameGenerator).unwrap();
    let mut target = WirelessDeviceDataTarget::new(b"SXT G-5HPacD");
    target.generate_from(&sxt, &l2, None).unwrap();
    let wlan = &target.wireless[&Some(AsciiString::from("wlan1"))];
    assert_eq!((wlan.mtu, wlan.l_2_mtu), (Some(2000), Some(2004)));
}
