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
use rand::{Rng, distr::Alphanumeric};
use std::time::Duration;

/// The name of the scheduler entry and of the backup (`.backup` added by RouterOS).
const NAME: &str = "netbox-provisioner-fallback";

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
