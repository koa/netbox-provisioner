//! What a job left on a device when it didn't see its work to the end (cancelled, the
//! provisioner restarted or the device unreached): the scheduler entries of the apply fallback,
//! the update fallback and the watchdogs' safety net, the apply's script still running and
//! uploaded packages, which install at the next boot. The script itself stays after every
//! apply (the next one replaces it), so only a running one counts, and it ends by itself. The device page shows them, each with what the operator can
//! do about it (`docs/concepts/job-polling.md`); nothing is cleared by itself.
//!
//! A device mode change waiting for its confirmation is no leftover: RouterOS cancels it when
//! the connection of its job closes.
use crate::{
    Error,
    device::{
        raw::{Row, command, field, rows},
        ros::{fallback, graphql::APPLY_SCRIPT},
        update::{self, DeviceLink, REBOOT_TIMEOUT, Reboot, RebootReason, read_state},
        update_fallback, watchdog,
    },
    job::Log,
};
use async_graphql::{Enum, SimpleObject};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use tokio::time::Instant;

/// What is left on the device; `None` fields aren't there.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Leftovers {
    /// loads the config from before an apply and reboots
    pub apply_fallback: Option<ScheduledLeft>,
    /// boots the partition from before an update
    pub update_fallback: Option<ScheduledLeft>,
    /// restores the watchdogs suspended for an update or a radio change
    pub watchdogs: Option<WatchdogsLeft>,
    /// the script of an apply runs (`/system/script/job`); it ends by itself
    pub apply_script_running: bool,
    /// `.npk` files, installed at the next boot
    pub packages: Vec<PackageLeft>,
}

impl Leftovers {
    pub fn is_empty(&self) -> bool {
        self.apply_fallback.is_none()
            && self.update_fallback.is_none()
            && self.watchdogs.is_none()
            && !self.apply_script_running
            && self.packages.is_empty()
    }
}

/// A scheduler entry of the provisioner.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ScheduledLeft {
    /// `next-run` as the device reports it (by its clock, which may have jumped since); `None`
    /// for an entry run at startup
    pub next_run: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogsLeft {
    pub next_run: Option<Box<str>>,
    pub watching: watchdog::Watching,
    /// a running job of another device suspended them and restores them at its end; they
    /// aren't a leftover then, and can't be restored meanwhile (`LEFTOVER_HELD`)
    pub held_by: Option<crate::job::WatchdogsHeld>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageLeft {
    pub file: Box<str>,
    /// bytes, as the device reports them
    pub size: Option<Box<str>>,
}

/// What the operator can do about a leftover.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum LeftoverAction {
    /// take the apply fallback away: the config as it is stays
    StopApplyFallback,
    /// load the config from before the apply now (the device reboots)
    ApplyFallbackNow,
    /// take the update fallback away: the RouterOS running stays
    StopUpdateFallback,
    /// set the watchdogs as the safety net would
    RestoreWatchdogs,
    /// remove the uploaded packages
    RemovePackages,
}

/// The leftover is taken care of.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct LeftoverCleared {
    pub action: LeftoverAction,
}

/// Reads what is left on the device.
pub async fn read(device: &RawDevice<SimpleResult>) -> Result<Leftovers, Error> {
    let schedulers = rows(device, "/system/scheduler/print").await?;
    let scheduled = |name: &str| {
        schedulers.iter().find(|row| field(row, "name") == Some(name)).map(|row| ScheduledLeft {
            next_run: field(row, "next-run").filter(|run| !run.is_empty()).map(Box::from),
        })
    };
    let watchdogs = match scheduled(watchdog::NAME) {
        Some(entry) => watchdog::pending(device).await?.map(|watching| WatchdogsLeft {
            next_run: entry.next_run,
            watching,
            held_by: None,
        }),
        None => None,
    };
    let apply_script_running = rows(device, "/system/script/job/print")
        .await?
        .iter()
        .any(|job| field(job, "script") == Some(APPLY_SCRIPT));
    let packages = packages(device)
        .await?
        .iter()
        .filter_map(|row| {
            Some(PackageLeft {
                file: field(row, "name")?.into(),
                size: field(row, "size").map(Box::from),
            })
        })
        .collect();
    Ok(Leftovers {
        apply_fallback: scheduled(fallback::NAME),
        update_fallback: scheduled(update_fallback::NAME),
        watchdogs,
        apply_script_running,
        packages,
    })
}

/// The `.npk` files on the device.
async fn packages(device: &RawDevice<SimpleResult>) -> Result<Vec<Row>, Error> {
    Ok(rows(device, "/file/print")
        .await?
        .into_iter()
        .filter(|row| field(row, "name").is_some_and(|name| name.ends_with(".npk")))
        .collect())
}

/// Does `action` on the device `link` reaches (`name` for the errors).
pub async fn clear(link: &impl DeviceLink, name: &str, action: LeftoverAction, log: &Log) -> Result<(), Error> {
    let device = link.api().await?;
    let gone = || Error::LeftoverGone(name.into());
    match action {
        LeftoverAction::StopApplyFallback => fallback::disarm(&device).await?,
        LeftoverAction::ApplyFallbackNow => {
            // what the scheduler would run: remove itself, load the backup (which reboots)
            let on_event = rows(&device, "/system/scheduler/print")
                .await?
                .iter()
                .find(|row| field(row, "name") == Some(fallback::NAME))
                .and_then(|row| field(row, "on-event").map(String::from))
                .ok_or_else(gone)?;
            let before = read_state(&device).await?.uptime;
            let sent = Instant::now();
            command(&device, "/execute", &[("script", &on_event)]).await?;
            drop(device);
            log.emit(Reboot {
                reason: RebootReason::ApplyFallback,
                timeout: REBOOT_TIMEOUT.into(),
            });
            update::wait_until_back(link, log, before, sent, REBOOT_TIMEOUT, |_| Some(())).await?;
        }
        LeftoverAction::StopUpdateFallback => update_fallback::remove_scheduler(&device).await?,
        LeftoverAction::RestoreWatchdogs => {
            let watching = watchdog::pending(&device).await?.ok_or_else(gone)?;
            watchdog::restore(&device, &watching).await?;
        }
        LeftoverAction::RemovePackages => {
            for row in packages(&device).await? {
                if let Some(id) = field(&row, ".id") {
                    command(&device, "/file/remove", &[(".id", id)]).await?;
                }
            }
        }
    }
    log.emit(LeftoverCleared { action });
    Ok(())
}
