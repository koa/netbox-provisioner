use super::{Instrument, Progress, instrument};
use crate::device::ros::fallback::Rearmed;
use std::time::Duration;

const SCRIPT: &str = "/interface/list\nadd name=a\nadd name=b\n/ip/address\nadd address=10.0.0.1/24 interface=a\n";

fn how(probe: bool, rearm: bool) -> Instrument<'static> {
    Instrument {
        file: "flash/netbox-provisioner-progress.txt",
        run: "r1",
        lead: Duration::from_secs(1),
        probe: probe.then(|| "10.0.2.2".parse().unwrap()),
        rearm: rearm.then_some(Duration::from_secs(90)),
        fallback: "netbox-provisioner-fallback",
    }
}

#[test]
fn the_script_lines_stay_in_order_with_their_marks() {
    let script = instrument(SCRIPT, &how(true, true));
    let kept: Vec<&str> = script
        .lines()
        .filter(|line| SCRIPT.lines().any(|original| original == *line))
        .collect();
    assert_eq!(kept, SCRIPT.lines().collect::<Vec<_>>());
    assert!(script.contains("$mark \"section 4\"\n/ip/address\n"), "{script}");
    assert!(script.contains(":set ln 5\nadd address=10.0.0.1/24 interface=a\n"), "{script}");
    assert!(script.contains("$probe \"lost 5\""), "{script}");
    assert!(script.contains("interval=90s"), "{script}");
    assert!(script.contains("/file add name=\"flash/netbox-provisioner-progress.txt\""), "{script}");
}

#[test]
fn without_probe_or_fallback_nothing_pings_or_rearms() {
    let script = instrument(SCRIPT, &how(false, false));
    assert!(!script.contains("/ping"), "{script}");
    assert!(!script.contains("scheduler"), "{script}");
}

#[test]
fn marks_of_a_run_that_lost_the_provisioner() {
    let text = "10:00:01 start 5 r1\n10:00:01 ping 10.0.2.2\n10:00:02 section 1\n\
                10:00:02 section 4\n10:00:03 lost 5\n10:00:04 done\n10:00:04 rearmed 90 1w1d11:21:27\n";
    let progress = Progress::parse(text, "r1").unwrap();
    assert_eq!(progress.lines, 5);
    assert_eq!(progress.section, Some(4));
    assert_eq!(progress.reach, [(5, true)]);
    assert_eq!(progress.probe, Some(("10.0.2.2".into(), true)));
    assert!(progress.done && progress.finished());
    assert_eq!(progress.rearmed, Some(90));
    let at = Duration::from_secs(((8 * 24 + 11) * 60 + 21) * 60 + 27);
    assert_eq!(progress.rearm(), Some(Rearmed { at, window: Duration::from_secs(90) }));
}

/// The uptime as a script writes it, which the fallback's time counts from.
#[test]
fn uptimes_of_a_script() {
    let uptime = |text| super::script_uptime(text).map(|d| d.as_secs());
    assert_eq!(uptime("00:12:34"), Some(754));
    assert_eq!(uptime("2d00:00:05"), Some(2 * 86400 + 5));
    assert_eq!(uptime("1w00:00:00"), Some(7 * 86400));
    assert_eq!(uptime("12:34"), None);
}

#[test]
fn an_error_keeps_routeros_message_without_its_own_line() {
    let text = "10:00:01 start 5 r1\n10:00:02 section 1\n10:00:02 error 3 failure: already have \
                interface list with such name (/interface/list/add; line 21)\n";
    let progress = Progress::parse(text, "r1").unwrap();
    assert!(progress.finished() && !progress.done);
    assert_eq!(
        progress.error,
        Some((3, "failure: already have interface list with such name (/interface/list/add)".into()))
    );
}

#[test]
fn marks_of_another_run_are_not_taken() {
    assert_eq!(Progress::parse("10:00:01 start 5 r0\n10:00:02 done\n", "r1"), None);
    assert_eq!(Progress::parse("", "r1"), None);
}
