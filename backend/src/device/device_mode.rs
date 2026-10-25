//! The device mode (`/system/device-mode`, RouterOS 7.17 and later) switches features off that
//! the provisioner needs, and only changes with a confirmation at the device: pressing its reset
//! or mode button, or a cold boot (cutting the power), while `/system/device-mode/update` counts
//! down. Checked on a CHR (7.24.5):
//! - the update answers `turn off power in 4m59s to activate changes` every second, as long as
//!   its connection stays open; closing the connection cancels it (a cold boot afterwards changes
//!   nothing), so the job holds it until the device goes down;
//! - when the time is up it ends with the trap `update canceled`;
//! - a warm reboot (`/system/reboot`) doesn't confirm, a cold boot does, for all features of
//!   the update at once;
//! - an update that changes nothing counts down all the same, so it is sent only for features
//!   that are off.
//!
//! What the provisioner needs is in [`wanted`]; the change runs as a job ([`change`]) that asks
//! the user for the confirmation and checks the result afterwards.
use crate::{
    Error,
    device_state::DeviceModeState,
    config::CONFIG,
    device::{
        power::PowerChain,
        raw::{Row, field, rows},
        update::{DeviceLink, REBOOT_TIMEOUT, Reboot, RebootReason},
    },
    graphql::scalars::ScalarDuration,
    job::Log,
    topology::access::device::DeviceAccess,
};
use async_graphql::{Enum, SimpleObject, futures_util::StreamExt};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout, timeout_at};

/// How long the device waits for the confirmation (`activation-timeout`).
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The flash RouterOS repartitions (`/partitions`, from 7.20 on): at least this many MiB.
const PARTITIONS_MIN_MIB: u64 = 128;

/// Why the provisioner needs a feature of the device mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum DeviceModeReason {
    /// `/system/scheduler`, which the fallback of an apply loads the backup with
    ApplyFallback,
    /// `/ip socks` on the device of the management proxy
    ManagementProxy,
    /// two partitions for the fallback of an update (`docs/concepts/update-fallback.md`), on a
    /// device whose flash can take them
    UpdateFallback,
}

/// A feature of the device mode the provisioner needs on.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeviceModeChange {
    /// its name in `/system/device-mode`, e.g. `partitions`
    pub feature: Box<str>,
    pub reason: DeviceModeReason,
}

/// The device mode of a device and what it lacks.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceMode {
    /// `home`, `basic`, `advanced`, …
    pub mode: Box<str>,
    /// the features the provisioner needs that are off
    pub changes: Vec<DeviceModeChange>,
}

/// The device counts down: confirm by pressing its button or cutting its power within
/// `timeout`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceModeConfirm {
    pub changes: Vec<DeviceModeChange>,
    pub timeout: ScalarDuration,
}

/// The device runs with these features on (none: they were on already).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceModeChanged {
    pub changes: Vec<DeviceModeChange>,
}

/// The features the provisioner needs on `device`; `partitions`: its flash can take two.
pub fn wanted(device: &DeviceAccess, partitions: bool) -> Vec<(&'static str, DeviceModeReason)> {
    let proxy = CONFIG
        .management_proxy
        .as_ref()
        .is_some_and(|proxy| *proxy.device == *device.name());
    wanted_for(proxy, partitions)
}

fn wanted_for(proxy: bool, partitions: bool) -> Vec<(&'static str, DeviceModeReason)> {
    let mut wanted = vec![("scheduler", DeviceModeReason::ApplyFallback)];
    if proxy {
        wanted.push(("socks", DeviceModeReason::ManagementProxy));
    }
    if partitions {
        wanted.push(("partitions", DeviceModeReason::UpdateFallback));
    }
    wanted
}

