//! Scope: Tests of `components::resident::status`; nothing else.
use super::{State, Status};
use crate::graphql::resident::{
    changes::{
        ChangeDevice, ChangeState, HoldReason, RolloutState, RoundDevice, Touched, TouchedPort,
    },
    live::{DeviceLive, LiveState, PortLive, SeenUplink, WlanLive},
    residents::{
        FarEnd, Named, PortKind, Resident, ResidentDevice, ResidentNetwork, ResidentPort,
        ResidentWlan,
    },
};
use i18n::resident::StatusLabel;

fn named(id: i32, name: &str) -> Named {
    Named {
        id,
        name: name.into(),
    }
}

fn device(id: i32, name: &str) -> ResidentDevice {
    ResidentDevice {
        id,
        name: name.into(),
        routeros: true,
        uplink: Some("ether1".into()),
        ports: Vec::new(),
        location: None,
    }
}

fn resident(ports: Vec<ResidentPort>, devices: Vec<ResidentDevice>) -> Resident {
    Resident {
        id: 1,
        name: "Familie Meier".into(),
        ports,
        networks: Vec::new(),
        devices,
        locations: Vec::new(),
    }
}

fn socket(id: i32, network: Option<Named>, far_ends: Vec<FarEnd>) -> ResidentPort {
    ResidentPort {
        id,
        kind: PortKind::Socket,
        name: "1".into(),
        label: Some("Wohnzimmer".into()),
        device: named(50, "wohnung-meier"),
        site: None,
        location: Vec::new(),
        on_own_device: false,
        network,
        foreign_network: false,
        far_ends,
        routeros: true,
    }
}

fn at(device: i32, name: &str, state: RolloutState) -> ChangeDevice {
    ChangeDevice {
        device: RoundDevice {
            id: device,
            name: name.into(),
        },
        state,
        reasons: Vec::new(),
    }
}

fn change(devices: Vec<ChangeDevice>, touched: Touched) -> ChangeState {
    ChangeState {
        id: 1,
        by: "Hans Meier".into(),
        age: 30,
        devices,
        undoable: true,
        touched,
    }
}

fn touched_socket(id: i32) -> Touched {
    Touched {
        ports: vec![TouchedPort {
            kind: PortKind::Socket,
            id,
        }],
        wlans: Vec::new(),
        devices: Vec::new(),
    }
}

fn live(devices: Vec<DeviceLive>, ports: Vec<PortLive>, wlans: Vec<WlanLive>) -> LiveState {
    LiveState {
        known: true,
        devices,
        ports,
        wlans,
    }
}

fn label(status: Option<Status>) -> Option<StatusLabel> {
    status.map(|status| status.label)
}

/// A socket's change on its way wins over what is up; once through, the live state decides.
#[test]
fn a_socket_shows_its_rollout_then_its_live_state() {
    let port = socket(2, Some(named(1, "lab-wlan")), Vec::new());
    let up = live(
        Vec::new(),
        vec![PortLive {
            kind: PortKind::Socket,
            id: 2,
            switch: named(10, "lab-crs125"),
            reached: Some(true),
            link: None,
        }],
        Vec::new(),
    );
    let rolling = [change(
        vec![
            at(10, "lab-crs125", RolloutState::Pending),
            at(11, "lab-np2", RolloutState::RolledOut),
        ],
        touched_socket(2),
    )];
    let state = State {
        changes: &rolling,
        live: Some(&up),
    };
    let status = state.port(&port, None).expect("a label");
    assert_eq!(status.label, StatusLabel::Rolling);
    assert_eq!(
        status.text.as_deref(),
        Some("Wird ausgerollt: 1 von 2 Geräten.")
    );
    let through = [change(
        vec![at(10, "lab-crs125", RolloutState::RolledOut)],
        touched_socket(2),
    )];
    let state = State {
        changes: &through,
        live: Some(&up),
    };
    assert_eq!(label(state.port(&port, None)), Some(StatusLabel::Ok));
}

