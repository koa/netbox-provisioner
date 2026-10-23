//! Marks the apply script leaves on the device as it goes, in a file (`/file`), which outlasts
//! the reboot of a fallback: how far it got, the line it stopped at and why, and after which
//! line the provisioner stopped answering a ping from the device. The provisioner reads it
//! while it waits, so it learns of the script's end without its connection, and after a
//! fallback, to show where the apply cut the device off.
//!
//! The script also brings the fallback forward once it is through (`rearm`): from its end the
//! provisioner has the fallback's window to reach the device, instead of a delay counted from
//! the start that has to cover a script of any length.
//!
//! Checked on a CHR (7.24.5): a menu line (`/ip/address`) inside `:onerror … in={}` keeps its
//! context across the script's own `:set`/`:if` lines, `$e` holds RouterOS' message, `/ping
//! … count=1` gives the number of replies, and `/file set … contents=` takes text.
use crate::{
    Error,
    device::{
        raw::{field, rows},
        ros::fallback::Rearmed,
    },
};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{collections::HashMap, fmt::Write, net::IpAddr, time::Duration};

/// The file the marks go to (`.txt` keeps RouterOS from taking it as anything else).
pub const FILE: &str = "netbox-provisioner-progress.txt";

/// Where [`FILE`] outlasts a reboot: under `flash/` where the device has that disk, its root is
/// in RAM then (lab-w60-a, a wAP 60G, 7.23.8: the marks were gone after the fallback's reboot,
/// only `flash/…` was left); in the root elsewhere (a CHR, newer RouterBOARDs).
pub async fn file_path(raw: &RawDevice<SimpleResult>) -> Result<String, Error> {
    let flash = rows(raw, "/file/print")
        .await?
        .iter()
        .any(|row| field(row, "name") == Some("flash") && field(row, "type") == Some("disk"));
    Ok(if flash { format!("flash/{FILE}") } else { FILE.into() })
}

/// What [`instrument`] adds to a script.
pub struct Instrument<'a> {
    /// the file of the marks ([`file_path`])
    pub file: &'a str,
    /// names this run in the file, so a file left from an apply before isn't taken for it
    pub run: &'a str,
    /// how long the script waits before it changes anything
    pub lead: Duration,
    /// the provisioner's address as the device sees it: pinged after each line, until it
    /// doesn't answer, and then before each menu, until it answers again
    pub probe: Option<IpAddr>,
    /// the fallback's new delay from the script's end, or after its error; `None` without a
    /// fallback
    pub rearm: Option<Duration>,
    /// the scheduler entry of the fallback
    pub fallback: &'a str,
}

