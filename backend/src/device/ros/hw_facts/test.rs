use super::{RadioPackage, build_ethernet_ports, radio_package};
use mikrotik_model::value::{Auto, HasNone};

#[test]
fn crs309_has_one_ether_and_eight_sfp_plus_ports() {
    let names: Vec<String> = build_ethernet_ports(b"CRS309-1G-8S+")
        .iter()
        .map(|port| port.default_name.to_string())
        .collect();
    let mut wanted = vec!["ether1".to_string()];
    wanted.extend((1..=8).map(|n| format!("sfp-sfpplus{n}")));
    assert_eq!(names, wanted);
}

/// klr-sw-1: the module of a cage reports its shutdown temperature on its first lane only.
#[test]
fn crs354_qsfp_lanes_after_the_first_have_no_shutdown_temperature() {
    let ports = build_ethernet_ports(b"CRS354-48G-4S+2Q+");
    let temperatures: Vec<(String, Option<u8>)> = ports
        .iter()
        .filter(|port| port.default_name.to_string().starts_with("qsfpplus"))
        .map(|port| (port.default_name.to_string(), port.data.sfp_shutdown_temperature))
        .collect();
    let wanted: Vec<(String, Option<u8>)> = (1..=2)
        .flat_map(|cage| (1..=4).map(move |lane| (format!("qsfpplus{cage}-{lane}"), (lane == 1).then_some(95))))
        .collect();
    assert_eq!(temperatures, wanted);
}

#[test]
fn crs318_poe_ports_have_their_power_settings() {
    let ports = build_ethernet_ports(b"CRS318-16P-2S+");
    let ether1 = &ports[0].data;
    assert_eq!(ether1.poe_voltage, Some(Auto::Auto));
    assert_eq!(ether1.power_cycle_interval, Some(HasNone::NoneValue));
    assert_eq!(ether1.power_cycle_ping_enabled, Some(false));
    let sfp1 = &ports[16].data;
    assert_eq!(sfp1.poe_voltage, None);
}

#[test]
fn hex_poe_has_poe_out_on_ether2_to_5() {
    let ports = build_ethernet_ports(b"RB960PGS");
    let names: Vec<_> = ports
        .iter()
        .map(|port| String::from_utf8_lossy(&port.default_name).into_owned())
        .collect();
    assert_eq!(names, ["ether1", "ether2", "ether3", "ether4", "ether5", "sfp1"]);
    assert_eq!(ports[0].data.poe_out, None);
    for port in &ports[1..5] {
        assert!(port.data.poe_out.is_some());
        assert_eq!(port.data.power_cycle_interval, Some(HasNone::NoneValue));
        assert_eq!(port.data.power_cycle_ping_enabled, Some(false));
        assert_eq!(port.data.poe_voltage, None);
    }
    assert_eq!(ports[5].data.l_2_mtu, Some(1600));
    assert_eq!(ports[5].data.poe_out, None);
}

#[test]
fn hap_ax2_poe_port_has_its_power_cycle_settings() {
    let ports = build_ethernet_ports(b"C52iG-5HaxD2HaxD");
    assert_eq!(ports.len(), 5);
    let ether1 = &ports[0];
    assert_eq!(&*ether1.default_name, b"ether1");
    assert_eq!(ether1.data.power_cycle_interval, Some(HasNone::NoneValue));
    assert_eq!(ether1.data.power_cycle_ping_enabled, Some(false));
    assert_eq!(ether1.data.poe_voltage, None);
    let ether2 = &ports[1];
    assert_eq!(&*ether2.default_name, b"ether2");
    assert_eq!(ether2.data.power_cycle_ping_enabled, None);
}

#[test]
fn first_wap_ac_has_one_port_and_runs_wireless() {
    let ports = build_ethernet_ports(b"RBwAPG-5HacT2HnD");
    assert_eq!(ports.len(), 1);
    assert_eq!(&*ports[0].default_name, b"ether1");
    assert_eq!(ports[0].data.l_2_mtu, Some(1600));
    assert_eq!(ports[0].data.poe_out, None);
    assert_eq!(radio_package(b"RBwAPG-5HacT2HnD"), Some(RadioPackage::Wireless));
}

#[test]
fn hap_ac_poe_port_has_its_power_cycle_settings() {
    let ports = build_ethernet_ports(b"RB962UiGS-5HacT2HnT");
    let names: Vec<_> = ports
        .iter()
        .map(|port| String::from_utf8_lossy(&port.default_name).into_owned())
        .collect();
    assert_eq!(names, ["ether1", "ether2", "ether3", "ether4", "ether5", "sfp1"]);
    for port in &ports[..4] {
        assert_eq!(port.data.poe_out, None);
        assert_eq!(port.data.power_cycle_ping_enabled, None);
    }
    let ether5 = &ports[4].data;
    assert!(ether5.poe_out.is_some());
    assert_eq!(ether5.power_cycle_interval, Some(HasNone::NoneValue));
    assert_eq!(ether5.power_cycle_ping_enabled, Some(false));
    assert_eq!(ports[5].data.power_cycle_ping_enabled, None);
}
