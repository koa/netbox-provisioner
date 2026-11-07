//! Scope: Rebooting a device and waiting until it is back: the plain reboot („Neu starten“) and
//! the waiting the update, the repartition and the boot from another partition share; not why
//! they reboot (`update`, `partitions`, `device_mode`).
use crate::{
    Error,
    device::{
        dependency::{PowerChain, RebootLease},
        update::{DeviceLink, DeviceState, RouterOsRunning, read_state},
    },
    device_state::RebootState,
    graphql::scalars::ScalarDuration,
    job::Log,
};
use async_graphql::futures_util::StreamExt;
use async_graphql::{Enum, SimpleObject};
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout};

/// Booting a package update takes a few minutes on small devices.
pub const REBOOT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The device reboots; the job waits until it is back (up to [`REBOOT_TIMEOUT`]).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Reboot {
    pub reason: RebootReason,
    /// how long the job waits for it
    pub timeout: ScalarDuration,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RebootReason {
    /// to install the uploaded packages
    Packages,
    /// to load the upgraded RouterBOARD firmware
    Firmware,
    /// asked for (`reboot`)
    Requested,
    /// a cold boot (power cut or button) confirming a device-mode change
    DeviceMode,
    /// the flash is reformatted in partitions
    Repartition,
    /// to boot from another partition (`bootPartition`)
    Partition,
    /// `/system/package/downgrade` installs the uploaded older packages
    Downgrade,
    /// the apply fallback left on the device loads the config from before, asked for
    ApplyFallback,
}

/// Reboots and waits (up to `wait`) until the device answers with `expect` (a version, a
/// firmware); the lease is held until the device is back.
pub(crate) async fn reboot_and_wait<T, F>(
    link: &impl DeviceLink,
    rebooting: RebootLease,
    log: &Log,
    reason: RebootReason,
    wait: Duration,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    restart_and_wait(link, rebooting, log, reason, wait, "/system/reboot", check).await
}

/// [`reboot_and_wait`] with `path` as the command that reboots (`/system/package/downgrade`).
pub(crate) async fn restart_and_wait<T, F>(
    link: &impl DeviceLink,
    _rebooting: RebootLease,
    log: &Log,
    reason: RebootReason,
    wait: Duration,
    path: &str,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    let device = link.api().await?;
    let before = read_state(&device).await?.uptime;
    let sent = Instant::now();
    let mut answer = device.send_simple_command(path.as_bytes(), ()).await;
    // the answer, or the connection breaking with the reboot
    if let Ok(Some(SimpleResult::Trap { message, .. })) =
        timeout(Duration::from_secs(10), answer.next()).await
    {
        return Err(Error::Refused(format!("{path}: {message}").into()));
    }
    drop(answer);
    drop(device);
    log.emit(Reboot {
        reason,
        timeout: wait.into(),
    });
    wait_until_back(link, log, before, sent, wait, check).await
}

/// Waits (up to `wait`, usually [`REBOOT_TIMEOUT`]) until the device answers again after a
/// boot that came after `sent`, when it had been up for `before`, and checks it with `check`.
pub(crate) async fn wait_until_back<T, F>(
    link: &impl DeviceLink,
    log: &Log,
    before: Duration,
    sent: Instant,
    wait: Duration,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    // A connection while the device boots may just wait for it (it does through QEMU's port
    // forwarding), so the reboot shows in the uptime, not in a failing connection.
    let deadline = Instant::now() + wait;
    loop {
        log.unless_cancelled(sleep(Duration::from_secs(5))).await?;
        // a cancel doesn't wait for the try to end
        let state = match log
            .unless_cancelled(timeout(Duration::from_secs(30), link.api()))
            .await?
        {
            Ok(Ok(device)) => read_state(&device).await.ok(),
            _ => None,
        };
        if let Some(state) = state
            && state.uptime < before + sent.elapsed().saturating_sub(Duration::from_secs(5))
        {
            if let Some(result) = check(&state) {
                return Ok(result);
            }
            return Err(Error::UnexpectedAfterReboot {
                version: state.version.to_string().into(),
                firmware: state.firmware.map(|(running, _)| running),
            });
        }
        if Instant::now() > deadline {
            return Err(Error::NotBackAfterReboot);
        }
    }
}

/// Reboots the device and waits until it is back, as an update does (`power` as for
/// [`update`](super::update::update): not while a device it powers updates).
pub async fn reboot(link: &impl DeviceLink, power: &PowerChain, log: &Log) -> Result<(), Error> {
    log.enter(RebootState::WaitingForChain);
    let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    log.enter(RebootState::Rebooting);
    let state = reboot_and_wait(
        link,
        rebooting,
        log,
        RebootReason::Requested,
        REBOOT_TIMEOUT,
        |state| Some(state.clone()),
    )
    .await?;
    log.emit(RouterOsRunning {
        version: state.version.to_string().into(),
    });
    Ok(())
}
