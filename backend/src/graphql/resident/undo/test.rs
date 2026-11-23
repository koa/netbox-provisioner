//! Scope: Tests of `graphql::resident::undo`; nothing else.
use super::*;
use crate::{
    graphql::resident::{
        FarEnd, Named, ResidentDevice, ResidentNetwork, ResidentPort, ResidentWlan,
        change::{ConnectDeviceChange, DeviceRoomChange, PortVlanChange},
    },
    helper::wlan::WlanInput,
};

fn named(id: u32, name: &str) -> Named {
    Named {
        id,
        name: name.into(),
    }
}

fn port(
    id: u32,
    network: Option<u32>,
    foreign_network: bool,
    far_end: Option<u32>,
) -> ResidentPort {
    ResidentPort {
        id,
        kind: PortKind::Socket,
        name: format!("port{id}").into(),
        label: None,
        device: named(1, "sw"),
        site: None,
        location: Box::new([]),
        on_own_device: false,
        network: network.map(|vlan| named(vlan, "net")),
        foreign_network,
        far_ends: far_end
            .map(|device| FarEnd {
                device: named(device, "dev"),
                port: "ether1".into(),
                own_device: true,
            })
            .into_iter()
            .collect(),
    }
}

fn device(id: u32, uplink: Option<&str>, location: Option<u32>) -> ResidentDevice {
    ResidentDevice {
        id,
        name: format!("dev{id}").into(),
        routeros: true,
        uplink: uplink.map(Into::into),
        ports: Box::new([named(id * 10 + 1, "ether1"), named(id * 10 + 2, "ether2")]),
        location,
    }
}

/// Meier: WLAN 7 („Meier“, key, on) on VLAN 5; socket 20 with VLAN 5 and device 3 at its
/// ether2, socket 21 with the operator's network, socket 22 with none; device 4 linked
/// nowhere, device 6 at the operator's router.
fn before() -> Resident {
    Resident {
        id: 1,
        name: "Familie Meier".into(),
        ports: Box::new([
            port(20, Some(5), false, Some(3)),
            port(21, None, true, None),
            port(22, None, false, None),
        ]),
        networks: Box::new([ResidentNetwork {
            id: 5,
            name: "Meier".into(),
            vid: 105,
            wlans: Box::new([ResidentWlan {
                id: 7,
                ssid: "Meier".into(),
                enabled: true,
                psk: Some("old key 123456".into()),
                access_points: 2,
            }]),
        }]),
        devices: Box::new([
            device(3, Some("ether2"), Some(40)),
            device(4, None, None),
            device(6, Some("ether1"), Some(40)),
        ]),
        locations: Box::new([]),
    }
}

/// A WLAN gets back only what the change set: the SSID here, not key nor on/off.
#[test]
fn a_wlan_gets_back_what_the_change_set() {
    let change = ResidentChange::Wlan(WlanInput {
        wlan: 7,
        ssid: Some("Neu".into()),
        psk: None,
        enabled: None,
    });
    assert_eq!(
        &*undo_of(&before(), &[change]),
        [UndoStep::Wlan(WlanUndo {
            wlan: 7,
            ssid: Some("Meier".into()),
            psk: None,
            enabled: None,
        })]
    );
    let change = ResidentChange::Wlan(WlanInput {
        wlan: 7,
        ssid: None,
        psk: Some("new key 123456".into()),
        enabled: Some(false),
    });
    assert_eq!(
        &*undo_of(&before(), &[change]),
        [UndoStep::Wlan(WlanUndo {
            wlan: 7,
            ssid: None,
            psk: Some("old key 123456".into()),
            enabled: Some(true),
        })]
    );
}

/// The undo's debug output hides the key.
#[test]
fn the_debug_output_hides_the_key() {
    let undo = WlanUndo {
        wlan: 7,
        ssid: None,
        psk: Some("old key 123456".into()),
        enabled: None,
    };
    assert!(!format!("{undo:?}").contains("old key"));
}

/// A port gets its network or none back; one with the operator's network has no undo.
#[test]
fn a_port_gets_its_network_back() {
    let change = |port| {
        ResidentChange::PortVlan(PortVlanChange {
            port,
            kind: PortKind::Socket,
            vlan: Some(9),
        })
    };
    assert_eq!(
        &*undo_of(&before(), &[change(20), change(21), change(22)]),
        [
            UndoStep::PortVlan(PortVlanUndo {
                port: 20,
                kind: PortKind::Socket,
                vlan: Some(5),
            }),
            UndoStep::PortVlan(PortVlanUndo {
                port: 22,
                kind: PortKind::Socket,
                vlan: None,
            }),
        ]
    );
}

/// A device goes back to its port with its Anschluss, or off its link; one that was at the
/// operator's router has no undo.
#[test]
fn a_device_goes_back_to_its_link() {
    let connect = |device| {
        ResidentChange::ConnectDevice(ConnectDeviceChange {
            device,
            port: 22,
            kind: PortKind::Socket,
            device_port: None,
        })
    };
    assert_eq!(
        &*undo_of(
            &before(),
            &[connect(3), connect(4), ResidentChange::DisconnectDevice(6)]
        ),
        [
            UndoStep::ConnectDevice(ConnectDeviceUndo {
                device: 3,
                port: 20,
                kind: PortKind::Socket,
                device_port: Some(32),
            }),
            UndoStep::DisconnectDevice(DisconnectDeviceUndo { device: 4 }),
        ]
    );
}

/// A device goes back to its room; one without a room has no undo.
#[test]
fn a_device_goes_back_to_its_room() {
    let room = |device| {
        ResidentChange::DeviceRoom(DeviceRoomChange {
            device,
            location: 41,
        })
    };
    assert_eq!(
        &*undo_of(&before(), &[room(3), room(4)]),
        [UndoStep::DeviceRoom(DeviceRoomUndo {
            device: 3,
            location: 40,
        })]
    );
}

/// What isn't the resident's (another WLAN) has no undo.
#[test]
fn unknown_parts_have_no_undo() {
    let change = ResidentChange::Wlan(WlanInput {
        wlan: 8,
        ssid: Some("Neu".into()),
        psk: None,
        enabled: None,
    });
    assert!(undo_of(&before(), &[change]).is_empty());
}
