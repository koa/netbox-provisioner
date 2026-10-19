mod run;

use super::wireless_partners;
use crate::topology::{
    Cable, CablePort, Device, DeviceId, Interface, InterfaceId, PortType, RadioLink,
    WirelessLinkEnd, builder::TopologyBuilder,
};
use std::sync::Arc;

fn device(builder: &mut TopologyBuilder, name: &str) -> DeviceId {
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: true,
            ..Device::default()
        },
    );
    id
}
fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, link: Option<WirelessLinkEnd>) -> InterfaceId {
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            port_type: Some(if link.is_some() { PortType::Wireless } else { PortType::Ethernet }),
            wireless_link: link,
            ..Interface::default()
        },
    );
    id
}
fn cable(builder: &mut TopologyBuilder, a: InterfaceId, b: InterfaceId) {
    let id = builder.cables.next_id();
    builder.cables.insert(
        id,
        Cable {
            port_a: Box::new([CablePort::Interface(a)]),
            port_b: Box::new([CablePort::Interface(b)]),
            kind: None,
        },
    );
}
fn link_end(peer: InterfaceId) -> WirelessLinkEnd {
    WirelessLinkEnd {
        id: 1,
        peer: Some(peer),
        link: RadioLink {
            ssid: "link-5".into(),
            encrypted: false,
            psk: None,
            country: None,
            frequency_mode: None,
        },
    }
}

/// core -cable- burri -cable- ap ~radio~ station -cable- stall
#[test]
fn finds_the_other_end_of_a_radio_link() {
    let mut b = TopologyBuilder::default();
    let core = device(&mut b, "core");
    let burri = device(&mut b, "burri");
    let ap = device(&mut b, "link-5-ap");
    let station = device(&mut b, "link-5-station");
    let stall = device(&mut b, "stall");
    let c1 = port(&mut b, core, "ether1", None);
    let b1 = port(&mut b, burri, "ether1", None);
    let b2 = port(&mut b, burri, "ether2", None);
    let a1 = port(&mut b, ap, "ether1", None);
    let s1 = port(&mut b, station, "ether1", None);
    let t1 = port(&mut b, stall, "ether5", None);
    cable(&mut b, c1, b1);
    cable(&mut b, b2, a1);
    cable(&mut b, s1, t1);
    // the radios point at each other; their ids are the next two
    let next = b.interfaces.next_id();
    let (ap_radio, station_radio) = (next, InterfaceId(next.0 + 1));
    b.interfaces.insert(
        ap_radio,
        Interface {
            name: "wlan1".into(),
            device: ap,
            port_type: Some(PortType::Wireless),
            wireless_link: Some(link_end(station_radio)),
            ..Interface::default()
        },
    );
    let _ = b.interfaces.next_id();
    b.interfaces.insert(
        station_radio,
        Interface {
            name: "wlan1".into(),
            device: station,
            port_type: Some(PortType::Wireless),
            wireless_link: Some(link_end(ap_radio)),
            ..Interface::default()
        },
    );
    let topology = Arc::new(b.build());
    let get = |id| topology.get_device_by_id(&id).expect("device");
    let partners: Vec<_> = wireless_partners(&get(ap)).iter().map(|d| d.name().to_string()).collect();
    assert_eq!(partners, ["link-5-station"]);
    assert!(wireless_partners(&get(burri)).is_empty());
}