/// The `wanted` features that are off in `mode` (a row of `/system/device-mode/print`).
fn missing(mode: &Row, wanted: &[(&'static str, DeviceModeReason)]) -> Vec<DeviceModeChange> {
    wanted
        .iter()
        .filter(|(feature, _)| field(mode, feature) == Some("false"))
        .map(|&(feature, reason)| DeviceModeChange {
            feature: feature.into(),
            reason,
        })
        .collect()
}

/// Whether the device can have partitions: RouterOS lists one there on NAND flash (none on
/// 16 MiB devices, which have NOR), and repartitions from [`PARTITIONS_MIN_MIB`] on.
async fn supports_partitions(device: &RawDevice<SimpleResult>) -> bool {
    // a CHR has no `/partitions`
    let Ok(partitions) = rows(device, "/partitions/print").await else {
        return false;
    };
    let size: u64 = partitions
        .iter()
        .filter_map(|row| field(row, "size")?.parse::<u64>().ok())
        .sum();
    size >= PARTITIONS_MIN_MIB
}

/// The device mode of `device` and what it lacks for `access`; `None` before RouterOS 7.17,
/// which has none (and locks nothing).
pub async fn read(
    device: &RawDevice<SimpleResult>,
    access: &DeviceAccess,
) -> Result<Option<DeviceMode>, Error> {
    let wanted = wanted(access, supports_partitions(device).await);
    read_with(device, &wanted).await
}

async fn read_with(
    device: &RawDevice<SimpleResult>,
    wanted: &[(&'static str, DeviceModeReason)],
) -> Result<Option<DeviceMode>, Error> {
    let mode = match rows(device, "/system/device-mode/print").await {
        Ok(rows) => rows.into_iter().next(),
        // no such menu
        Err(Error::Refused(_)) => None,
        Err(error) => return Err(error),
    };
    Ok(mode.map(|mode| DeviceMode {
        mode: field(&mode, "mode").unwrap_or_default().into(),
        changes: missing(&mode, wanted),
    }))
}

/// Turns the features of `access` on that the device mode has off (`wanted`): asks the device
/// for the change, tells the user to confirm it at the device ([`DeviceModeConfirm`]) and waits
/// until the device runs with them. Cancelling before the confirmation leaves the device mode
/// as it was. `power`: the device loses its power or reboots, and with it what it powers.
pub async fn change(
    link: &impl DeviceLink,
    access: &DeviceAccess,
    power: &PowerChain,
    log: &Log,
) -> Result<(), Error> {
    let device = link.api().await?;
    let wanted = wanted(access, supports_partitions(&device).await);
    change_to(link, &wanted, power, log).await
}

/// [`change`] with the features given.
pub async fn change_to(
    link: &impl DeviceLink,
    wanted: &[(&'static str, DeviceModeReason)],
    power: &PowerChain,
    log: &Log,
) -> Result<(), Error> {
    let device = link.api().await?;
    let changes = read_with(&device, wanted)
        .await?
        .map(|mode| mode.changes)
        .unwrap_or_default();
    if changes.is_empty() {
        log.emit(DeviceModeChanged { changes });
        return Ok(());
    }
    // what the device powers goes down with it
    log.enter(DeviceModeState::WaitingForChain);
    let _rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    log.enter(DeviceModeState::Requesting);
    let confirm_timeout = format!("{}s", CONFIRM_TIMEOUT.as_secs());
    // the countdown has its own deadline below, not the API's limit without a reply
    let mut countdown = device
        .send_command_with_reply_timeout(
            b"/system/device-mode/update".as_slice(),
            |cmd| {
                changes
                    .iter()
                    .fold(cmd, |cmd, change| cmd.attribute(change.feature.as_bytes(), b"yes".as_slice()))
                    .attribute(b"activation-timeout".as_slice(), confirm_timeout.as_bytes())
            },
            (),
            None,
        )
        .await;
    let refused = |message: &str| Error::Refused(format!("/system/device-mode/update: {message}").into());
    match timeout(Duration::from_secs(30), countdown.next()).await {
        Ok(Some(SimpleResult::Sentence(_))) => {}
        Ok(Some(SimpleResult::Trap { message, .. })) => return Err(refused(&message)),
        Ok(Some(SimpleResult::Error(error))) => return Err(error.into()),
        Ok(None) | Err(_) => return Err(refused("no countdown")),
    }
    log.emit(DeviceModeConfirm {
        changes: changes.clone(),
        timeout: CONFIRM_TIMEOUT.into(),
    });
    log.enter(DeviceModeState::WaitingForConfirmation);
    // until the device goes down for the confirmation; cancelling drops the connection, which
    // cancels the change on the device
    let deadline = Instant::now() + CONFIRM_TIMEOUT + Duration::from_secs(30);
    loop {
        let next = log
            .unless_cancelled(timeout_at(deadline, countdown.next()))
            .await?;
        match next {
            Ok(Some(SimpleResult::Sentence(_))) => {}
            // RouterOS ends the countdown with `update canceled`
            Ok(Some(SimpleResult::Trap { .. }) | None) | Err(_) => {
                return Err(Error::DeviceModeNotConfirmed);
            }
            // the connection broke: the device went down
            Ok(Some(SimpleResult::Error(_))) => break,
        }
    }
    drop(countdown);
    drop(device);
    log.enter(DeviceModeState::Rebooting);
    log.emit(Reboot {
        reason: RebootReason::DeviceMode,
        timeout: REBOOT_TIMEOUT.into(),
    });
    // done when the device runs with the features on, whatever happened in between
    let deadline = Instant::now() + REBOOT_TIMEOUT;
    let mut left = changes.clone();
    loop {
        log.unless_cancelled(sleep(Duration::from_secs(5))).await?;
        if let Ok(Ok(device)) = timeout(Duration::from_secs(30), link.api()).await
            && let Ok(Some(mode)) = read_with(&device, wanted).await
        {
            if mode.changes.is_empty() {
                break;
            }
            left = mode.changes;
        }
        if Instant::now() > deadline {
            let features = left.iter().map(|c| &*c.feature).collect::<Vec<_>>().join(", ");
            return Err(Error::DeviceModeUnchanged(features.into()));
        }
    }
    log.emit(DeviceModeChanged { changes });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DeviceModeChange, DeviceModeReason, missing, wanted_for};
    use crate::device::raw::Row;

    fn mode(fields: &[(&str, &str)]) -> Row {
        fields
            .iter()
            .map(|&(name, value)| (name.into(), Some(value.into())))
            .collect()
    }

    #[test]
    fn asks_only_for_what_is_off() {
        // lab-core: the management proxy's device, with 128 MiB of NAND
        let wanted = wanted_for(true, true);
        let lab_core = mode(&[
            ("mode", "advanced"),
            ("scheduler", "true"),
            ("socks", "true"),
            ("partitions", "false"),
        ]);
        assert_eq!(
            missing(&lab_core, &wanted),
            [DeviceModeChange {
                feature: "partitions".into(),
                reason: DeviceModeReason::UpdateFallback,
            }]
        );
        // home mode has the scheduler off; a 16 MiB device gets no partitions
        let home = mode(&[("mode", "home"), ("scheduler", "false"), ("partitions", "false")]);
        assert_eq!(
            missing(&home, &wanted_for(false, false)),
            [DeviceModeChange {
                feature: "scheduler".into(),
                reason: DeviceModeReason::ApplyFallback,
            }]
        );
        // a feature this RouterOS doesn't know isn't asked for
        assert!(missing(&mode(&[("mode", "advanced")]), &wanted).is_empty());
    }
}
