//! `/system/logging`: RouterOS's own actions and rules, and with `device_syslog` set the
//! device's log to a syslog server in CEF, where Loki parses it.
use super::{BaseDeviceDataCurrent, BaseDeviceDataTarget};
use crate::config::DeviceSyslog;
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        SystemLoggingActionByName, SystemLoggingActionCfg, SystemLoggingActionRemoteLogFormat,
        SystemLoggingActionRemoteProtocol, SystemLoggingActionTarget, SystemLoggingCfg,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr},
};

const REMOTE: &[u8] = b"remote";
/// the target's actions by name
type Actions = BTreeMap<AsciiString, SystemLoggingActionByName>;
/// the target's rules by action and topics
type Rules = BTreeMap<RuleKey, SystemLoggingCfg>;
type RuleKey = (AsciiString, BTreeSet<AsciiString>);
/// what goes to the server without `topics`, as the devices had it before the provisioner
pub const DEFAULT_TOPICS: [&str; 4] = ["info", "warning", "error", "critical"];
const DEFAULT_PORT: u16 = 514;

/// RouterOS's actions, which it doesn't let go (`memory`, `disk`, `echo`, `remote`), and its
/// rules, which it doesn't let go either: a target without them would remove them. The
/// `remote` action stays in CEF even without a server (`remote=0.0.0.0` and no rule sends to
/// it): RouterOS hides `syslog-time-format` in the default format and refuses to clear it
/// (`=""`, `!syslog-time-format`, CHR 7.24.5), so the way back from CEF would fail.
pub(super) fn routeros_logging() -> (Actions, Rules) {
    let action = |name: &[u8], target| {
        (
            AsciiString::from(name),
            SystemLoggingActionByName(SystemLoggingActionCfg {
                name: name.into(),
                target,
                remote: None,
                remote_port: None,
                src_address: None,
                remote_log_format: None,
                remote_protocol: None,
                cef_event_delimiter: None,
                syslog_time_format: None,
                vrf: None,
            }),
        )
    };
    let actions = [
        action(b"memory", SystemLoggingActionTarget::Memory),
        action(b"disk", SystemLoggingActionTarget::Disk),
        action(b"echo", SystemLoggingActionTarget::Echo),
        (REMOTE.into(), SystemLoggingActionByName(remote_action(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_PORT))),
    ]
    .into();
    let rules = [
        (b"memory".as_slice(), "info"),
        (b"memory", "error"),
        (b"memory", "warning"),
        (b"echo", "critical"),
    ]
    .into_iter()
    .map(|(action, topic)| rule(action, [topic]))
    .collect();
    (actions, rules)
}

/// `current` as the diff sees it: an action not yet in CEF hides its delimiter, which then is
/// RouterOS's `\r\n` once the target switches it, so the target's `""` goes along in the same
/// `set` instead of a second apply.
pub(super) fn after_switch_to_cef(current: &mut BaseDeviceDataCurrent) {
    for action in current.logging_action.iter_mut() {
        let action = &mut action.0;
        if action.target == SystemLoggingActionTarget::Remote
            && action.remote_log_format != Some(SystemLoggingActionRemoteLogFormat::Cef)
        {
            action.cef_event_delimiter = Some(b"\r\n".as_slice().into());
        }
    }
}

fn remote_action(remote: IpAddr, port: u16) -> SystemLoggingActionCfg {
    SystemLoggingActionCfg {
        name: REMOTE.into(),
        target: SystemLoggingActionTarget::Remote,
        remote: Some(remote),
        remote_port: Some(port),
        src_address: Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        remote_log_format: Some(SystemLoggingActionRemoteLogFormat::Cef),
        remote_protocol: Some(SystemLoggingActionRemoteProtocol::Udp),
        // none: `=""`, one event per message instead of RouterOS's `\r\n` between them
        cef_event_delimiter: None,
        syslog_time_format: Some(b"iso8601".into()),
        vrf: Some(b"main".into()),
    }
}

fn rule<'t>(
    action: &[u8],
    topics: impl IntoIterator<Item = &'t str>,
) -> (RuleKey, SystemLoggingCfg) {
    let topics = topics.into_iter().map(|t| AsciiString::from(t.as_bytes())).collect::<BTreeSet<_>>();
    (
        (action.into(), topics.clone()),
        SystemLoggingCfg {
            action: action.into(),
            topics,
            prefix: None,
            regex: None,
            disabled: false,
        },
    )
}

impl BaseDeviceDataTarget {
    /// Sends the device's log to `syslog`: the `remote` action to its server, one rule per
    /// topic.
    pub fn setup_syslog(&mut self, syslog: &DeviceSyslog) {
        self.logging_action.insert(
            REMOTE.into(),
            SystemLoggingActionByName(remote_action(syslog.remote, syslog.port.unwrap_or(DEFAULT_PORT))),
        );
        for topic in syslog.topics() {
            let (key, rule) = rule(REMOTE, [topic]);
            self.logging.insert(key, rule);
        }
    }
}
