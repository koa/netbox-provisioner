//! Scope: Tests of `network_state::device_data`; nothing else.
use super::*;
use crate::device::answer::parsed as row;

/// As lab-core (CCR2004) reports it, with one PSU unplugged.
#[test]
fn health_rows_keep_name_value_and_unit() {
    let health: [SystemHealthState; 2] = [
        row(&[("name", "cpu-temperature"), ("value", "45"), ("type", "C")]),
        row(&[("name", "psu1-state"), ("value", "fail")]),
    ];
    let data = from_states(&health, None);
    assert_eq!(data.health.len(), 2);
    assert_eq!(&*data.health[0].name, "cpu-temperature");
    assert_eq!(&*data.health[0].value, "45");
    assert_eq!(data.health[0].unit.as_deref(), Some("C"));
    assert_eq!(&*data.health[1].name, "psu1-state");
    assert_eq!(
        data.health[1].unit, None,
        "no type: a state, not a measurement"
    );
}

#[test]
fn resources_take_their_fields() {
    let resources: SystemResourceState = row(&[
        ("uptime", "1d2h3m4s"),
        ("cpu-load", "3"),
        ("free-memory", "58720256"),
        ("total-memory", "134217728"),
        ("free-hdd-space", "16777216"),
        ("version", "7.23.8 (stable)"),
        ("architecture-name", "arm64"),
    ]);
    let data = from_states(&[], Some(&resources));
    let resources = data.resources.expect("a resource row");
    assert_eq!(resources.uptime, Duration::from_secs(93784));
    assert_eq!(resources.cpu_load, 3);
    assert_eq!(resources.free_memory, 58720256);
    assert_eq!(resources.total_memory, 134217728);
    assert_eq!(resources.free_hdd_space, 16777216);
    assert_eq!(&*resources.version, "7.23.8 (stable)");
}

#[test]
fn no_resource_row_is_none() {
    assert!(from_states(&[], None).resources.is_none());
}
