//! Scope: Tests of `poe`; nothing else.
use super::*;
use crate::topology::{
    Cable, CablePort, Device, DeviceId, Interface, InterfaceId, PhysicalPortId, PortType,
    builder::TopologyBuilder,
};
use std::{str::FromStr, sync::Arc};

fn device(builder: &mut TopologyBuilder, name: &str) -> DeviceId {
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: name == "sw",
            ..Device::default()
        },
    );
    id
}
fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str) -> InterfaceId {
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
            ..Interface::default()
        },
    );
    id
}
fn ethernet(name: &str, default_name: &str, poe_out: bool) -> EthernetPort {
    EthernetPort {
        name: name.into(),
        default_name: default_name.into(),
        poe_out,
    }
}
fn status(name: &str, status: &str) -> InterfaceEthernetPoeMonitor {
    monitor_reading(&[("name", name), ("poe-out-status", status)])
}

/// `sw` feeds on `ether2` (renamed, cabled to `ap` in NetBox) and `ether3` (no cable, `cam`
/// seen there); `ether4` only waits for a load.
#[test]
fn powered_ports_with_their_far_end() {
    let mut builder = TopologyBuilder::default();
    let sw = device(&mut builder, "sw");
    let ap = device(&mut builder, "ap");
    let cam = device(&mut builder, "cam");
    let sw2 = port(&mut builder, sw, "ether2");
    let sw3 = port(&mut builder, sw, "ether3");
    port(&mut builder, sw, "ether4");
    let ap1 = port(&mut builder, ap, "ether1");
    let cam1 = port(&mut builder, cam, "eth0");
    let cable = builder.cables.next_id();
    builder.cables.insert(
        cable,
        Cable {
            port_a: Box::new([CablePort::Interface(sw2)]),
            port_b: Box::new([CablePort::Interface(ap1)]),
            kind: None,
        },
    );
    let topology = Arc::new(builder.build());
    let sw = topology.get_device_by_id(&sw).unwrap();
    let ethernet = [
        ethernet("e02-ap", "ether2", true),
        ethernet("ether3", "ether3", true),
        ethernet("ether4", "ether4", true),
    ];
    let monitor = [
        status("e02-ap", "powered-on"),
        status("ether3", "powered-on"),
        status("ether4", "waiting-for-load"),
    ];
    let neighbors = [Seen {
        interface: "ether3,bridge".into(),
        identity: "cam".into(),
        interface_name: "eth0".into(),
        board: "".into(),
    }];
    let observed = compare(&sw, &ethernet, &monitor, &neighbors);
    let seen = observed
        .iter()
        .map(|o| {
            (
                &*o.port.name,
                o.port.interface.as_ref().map(|i| i.id),
                o.far.as_ref().map(|i| i.id),
                o.from_neighbor,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        seen,
        [
            ("e02-ap", Some(sw2.0), Some(ap1.0), false),
            ("ether3", Some(sw3.0), Some(cam1.0), true),
        ]
    );
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

/// `sw` has `pse` on ether2 (no PoE out there), ether3 (cabled to `cam`, nothing drawing
/// power), ether4 (feeding `ap`, whose port isn't `pd`), ether6 (no cable, waiting for its
/// device), ether7 (feeding `ap2`'s `pd`) and `pd` on ether5, where it feeds.
#[test]
fn mismatches_with_the_device() {
    let mut builder = TopologyBuilder::default();
    let sw = device(&mut builder, "sw");
    let names = ["ether2", "ether3", "ether4", "ether5", "ether6", "ether7"];
    let ports = names.map(|name| port(&mut builder, sw, name));
    let modes = [
        PoeMode::Pse,
        PoeMode::Pse,
        PoeMode::Pse,
        PoeMode::Pd,
        PoeMode::Pse,
        PoeMode::Pse,
    ];
    for (port, mode) in ports.iter().zip(modes) {
        builder.interfaces.get_mut(port).unwrap().poe = Some(mode);
    }
    for (name, at) in [("cam", 1), ("ap", 2), ("ap2", 5)] {
        let far_device = device(&mut builder, name);
        let far = port(&mut builder, far_device, "ether1");
        if name == "ap2" {
            builder.interfaces.get_mut(&far).unwrap().poe = Some(PoeMode::Pd);
        }
        cable(&mut builder, ports[at], far);
    }
    let topology = Arc::new(builder.build());
    let sw = topology.get_device_by_id(&sw).unwrap();
    let with_poe = |name: &str, renamed: &str| ethernet(renamed, name, true);
    let ports_read = PoePorts {
        ethernet: vec![
            ethernet("ether2", "ether2", false),
            with_poe("ether3", "e03-cam"),
            with_poe("ether4", "ether4"),
            with_poe("ether5", "ether5"),
            with_poe("ether6", "ether6"),
            with_poe("ether7", "ether7"),
        ]
        .into(),
        monitor: vec![
            status("e03-cam", "waiting-for-load"),
            status("ether4", "powered-on"),
            status("ether5", "powered-on"),
            status("ether6", "waiting-for-load"),
            status("ether7", "powered-on"),
        ]
        .into(),
    };
    let found = mismatches(&sw, &ports_read)
        .into_iter()
        .map(|m| (m.port.id, m.problem))
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            (ports[0].0, PoeProblem::NoPoeOut),
            (ports[1].0, PoeProblem::NoLoad),
            (ports[2].0, PoeProblem::PdMissing),
            (ports[3].0, PoeProblem::FeedsInstead),
        ]
    );
}

/// A port feeding without any PoE mode in NetBox at all is `Undocumented` in [`feeding`],
/// but [`mismatches`] still says nothing about it, same as before `feeding` existed.
#[test]
fn feeding_reports_what_mismatches_leaves_out() {
    let mut builder = TopologyBuilder::default();
    let sw = device(&mut builder, "sw");
    let modeled = port(&mut builder, sw, "ether2");
    let unmodeled = port(&mut builder, sw, "ether3");
    builder.interfaces.get_mut(&modeled).unwrap().poe = Some(PoeMode::Pse);
    let topology = Arc::new(builder.build());
    let sw = topology.get_device_by_id(&sw).unwrap();
    let with_poe = |name: &str| ethernet(name, name, true);
    let ports = PoePorts {
        ethernet: vec![with_poe("ether2"), with_poe("ether3")].into(),
        monitor: vec![
            status("ether2", "powered-on"),
            status("ether3", "powered-on"),
        ]
        .into(),
    };
    let found = feeding(&sw, &ports)
        .into_iter()
        .map(|f| (f.port.id, f.problem))
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            (modeled.0, None),
            (unmodeled.0, Some(PoeProblem::Undocumented))
        ]
    );
    assert!(
        mismatches(&sw, &ports).is_empty(),
        "neither is a mismatch mismatches() would report"
    );
}