/// `script` (a generated one: a menu line, then a command per line) with the marks of `how`.
/// Lines are counted from 1 as `script` has them, menu lines included.
pub fn instrument(script: &str, how: &Instrument) -> String {
    let mut out = String::new();
    let file = how.file;
    let append = format!(
        ":do {{ /file set \"{file}\" contents=([/file get \"{file}\" contents] . [/system clock get time] . \" \" . $1 . \"\\n\") }} on-error={{}}"
    );
    let lines = script.lines().count();
    let _ = writeln!(out, ":local mark do={{ {append} }}");
    if let Some(address) = how.probe {
        let _ = writeln!(
            out,
            ":local probe do={{ :if ([/ping {address} count=1] > 0) do={{ :return \"ok\" }}; {append}; :return \"lost\" }}"
        );
        let _ = writeln!(
            out,
            ":local again do={{ :if ([/ping {address} count=1] = 0) do={{ :return \"lost\" }}; {append}; :return \"ok\" }}"
        );
    }
    let _ = writeln!(out, ":local ln 0");
    let _ = writeln!(out, ":local reach \"off\"");
    let _ = writeln!(out, ":do {{ /file remove [find name=\"{file}\"] }} on-error={{}}");
    let _ = writeln!(out, ":do {{ /file add name=\"{file}\" contents=\"\" }} on-error={{}}");
    let _ = writeln!(out, "$mark \"start {lines} {}\"", how.run);
    if let Some(address) = how.probe {
        let _ = writeln!(
            out,
            ":if ([/ping {address} count=1] > 0) do={{ :set reach \"ok\"; $mark \"ping {address}\" }} else={{ $mark \"noping {address}\" }}"
        );
    }
    let _ = writeln!(out, ":delay {}s", how.lead.as_secs().max(1));
    let back = |out: &mut String| {
        if how.probe.is_some() {
            let _ = writeln!(
                out,
                ":if ($reach = \"lost\") do={{ :set reach [$again (\"back \" . $ln)] }}"
            );
        }
    };
    let _ = writeln!(out, ":onerror e in={{");
    for (index, line) in script.lines().enumerate() {
        let number = index + 1;
        if line.starts_with('/') {
            back(&mut out);
            let _ = writeln!(out, "$mark \"section {number}\"");
            let _ = writeln!(out, "{line}");
        } else {
            let _ = writeln!(out, ":set ln {number}");
            let _ = writeln!(out, "{line}");
            if how.probe.is_some() {
                let _ = writeln!(
                    out,
                    ":if ($reach = \"ok\") do={{ :set reach [$probe \"lost {number}\"] }}"
                );
            }
        }
    }
    // a ping lags behind a change (a drop rule removed answers the ping a second later), so
    // the end looks a few times
    if how.probe.is_some() {
        let _ = writeln!(
            out,
            ":for i from=1 to=3 do={{ :if ($reach = \"lost\") do={{ :delay 1s; :set reach [$again (\"back \" . $ln)] }} }}"
        );
    }
    let _ = writeln!(out, "$mark \"done\"");
    let _ = writeln!(out, "}} do={{ $mark (\"error \" . $ln . \" \" . $e) }}");
    if let Some(window) = how.rearm {
        let name = how.fallback;
        let seconds = window.as_secs().max(1);
        let _ = writeln!(
            out,
            ":do {{ :local s [/system scheduler find name=\"{name}\"]; :if ([:len $s] > 0) do={{ \
             :local ev [/system scheduler get $s on-event]; :local c [/system scheduler get $s comment]; \
             /system scheduler remove $s; \
             /system scheduler add name=\"{name}\" interval={seconds}s on-event=$ev comment=$c; \
             $mark (\"rearmed {seconds} \" . [/system resource get uptime]) }} }} on-error={{ $mark \"rearm-failed\" }}"
        );
    }
    out
}

/// The marks of a run, as [`instrument`] leaves them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Progress {
    /// the lines of the script
    pub lines: u32,
    /// the line of the last menu begun
    pub section: Option<u32>,
    pub done: bool,
    /// the line it stopped at and RouterOS' reason
    pub error: Option<(u32, Box<str>)>,
    /// the address pinged, and whether it answered before the script changed anything
    pub probe: Option<(Box<str>, bool)>,
    /// the lines after which the ping stopped (`true`) or came back (`false`), in order
    pub reach: Vec<(u32, bool)>,
    /// the fallback's new delay from the end
    pub rearmed: Option<u64>,
    /// the device's uptime then, which the scheduler counts by
    pub rearmed_at: Option<Duration>,
    /// bringing the fallback forward failed: its delay from the start counts
    pub rearm_failed: bool,
    /// the file as it is, each line with the device's time
    pub text: Box<str>,
}

impl Progress {
    /// The script has come to its end, or stopped at an error.
    pub fn finished(&self) -> bool {
        self.done || self.error.is_some()
    }

    /// When the fallback fires, where the script brought it forward.
    pub fn rearm(&self) -> Option<Rearmed> {
        Some(Rearmed {
            at: self.rearmed_at?,
            window: Duration::from_secs(self.rearmed?),
        })
    }

