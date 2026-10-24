//! `/system/logging` (`device::ros::syslog`): RouterOS's own actions and rules stay, the
//! `remote` action goes to CEF, and with `device_syslog` the device logs to the server.
use crate::{
    config::DeviceSyslog,
    device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, test::empty_current},
};
use mikrotik_model::{
    generator::Generator,
    model::{SystemLoggingActionRemoteLogFormat, SystemLoggingById, SystemLoggingCfg},
    resource::ResourceMutation,
    value::Id,
};

/// The script lines of the logging menus `target` sends to `current`.
fn logging_script(target: &BaseDeviceDataTarget, current: &BaseDeviceDataCurrent) -> String {
    let mut current = current.clone();
    crate::device::ros::syslog::after_switch_to_cef(&mut current);
    let mutations = target.generate_mutations(&current).unwrap();
    let mutations = mutations
        .iter()
        .filter(|m| m.resource.starts_with(b"system/logging"))
        .cloned()
        .collect::<Vec<ResourceMutation>>();
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in mutations.iter() {
        generator.append_mutation(mutation).unwrap();
    }
    script
}

fn target() -> BaseDeviceDataTarget {
    BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap()
}

fn loki() -> DeviceSyslog {
    DeviceSyslog {
        remote: "2a02:16a:fe00:1a0::10:8".parse().unwrap(),
        port: None,
        topics: None,
    }
}

/// Without a server only the `remote` action changes, to CEF (it stays there, see
/// `routeros_logging`); RouterOS's rules stay, none is added.
#[test]
fn a_reset_device_without_server_keeps_its_rules() {
    let target = target();
    let script = logging_script(&target, &empty_current(&target));
    assert!(script.contains("remote-log-format=cef"), "{script}");
    assert!(script.contains("syslog-time-format=iso8601"), "{script}");
    // RouterOS's `\r\n` once in CEF, set in the same line
    assert!(script.contains(r#"cef-event-delimiter="""#), "{script}");
    assert!(!script.contains("add"), "{script}");
    assert!(!script.contains("remove"), "{script}");
}

/// With the server: the `remote` action to it and a rule per topic, RouterOS's rules stay.
#[test]
fn a_reset_device_logs_to_the_server() {
    let mut target = target();
    target.setup_syslog(&loki());
    let script = logging_script(&target, &empty_current(&target));
    assert!(script.contains(r#"remote="2a02:16a:fe00:1a0::10:8""#), "{script}");
    for topic in ["info", "warning", "error", "critical"] {
        assert!(script.contains(&format!("add action=remote topics={topic} ")), "{topic}: {script}");
    }
    assert_eq!(script.matches("add ").count(), 4, "{script}");
    assert!(!script.contains("remove"), "{script}");
}

/// A device set up by hand as in `examples/` (CEF to the server, a rule per topic, rules of
/// its own besides): only the rules NetBox doesn't know of go.
#[test]
fn a_device_set_up_by_hand_loses_only_its_own_rules() {
    let mut target = target();
    target.setup_syslog(&loki());
    let mut current = empty_current(&target);
    current.logging_action = target.logging_action.values().cloned().collect();
    let mut rules = current.logging.to_vec();
    for (action, topics, disabled) in [
        ("remote", "info", false),
        ("remote", "warning", false),
        ("remote", "error", false),
        ("remote", "critical", false),
        ("memory", "dhcp", true),
        ("memory", "poe-out", false),
    ] {
        rules.push(SystemLoggingById {
            id: Id(rules.len() as u32 + 1),
            data: SystemLoggingCfg {
                action: action.as_bytes().into(),
                topics: [topics.as_bytes().into()].into(),
                prefix: None,
                regex: None,
                disabled,
            },
        });
    }
    current.logging = rules.into();
    let script = logging_script(&target, &current);
    assert_eq!(script.matches("remove").count(), 2, "{script}");
    assert!(!script.contains("add "), "{script}");
    assert!(!script.contains("set "), "{script}");
}

/// RouterOS hides the CEF fields in the default format: the fixture of a reset device has
/// none, so the target's `remote` action can't stay in it unnoticed.
#[test]
fn the_reset_remote_action_is_in_the_default_format() {
    let target = target();
    let current = empty_current(&target);
    let remote = current.logging_action.iter().find(|a| a.0.name.0.as_ref() == b"remote").unwrap();
    assert_eq!(remote.0.remote_log_format, Some(SystemLoggingActionRemoteLogFormat::Default));
}
