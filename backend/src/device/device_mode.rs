//! Scope: The device mode: which features the provisioner needs and changing them; nothing else.
//!
//! The device mode (`/system/device-mode`, RouterOS 7.17 and later) switches features off that
//! the provisioner needs, and only changes with a confirmation at the device: pressing its reset
//! or mode button, or a cold boot (cutting the power), while `/system/device-mode/update` counts
//! down. Checked on a CHR (7.24.5):
//! - the update answers `turn off power in 4m59s to activate changes` every second, as long as
//!   its connection stays open; closing the connection cancels it (a cold boot afterwards changes
//!   nothing), so the job holds it until the device goes down. A device whose power is cut
//!   closes nothing, its connection only falls silent ([`SILENCE`]; the CHR's QEMU closes it
//!   when killed, a RouterBOARD doesn't: a job on one waited out the countdown and was
//!   cancelled, though the change had been confirmed);
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
    config::CONFIG,
    device::{
        access::{Session, collect_if_present},
        dependency::PowerChain,
        reboot::{REBOOT_TIMEOUT, Reboot, RebootReason},
        update::DeviceLink,
    },
    device_state::DeviceModeState,
    graphql::scalars::ScalarDuration,
    job::Log,
    topology::access::device::DeviceAccess,
};
use async_graphql::{Enum, SimpleObject, futures_util::StreamExt};
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::{
    action::DeviceModeUpdate,
    command::Reads,
    model::{PartitionsState, SystemDeviceModeState},
    resource::{self, SingleResource},
    value::RosValue,
};
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout, timeout_at};

/// How long the device waits for the confirmation (`activation-timeout`).
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// No answer of the countdown for this long: the device went down (its power cut). It answers
/// every second while it counts down.
const SILENCE: Duration = Duration::from_secs(15);
/// The flash RouterOS repartitions (`/partitions`, from 7.20 on): at least this many MiB.
const PARTITIONS_MIN_MIB: u64 = 128;

/// Why the provisioner needs a feature of the device mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum DeviceModeReason {
    /// `/system/scheduler`, which the fallback of an apply loads the backup with
    ApplyFallback,
    /// `/ip socks` on the device of the management proxy
    ManagementProxy,
    /// two partitions for the fallback of an update (`docs/update-fallback.md`), on a
    /// device whose flash can take them
    UpdateFallback,
}

/// A feature of the device mode the provisioner needs on.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeviceModeChange {
    pub feature: DeviceModeFeature,
    pub reason: DeviceModeReason,
}

/// A feature of `/system/device-mode` the provisioner may need.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum DeviceModeFeature {
    Scheduler,
    Socks,
    Partitions,
}

impl DeviceModeFeature {
    /// Its name in `/system/device-mode`.
    fn name(self) -> &'static str {
        match self {
            DeviceModeFeature::Scheduler => "scheduler",
            DeviceModeFeature::Socks => "socks",
            DeviceModeFeature::Partitions => "partitions",
        }
    }
    /// Whether `mode` has it on; `None` where this RouterOS doesn't know it.
    fn in_mode(self, mode: &SystemDeviceModeState) -> Option<bool> {
        match self {
            DeviceModeFeature::Scheduler => mode.scheduler,
            DeviceModeFeature::Socks => mode.socks,
            DeviceModeFeature::Partitions => mode.partitions,
        }
    }
}

/// The device mode of a device and what it lacks.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceMode {
    /// `home`, `basic`, `advanced`, …
    pub mode: Box<str>,
    /// the features the provisioner needs that are off
    pub changes: Box<[DeviceModeChange]>,
}

/// The device counts down: confirm by pressing its button or cutting its power within
/// `timeout`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceModeConfirm {
    pub changes: Box<[DeviceModeChange]>,
    pub timeout: ScalarDuration,
}

/// The device runs with these features on (none: they were on already).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceModeChanged {
    pub changes: Box<[DeviceModeChange]>,
}

/// The features the provisioner needs on `device`; `partitions`: its flash can take two.
pub fn wanted(device: &DeviceAccess, partitions: bool) -> Vec<DeviceModeChange> {
    let proxy = CONFIG
        .management_proxy
        .as_ref()
        .is_some_and(|proxy| *proxy.device == *device.name());
    wanted_for(proxy, partitions)
}

fn wanted_for(proxy: bool, partitions: bool) -> Vec<DeviceModeChange> {
    let feature = |feature, reason| DeviceModeChange { feature, reason };
    let mut wanted = vec![feature(
        DeviceModeFeature::Scheduler,
        DeviceModeReason::ApplyFallback,
    )];
    if proxy {
        wanted.push(feature(
            DeviceModeFeature::Socks,
            DeviceModeReason::ManagementProxy,
        ));
    }
    if partitions {
        wanted.push(feature(
            DeviceModeFeature::Partitions,
            DeviceModeReason::UpdateFallback,
        ));
    }
    wanted
}