/// lab-np2 (netPower 16P, two PSUs): lab-wap at 47.7 V and lab-hex at 24 V in the first
/// group, lab-sxt-b in the second; a port waiting for a load draws nothing.
#[test]
fn a_netpower_group_draws_per_voltage() {
    let ports = PoePorts {
        ethernet: vec![
            ethernet("e03-lab-wap", "ether3", true),
            ethernet("e05-hex", "ether5", true),
            ethernet("e07", "ether7", true),
            ethernet("e13-lab-sxt-b", "ether13", true),
        ]
        .into(),
        monitor: vec![
            monitor_reading(&[
                ("name", "e03-lab-wap"),
                ("poe-out-status", "powered-on"),
                ("poe-out-voltage", "47.7"),
                ("poe-out-current", "52"),
            ]),
            monitor_reading(&[
                ("name", "e05-hex"),
                ("poe-out-status", "powered-on"),
                ("poe-out-voltage", "24.1"),
                ("poe-out-current", "150"),
            ]),
            monitor_reading(&[("name", "e07"), ("poe-out-status", "waiting-for-load")]),
            monitor_reading(&[
                ("name", "e13-lab-sxt-b"),
                ("poe-out-status", "powered-on"),
                ("poe-out-voltage", "24.0"),
                ("poe-out-current", "200"),
            ]),
        ]
        .into(),
    };
    let found = load(b"CRS318-16P-2S+", &ports)
        .into_iter()
        .map(|l| (l.first, l.high_voltage, l.current, l.limit))
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            ("ether1".into(), false, 0.15, Some(2.8)),
            ("ether1".into(), true, 0.052, Some(1.4)),
            ("ether9".into(), false, 0.2, Some(2.8)),
        ]
    );
}

/// A board without entered limits, or one feeding nothing, has no load to show.
#[test]
fn no_limits_no_load() {
    let ports = PoePorts {
        ethernet: vec![ethernet("e01", "ether1", true)].into(),
        monitor: vec![monitor_reading(&[
            ("name", "e01"),
            ("poe-out-status", "powered-on"),
            ("poe-out-voltage", "24.0"),
            ("poe-out-current", "100"),
        ])]
        .into(),
    };
    assert!(load(b"RB5009UPr+S+", &ports).is_empty());
    let idle = PoePorts {
        ethernet: ports.ethernet.clone(),
        monitor: Box::default(),
    };
    assert!(load(b"C52iG-5HaxD2HaxD", &idle).is_empty());
}