    /// The marks in `text`, if they are of `run`.
    pub fn parse(text: &str, run: &str) -> Option<Progress> {
        let mut progress = Progress {
            text: text.into(),
            ..Progress::default()
        };
        let mut of_run = false;
        for line in text.lines() {
            // `HH:MM:SS kind args`
            let Some((_, mark)) = line.split_once(' ') else {
                continue;
            };
            let (kind, args) = mark.split_once(' ').unwrap_or((mark, ""));
            let number = || args.split(' ').next().and_then(|n| n.parse::<u32>().ok());
            match kind {
                "start" => {
                    let mut parts = args.split(' ');
                    progress.lines = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                    of_run = parts.next() == Some(run);
                }
                "ping" => progress.probe = Some((args.into(), true)),
                "noping" => progress.probe = Some((args.into(), false)),
                "section" => progress.section = number(),
                "lost" => progress.reach.extend(number().map(|n| (n, true))),
                "back" => progress.reach.extend(number().map(|n| (n, false))),
                "done" => progress.done = true,
                "error" => {
                    let (line, message) = args.split_once(' ').unwrap_or((args, ""));
                    progress.error = Some((line.parse().unwrap_or(0), without_line(message).into()));
                }
                "rearmed" => {
                    let mut parts = args.split(' ');
                    progress.rearmed = parts.next().and_then(|n| n.parse().ok());
                    progress.rearmed_at = parts.next().and_then(script_uptime);
                }
                "rearm-failed" => progress.rearm_failed = true,
                _ => {}
            }
        }
        of_run.then_some(progress)
    }
}

/// An uptime as a script writes it: `1w1d11:21:27`, `2d00:00:05`, `00:12:34`.
fn script_uptime(text: &str) -> Option<Duration> {
    let (weeks, rest) = text.split_once('w').unwrap_or(("0", text));
    let (days, clock) = rest.split_once('d').unwrap_or(("0", rest));
    let mut parts = clock.split(':').map(|part| part.split('.').next().unwrap_or(part).parse::<u64>());
    let (Some(Ok(hours)), Some(Ok(minutes)), Some(Ok(seconds)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let days = weeks.parse::<u64>().ok()? * 7 + days.parse::<u64>().ok()?;
    Some(Duration::from_secs(((days * 24 + hours) * 60 + minutes) * 60 + seconds))
}

/// RouterOS' message without the line of the instrumented script, which isn't the one shown
/// (`… (/interface/list/add; line 12)` → `… (/interface/list/add)`).
fn without_line(message: &str) -> String {
    match message.rfind("; line ") {
        Some(at) if message.ends_with(')') => format!("{})", &message[..at]),
        _ => message.into(),
    }
}

/// The marks on the device, if there are any of `run` (in the root or under `flash/`).
pub async fn read(raw: &RawDevice<SimpleResult>, run: &str) -> Result<Option<Progress>, Error> {
    Ok(rows(raw, "/file/print")
        .await?
        .iter()
        .filter(|row| field(row, "name").is_some_and(|name| name.trim_start_matches("flash/") == FILE))
        .filter_map(|row| field(row, "contents"))
        .find_map(|text| Progress::parse(text, run)))
}

/// The provisioner's address as the device sees it: the one most of the device's API
/// sessions come from (the provisioner holds several), behind the SOCKS proxy the proxy's.
pub async fn provisioner_address(raw: &RawDevice<SimpleResult>) -> Result<Option<IpAddr>, Error> {
    let mut counts = HashMap::<IpAddr, usize>::new();
    for row in rows(raw, "/user/active/print").await? {
        if field(&row, "via") == Some("api")
            && let Some(address) = field(&row, "address").and_then(|a| a.parse().ok())
        {
            *counts.entry(address).or_default() += 1;
        }
    }
    Ok(counts.into_iter().max_by_key(|(_, count)| *count).map(|(address, _)| address))
}

#[cfg(test)]
mod test;