/// The `wanted` features that are off in `mode`.
fn missing(mode: &SystemDeviceModeState, wanted: &[DeviceModeChange]) -> Vec<DeviceModeChange> {
    wanted
        .iter()
        .filter(|wanted| wanted.feature.in_mode(mode) == Some(false))
        .cloned()
        .collect()
}

/// Whether the device can have partitions: RouterOS lists one there on NAND flash (none on
/// 16 MiB devices, which have NOR), and repartitions from [`PARTITIONS_MIN_MIB`] on.
async fn supports_partitions(device: &impl Reads) -> bool {
    // a CHR has no `/partitions`
    let Ok(partitions) = collect_if_present::<PartitionsState>(device).await else {
        return false;
    };
    let size: u64 = partitions.iter().filter_map(|p| p.size).sum();
    size >= PARTITIONS_MIN_MIB
}

/// The device mode of `device` and what it lacks for `access`; `None` before RouterOS 7.17,
/// which has none (and locks nothing).
pub async fn read(device: &impl Reads, access: &DeviceAccess) -> Result<Option<DeviceMode>, Error> {
    let wanted = wanted(access, supports_partitions(device).await);
    read_with(device, &wanted).await
}

async fn read_with(
    device: &impl Reads,
    wanted: &[DeviceModeChange],
) -> Result<Option<DeviceMode>, Error> {
    let mode = match SystemDeviceModeState::fetch(device).await {
        Ok(mode) => mode,
        // no such menu
        Err(resource::Error::Trap(_)) => None,
        Err(error) => return Err(error.into()),
    };
    Ok(mode.map(|mode| DeviceMode {
        mode: String::from_utf8_lossy(&mode.mode.encode_ros()).into(),
        changes: missing(&mode, wanted).into(),
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
    wanted: &[DeviceModeChange],
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
    let features = changes
        .iter()
        .map(|change| change.feature.name())
        .collect::<Vec<_>>();
    // the countdown has its own deadline below, not the API's limit without a reply
    let mut countdown = hold_countdown(&device, &features).await;
    let refused =
        |message: &str| Error::Refused(format!("/system/device-mode/update: {message}").into());
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
        let wait = deadline.min(Instant::now() + SILENCE);
        let next = log
            .unless_cancelled(timeout_at(wait, countdown.next()))
            .await?;
        match next {
            Ok(Some(SimpleResult::Sentence(_))) => {}
            // RouterOS ends the countdown with `update canceled`
            Ok(Some(SimpleResult::Trap { .. }) | None) => {
                return Err(Error::DeviceModeNotConfirmed);
            }
            Err(_) if Instant::now() >= deadline => return Err(Error::DeviceModeNotConfirmed),
            // silent: its power is cut
            Err(_) => break,
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
            let features = left
                .iter()
                .map(|c| c.feature.name())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::DeviceModeUnchanged(features.into()));
        }
    }
    log.emit(DeviceModeChanged { changes });
    Ok(())
}

/// Sends `/system/device-mode/update` turning `features` on; its countdown answers for as long as
/// `device` holds it.
async fn hold_countdown(
    device: &Session,
    features: &[&str],
) -> mikrotik_api::prelude::Replies<SimpleResult> {
    let update = DeviceModeUpdate {
        mode: None,
        features,
        activation_timeout: CONFIRM_TIMEOUT,
    };
    device.hold(&update).await
}

#[cfg(test)]
mod tests {
    use super::{DeviceModeChange, DeviceModeFeature, DeviceModeReason, missing, wanted_for};
    use crate::device::answer::parsed;
    use mikrotik_model::model::SystemDeviceModeState;

    fn mode(fields: &[(&str, &str)]) -> SystemDeviceModeState {
        parsed(fields)
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
                feature: DeviceModeFeature::Partitions,
                reason: DeviceModeReason::UpdateFallback,
            }]
        );
        // home mode has the scheduler off; a 16 MiB device gets no partitions
        let home = mode(&[
            ("mode", "home"),
            ("scheduler", "false"),
            ("partitions", "false"),
        ]);
        assert_eq!(
            missing(&home, &wanted_for(false, false)),
            [DeviceModeChange {
                feature: DeviceModeFeature::Scheduler,
                reason: DeviceModeReason::ApplyFallback,
            }]
        );
        // a feature this RouterOS doesn't know isn't asked for
        assert!(missing(&mode(&[("mode", "advanced")]), &wanted).is_empty());
    }
}