/// A port whose own device the routers don't reach says so; a free one without network
/// is off; one with a network has no label where nothing behind it is judged.
#[test]
fn a_port_is_down_with_its_device() {
    let wap = device(7, "lab-wap");
    let port = socket(
        2,
        None,
        vec![FarEnd {
            device: named(7, "lab-wap"),
            port: "ether1".into(),
            own_device: true,
        }],
    );
    let down = live(
        vec![DeviceLive {
            device: named(7, "lab-wap"),
            reached: false,
        }],
        Vec::new(),
        Vec::new(),
    );
    let state = State {
        changes: &[],
        live: Some(&down),
    };
    let status = state.port(&port, Some(&wap)).expect("a label");
    assert_eq!(status.label, StatusLabel::Down);
    assert_eq!(status.text.as_deref(), Some("lab-wap antwortet nicht."));
    let free = socket(3, None, Vec::new());
    assert_eq!(label(state.port(&free, None)), Some(StatusLabel::Off));
    // its switch without OSPF of its own: not judged, no label
    let unjudged = socket(5, Some(named(1, "lab-wlan")), Vec::new());
    assert_eq!(label(state.port(&unjudged, None)), None);
    let unknown = State {
        changes: &[],
        live: None,
    };
    let with_network = socket(4, Some(named(1, "lab-wlan")), Vec::new());
    assert_eq!(label(unknown.port(&with_network, None)), None);
}

