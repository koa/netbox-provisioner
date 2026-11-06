use super::*;

fn row(fields: &[(&str, &str)]) -> Row {
    fields
        .iter()
        .map(|(k, v)| ((*k).into(), Some((*v).into())))
        .collect()
}

/// As lab-core (CCR2004) reports it, with one PSU unplugged.
#[test]
fn health_rows_keep_name_value_and_unit() {
    let health = [
        row(&[("name", "cpu-temperature"), ("value", "45"), ("type", "C")]),
        row(&[("name", "psu1-state"), ("value", "fail")]),
    ];
    let data = from_rows(&health, &[]);
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
fn resources_take_the_first_row() {
    let resources = [row(&[
        ("uptime", "1d02:03:04"),
        ("cpu-load", "3"),
        ("free-memory", "58720256"),
        ("total-memory", "134217728"),
        ("free-hdd-space", "16777216"),
        ("version", "7.23.8 (stable)"),
    ])];
    let data = from_rows(&[], &resources);
    let resources = data.resources.expect("a resource row");
    assert_eq!(&*resources.uptime, "1d02:03:04");
    assert_eq!(resources.cpu_load, 3);
    assert_eq!(resources.free_memory, 58720256);
    assert_eq!(resources.total_memory, 134217728);
    assert_eq!(resources.free_hdd_space, 16777216);
    assert_eq!(&*resources.version, "7.23.8 (stable)");
}

#[test]
fn no_resource_row_is_none() {
    assert!(from_rows(&[], &[]).resources.is_none());
}
