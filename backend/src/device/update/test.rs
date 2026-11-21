//! Scope: Tests of `update`; nothing else.
use super::{Architecture, DeviceState, UpdateStep, Version, downgrade_step, next_step};
use crate::Error;
use std::time::Duration;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}
fn state(version: &str, arch: Architecture, packages: &[&str]) -> DeviceState {
    DeviceState {
        version: v(version),
        architecture: arch,
        packages: names(packages),
        firmware: None,
        model: None,
        uptime: Duration::ZERO,
    }
}

fn names(names: &[&str]) -> Box<[Box<str>]> {
    names.iter().map(|&name| name.into()).collect()
}

#[test]
fn parses_and_orders_versions() {
    assert_eq!(v("7.15.3 (stable)").to_string(), "7.15.3");
    assert_eq!(v("7.25rc1").to_string(), "7.25rc1");
    assert_eq!(v("7.26beta1 1790861387").to_string(), "7.26beta1");
    assert!(v("7.24.5") < v("7.25beta2"));
    assert!(v("7.25beta2") < v("7.25rc1"));
    assert!(v("7.25rc1") < v("7.25"));
    assert!(Version::parse("7.x").is_none());
    assert!(v("7.9") < v("7.15.3"));
    assert_eq!(v("7.20"), v("7.20.0"));
    assert!(Version::parse("").is_none());
}

#[test]
fn plans_the_steps() {
    let target = v("7.20.8");
    let v6 = v("6.49.19");
    let wireless = names(&["wireless"]);
    assert_eq!(
        next_step(
            &state("7.15.3", Architecture::Arm, &["routeros", "wireless"]),
            &target,
            &v6,
            &wireless
        ),
        Some(UpdateStep {
            version: target.clone(),
            files: names(&["routeros-7.20.8-arm.npk", "wireless-7.20.8-arm.npk"]),
            uninstall: names(&[]),
        })
    );
    assert_eq!(
        next_step(
            &state("7.24.5", Architecture::X8664, &["routeros"]),
            &v("7.25"),
            &v6,
            &[]
        )
        .unwrap()
        .files,
        names(&["routeros-7.25.npk"])
    );
    assert_eq!(
        next_step(
            &state("7.20.8", Architecture::Arm, &["routeros"]),
            &target,
            &v6,
            &[]
        ),
        None
    );
    // newer than the channel: no downgrade
    assert_eq!(
        next_step(
            &state("7.21", Architecture::Arm, &["routeros"]),
            &target,
            &v6,
            &[]
        ),
        None
    );
    let sxt = ["system", "wireless", "routing", "dhcp", "security"];
    assert_eq!(
        next_step(
            &state("6.45.1", Architecture::Mipsbe, &sxt),
            &target,
            &v6,
            &wireless
        )
        .unwrap()
        .files,
        names(&["routeros-mipsbe-6.49.19.npk"])
    );
    assert_eq!(
        next_step(
            &state("6.49.19", Architecture::Mipsbe, &sxt),
            &target,
            &v6,
            &wireless
        )
        .unwrap()
        .files,
        names(&["routeros-7.20.8-mipsbe.npk", "wireless-7.20.8-mipsbe.npk"])
    );
}

#[test]
fn brings_the_packages_in_line() {
    let target = v("7.20.8");
    let v6 = v("6.49.19");
    let wifi = names(&["wifi-qcom-ac"]);
    // an unused radio package goes with the update
    assert_eq!(
        next_step(
            &state("7.18.2", Architecture::Arm, &["routeros", "wifi-qcom-ac"]),
            &target,
            &v6,
            &[]
        ),
        Some(UpdateStep {
            version: target.clone(),
            files: names(&["routeros-7.20.8-arm.npk"]),
            uninstall: names(&["wifi-qcom-ac"]),
        })
    );
    // at the version already: only the missing package, and what isn't needed goes
    assert_eq!(
        next_step(
            &state("7.20.8", Architecture::Arm, &["routeros", "wireless"]),
            &target,
            &v6,
            &wifi
        ),
        Some(UpdateStep {
            version: target.clone(),
            files: names(&["wifi-qcom-ac-7.20.8-arm.npk"]),
            uninstall: names(&["wireless"]),
        })
    );
    assert_eq!(
        next_step(
            &state("7.20.8", Architecture::Arm, &["routeros", "wifi-qcom-ac"]),
            &target,
            &v6,
            &wifi
        ),
        None
    );
}

#[test]
fn a_downgrade_takes_the_packages_the_device_runs() {
    assert_eq!(
        downgrade_step(
            &state("7.25", Architecture::Arm, &["routeros", "wifi-qcom-ac"]),
            &v("7.23.8")
        )
        .unwrap(),
        UpdateStep {
            version: v("7.23.8"),
            files: names(&["routeros-7.23.8-arm.npk", "wifi-qcom-ac-7.23.8-arm.npk"]),
            uninstall: names(&[]),
        }
    );
    assert_eq!(
        downgrade_step(
            &state("7.24.5", Architecture::X8664, &["routeros"]),
            &v("7.23.8")
        )
        .unwrap()
        .files,
        names(&["routeros-7.23.8.npk"])
    );
    // only to an older RouterOS 7
    for (running, target) in [
        ("7.23.8", "7.23.8"),
        ("7.23.8", "7.25"),
        ("7.20.8", "6.49.19"),
        ("6.49.18", "6.48"),
    ] {
        assert!(
            matches!(
                downgrade_step(
                    &state(running, Architecture::Arm, &["routeros"]),
                    &v(target)
                ),
                Err(Error::NoDowngrade { .. })
            ),
            "{running} to {target}"
        );
    }
}