/// A device shows its own state in a change to a network; one held for an admin says why;
/// one not reached behind another not reached says so.
#[test]
fn a_device_shows_its_own_rollout_and_what_it_hangs_at() {
    let wap = device(7, "lab-wap");
    let mut held = at(7, "lab-wap", RolloutState::Waiting);
    held.reasons = vec![HoldReason::CapOff];
    let changes = [change(
        vec![held, at(11, "lab-np2", RolloutState::RolledOut)],
        Touched {
            ports: Vec::new(),
            wlans: vec![1],
            devices: Vec::new(),
        },
    )];
    let state = State {
        changes: &changes,
        live: None,
    };
    let status = state.device(&wap, None, None).expect("a label");
    assert_eq!(status.label, StatusLabel::Waiting);
    assert_eq!(
        status.text.as_deref(),
        Some("Wartet bei lab-wap auf einen Admin: seine Radios senden danach kein WLAN.")
    );
    let switch = device(8, "switch-flur");
    let down = live(
        vec![
            DeviceLive {
                device: named(7, "lab-wap"),
                reached: false,
            },
            DeviceLive {
                device: named(8, "switch-flur"),
                reached: false,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    let state = State {
        changes: &[],
        live: Some(&down),
    };
    let status = state.device(&wap, Some(&switch), None).expect("a label");
    assert_eq!(status.label, StatusLabel::Down);
    assert_eq!(
        status.text.as_deref(),
        Some("Nicht erreichbar, weil switch-flur nicht erreichbar ist.")
    );
}

/// A cabled port without link is down, and so says its device and the summary; with link
/// and its device silent, the port says the cable is fine.
#[test]
fn a_cabled_port_without_link() {
    let wap = device(7, "lab-wap");
    let port = socket(
        2,
        None,
        vec![FarEnd {
            device: named(7, "lab-wap"),
            port: "ether1".into(),
            own_device: true,
        }],
    );
    let state_of = |link| {
        live(
            vec![DeviceLive {
                device: named(7, "lab-wap"),
                reached: false,
            }],
            vec![PortLive {
                kind: PortKind::Socket,
                id: 2,
                switch: named(10, "lab-crs125"),
                reached: Some(true),
                link: Some(link),
            }],
            Vec::new(),
        )
    };
    let unplugged = state_of(false);
    let state = State {
        changes: &[],
        live: Some(&unplugged),
    };
    let status = state.port(&port, Some(&wap)).expect("a label");
    assert_eq!(status.label, StatusLabel::Down);
    assert_eq!(
        status.text.as_deref(),
        Some("Kein Link. Steckt das Kabel an der Dose und am Gerät?")
    );
    let status = state.device(&wap, None, Some(&port)).expect("a label");
    assert_eq!(
        status.text.as_deref(),
        Some("Nicht erreichbar: Wohnzimmer hat keinen Link.")
    );
    let issues = state
        .issues(&resident(vec![port.clone()], vec![wap.clone()]), &[])
        .into_iter()
        .map(|issue| issue.text)
        .collect::<Vec<_>>();
    assert_eq!(
        issues,
        [
            "lab-wap ist nicht erreichbar.",
            "Wohnzimmer hat keinen Link: steckt das Kabel?"
        ]
    );
    let plugged = state_of(true);
    let state = State {
        changes: &[],
        live: Some(&plugged),
    };
    assert_eq!(
        state.port(&port, Some(&wap)).and_then(|status| status.text),
        Some("Das Kabel hat Link, aber lab-wap antwortet nicht.".to_string())
    );
}

/// A WLAN on part of its access points is a problem, on none down; switched off, off.
#[test]
fn a_network_by_its_access_points() {
    let wlan = |enabled: bool| ResidentNetwork {
        id: 1,
        name: "lab-wlan".into(),
        wlans: vec![ResidentWlan {
            id: 1,
            ssid: "lab-wlan".into(),
            enabled,
            psk: None,
            access_points: 2,
        }],
    };
    let some = live(
        Vec::new(),
        Vec::new(),
        vec![WlanLive {
            wlan: 1,
            access_points: 2,
            unreached: vec![named(7, "lab-wap")],
        }],
    );
    let state = State {
        changes: &[],
        live: Some(&some),
    };
    let status = state.network(&wlan(true)).expect("a label");
    assert_eq!(status.label, StatusLabel::Limited);
    assert_eq!(
        status.text.as_deref(),
        Some("WLAN nur auf 1 von 2 Access Points; lab-wap nicht erreichbar.")
    );
    assert_eq!(label(state.network(&wlan(false))), Some(StatusLabel::Off));
}

/// A change waiting for an admin at a manual device says so, not that the other device,
/// deferred behind it, isn't reached (lab-np2 manual, lab-wap deferred).
#[test]
fn a_change_waiting_for_an_admin_says_so_first() {
    let network = ResidentNetwork {
        id: 2,
        name: "lab-gast".into(),
        wlans: vec![ResidentWlan {
            id: 2,
            ssid: "lab-gast".into(),
            enabled: true,
            psk: None,
            access_points: 1,
        }],
    };
    let changes = [change(
        vec![
            at(3, "lab-np2", RolloutState::Manual),
            at(6, "lab-wap", RolloutState::Deferred),
        ],
        Touched {
            ports: Vec::new(),
            wlans: vec![2],
            devices: Vec::new(),
        },
    )];
    let state = State {
        changes: &changes,
        live: None,
    };
    let status = state.network(&network).expect("a label");
    assert_eq!(status.label, StatusLabel::Waiting);
    assert_eq!(
        status.text.as_deref(),
        Some("Wartet bei lab-np2 auf einen Admin.")
    );
}

/// A change held at a radio link tells the resident it is complex and needs support (Z58).
#[test]
fn a_radio_link_needs_support() {
    let sxt = device(9, "lab-sxt-a");
    let mut held = at(9, "lab-sxt-a", RolloutState::Waiting);
    held.reasons = vec![HoldReason::RadioLink];
    let changes = [change(
        vec![held],
        Touched {
            ports: Vec::new(),
            wlans: Vec::new(),
            devices: vec![9],
        },
    )];
    let state = State {
        changes: &changes,
        live: None,
    };
    let status = state.device(&sxt, None, None).expect("a label");
    assert_eq!(status.label, StatusLabel::Waiting);
    assert_eq!(
        status.text.as_deref(),
        Some(
            "Wartet bei lab-sxt-a auf einen Admin: eine komplexe Anpassung an einer \
             Richtfunk-Verbindung, die nicht automatisch ausgerollt werden kann; der Support \
             übernimmt sie."
        )
    );
}

#[test]
fn a_cable_seen_elsewhere_is_in_the_summary() {
    let wap = device(7, "lab-wap");
    let state = State {
        changes: &[],
        live: None,
    };
    let seen = [SeenUplink {
        device: 7,
        port: named(3, "ether2"),
    }];
    let issues = state
        .issues(&resident(Vec::new(), vec![wap]), &seen)
        .into_iter()
        .map(|issue| issue.text)
        .collect::<Vec<_>>();
    assert_eq!(
        issues,
        ["Das Kabel von lab-wap steckt an ether2, nicht an ether1."]
    );
}
