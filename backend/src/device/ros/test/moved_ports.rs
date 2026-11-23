//! Scope: Tests of ports whose factory names changed since the provisioner named them
//! (`cfg::moved_ports`); nothing else.
use super::empty_current;
use crate::device::ros::{BaseDeviceDataTarget, cfg::moved_ports};
use mikrotik_model::ascii::AsciiString;

fn crs326() -> BaseDeviceDataTarget {
    BaseDeviceDataTarget::new(b"CRS326-24G-2S+").expect("a known board")
}

/// wng-926-sw1 (CRS326-24G-2S+ r2, 7.24.6): named `e01` to `e24` by factory name, then
/// RouterOS reported every port's factory name shifted by 14, the names staying.
#[test]
fn shifted_factory_names_are_found() {
    let target = crs326();
    let mut current = empty_current(&target);
    for port in current.ethernet.iter_mut() {
        let Some(n) = port
            .default_name
            .to_string()
            .strip_prefix("ether")
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        // the row now reporting ether<n> was named for ether<(n + 9) % 24 + 1>
        let named = (n + 9) % 24 + 1;
        let label = match named {
            17 => "-cap-whg-926",
            24 => "-klr-sw-1",
            _ => "",
        };
        port.data.name = AsciiString::from(format!("e{named:02}{label}").as_str());
    }
    let moved = moved_ports(&target, &current);
    assert_eq!(moved.len(), 24, "{moved:?}");
    let cap = moved
        .iter()
        .find(|port| port.name == AsciiString::from("e17-cap-whg-926"))
        .expect("the port to cap-whg-926");
    assert_eq!(cap.default_name, AsciiString::from("ether7"));
    assert_eq!(cap.named_for, AsciiString::from("ether17"));
}

/// Factory names, names by them, hand-made names and an SFP's are no moved port.
#[test]
fn names_by_factory_name_are_no_moved_port() {
    let target = crs326();
    let mut current = empty_current(&target);
    for port in current.ethernet.iter_mut() {
        port.data.name = match port.default_name.to_string().as_str() {
            "ether1" => "e01".into(),
            "ether2" => "e02-buero".into(),
            "ether3" => "uplink".into(),
            "ether4" => "e1".into(),
            "sfp-sfpplus1" => "s01".into(),
            _ => port.default_name.clone(),
        };
    }
    assert_eq!(moved_ports(&target, &current), Vec::new());
}

/// A name another port of the board has by its factory name is moved, though only by one.
#[test]
fn a_name_of_another_port_is_moved() {
    let target = crs326();
    let mut current = empty_current(&target);
    for port in current.ethernet.iter_mut() {
        if port.default_name == AsciiString::from("ether3") {
            port.data.name = "e04-drucker".into();
        }
    }
    let moved = moved_ports(&target, &current);
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].named_for, AsciiString::from("ether4"));
}
