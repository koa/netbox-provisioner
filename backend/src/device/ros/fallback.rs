//! The way back from an apply that cuts the device off: before the config changes, the device
//! saves a backup and schedules loading it after a deadline (`/system/scheduler`, which runs
//! a script with `interval` once that long after it was added, and removes itself first). When
//! the provisioner reaches the device again after the apply, it takes both away again. When it
//! doesn't, the device loads the backup and reboots with the config from before.
//!
//! A backup brings back the whole config, also what the model doesn't know, and needs no
//! inverse of the diff (an entry added by the diff has no `.id` before, and the firewall's
//! order would have to be restored). The price is the reboot, only when the fallback fires.
//! Checked on a CHR (7.24.5): `/system backup load` from a scheduler asks nothing.

use crate::{
    Error,
    device::raw::{command, field, rows},
};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::value::{ParseRosValueResult, RosValue};
use rand::{Rng, distr::Alphanumeric};
use std::time::Duration;

/// The name of the scheduler entry and of the backup (`.backup` added by RouterOS).
pub const NAME: &str = "netbox-provisioner-fallback";

/// How long before the fallback fires it is no longer taken away: it may be loading the backup
/// by the time the provisioner's remove gets there.
pub(crate) const DUE: Duration = Duration::from_secs(10);

fn backup_file() -> String {
    format!("{NAME}.backup")
}

/// Saves the backup and schedules loading it in `delay`. A fallback left from an apply the
/// provisioner didn't see to the end is replaced. `Some(mode)` and nothing saved where the
/// device mode `mode` has the scheduler off (`home`): RouterOS refuses the entry ("not allowed
/// by device-mode", berghof-hansueli, 7.21.5), so there is no fallback.
pub async fn arm(raw: &RawDevice<SimpleResult>, delay: Duration) -> Result<Option<Box<str>>, Error> {
    if let Some(mode) = without_scheduler(raw).await? {
        return Ok(Some(mode));
    }
    remove_scheduler(raw).await?;
    // encrypted, should the file be copied off the device; the password is only in the
    // scheduler entry
    let password = rand::rng()
        .sample_iter(Alphanumeric)
        .take(24)
        .map(char::from)
        .collect::<String>();
    command(
        raw,
        "/system/backup/save",
        &[("name", NAME), ("password", &password)],
    )
    .await?;
    let file = backup_file();
    let saved = rows(raw, "/file/print")
        .await?
        .iter()
        .any(|row| field(row, "name") == Some(file.as_str()));
    if !saved {
        return Err(Error::Refused(format!("no {file} after saving it").into()));
    }
    // the backup before the scheduler: what it brings back has no fallback scheduled
    let on_event = format!(
        "/system scheduler remove [find name=\"{NAME}\"]; \
         /system backup load name=\"{file}\" password=\"{password}\""
    );
    let interval = format!("{}s", delay.as_secs().max(1));
    command(
        raw,
        "/system/scheduler/add",
        &[
            ("name", NAME),
            ("interval", &interval),
            ("on-event", &on_event),
            ("comment", "loads the config from before the apply, unless the provisioner stops it"),
        ],
    )
    .await?;
    Ok(None)
}

/// The device mode, where it has the scheduler off; `None` with the scheduler on, or before
/// RouterOS 7.17, which has no device mode.
async fn without_scheduler(raw: &RawDevice<SimpleResult>) -> Result<Option<Box<str>>, Error> {
    let mode = match rows(raw, "/system/device-mode/print").await {
        Ok(rows) => rows.into_iter().next(),
        Err(Error::Refused(_)) => None,
        Err(error) => return Err(error),
    };
    Ok(mode
        .filter(|mode| field(mode, "scheduler") == Some("false"))
        .map(|mode| field(&mode, "mode").unwrap_or_default().into()))
}

/// Where the fallback stands on the device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pending {
    /// it fires in this long (`None` before the script brought it forward, or where the
    /// device's uptime can't be read)
    Armed(Option<Duration>),
    /// no scheduler entry: it fired, or was taken away
    Gone,
}

/// The script brought the fallback forward (`progress::Instrument::rearm`): at this uptime of
/// the device, to fire `window` later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rearmed {
    pub at: Duration,
    pub window: Duration,
}

/// Where the fallback stands; when it fires from the uptime the script brought it forward at
/// (`rearmed`). Not from `next-run` and the clock: a scheduler counts by uptime, and a clock
/// that jumps forward (NTP setting it after the apply) leaves `next-run` behind, which then
/// looked due (1og-sw1: „fires in 0 s“ at 51 s, so the fallback stayed and took a good apply
/// back; on a CHR 7.24.5 an hour's jump left `next-run` and the entry didn't run).
pub async fn pending(raw: &RawDevice<SimpleResult>, rearmed: Option<Rearmed>) -> Result<Pending, Error> {
    let entry = rows(raw, "/system/scheduler/print")
        .await?
        .into_iter()
        .find(|row| field(row, "name") == Some(NAME));
    if entry.is_none() {
        return Ok(Pending::Gone);
    }
    let Some(rearmed) = rearmed else {
        return Ok(Pending::Armed(None));
    };
    Ok(Pending::Armed(
        uptime(raw)
            .await?
            .map(|uptime| (rearmed.at + rearmed.window).saturating_sub(uptime)),
    ))
}

/// The device's uptime (`/system/resource`).
async fn uptime(raw: &RawDevice<SimpleResult>) -> Result<Option<Duration>, Error> {
    Ok(rows(raw, "/system/resource/print")
        .await?
        .first()
        .and_then(|resource| field(resource, "uptime"))
        .and_then(|uptime| match Duration::parse_ros(uptime.as_bytes()) {
            ParseRosValueResult::Value(uptime) => Some(uptime),
            _ => None,
        }))
}

/// What [`disarm_unless_due`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Disarmed {
    /// taken away
    Stopped,
    /// left, it fires in this long, within [`DUE`]
    Due(Duration),
    /// it fired already (or was taken away elsewhere)
    Gone,
}

/// Takes the fallback away unless it fires within [`DUE`] or has fired.
pub(crate) async fn disarm_unless_due(
    raw: &RawDevice<SimpleResult>,
    rearmed: Option<Rearmed>,
) -> Result<Disarmed, Error> {
    match pending(raw, rearmed).await? {
        Pending::Gone => Ok(Disarmed::Gone),
        Pending::Armed(Some(fires_in)) if fires_in < DUE => Ok(Disarmed::Due(fires_in)),
        Pending::Armed(_) => disarm(raw).await.map(|()| Disarmed::Stopped),
    }
}

/// Takes the fallback away: the device is reachable with the new config.
pub async fn disarm(raw: &RawDevice<SimpleResult>) -> Result<(), Error> {
    remove_scheduler(raw).await?;
    let file = backup_file();
    let ids = rows(raw, "/file/print")
        .await?
        .iter()
        .filter(|row| field(row, "name") == Some(file.as_str()))
        .filter_map(|row| field(row, ".id").map(String::from))
        .collect::<Vec<_>>();
    for id in ids {
        command(raw, "/file/remove", &[(".id", &id)]).await?;
    }
    Ok(())
}

async fn remove_scheduler(raw: &RawDevice<SimpleResult>) -> Result<(), Error> {
    let ids = rows(raw, "/system/scheduler/print")
        .await?
        .iter()
        .filter(|row| field(row, "name") == Some(NAME))
        .filter_map(|row| field(row, ".id").map(String::from))
        .collect::<Vec<_>>();
    for id in ids {
        command(raw, "/system/scheduler/remove", &[(".id", &id)]).await?;
    }
    Ok(())
}
