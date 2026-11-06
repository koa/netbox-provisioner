//! Going on after a restart with the jobs saved before it (`saved_job`): an apply watching
//! its script, an update waiting for its reboot, a batch's applies together.
use crate::{
    Error,
    device::{
        AccessibleDevice, Connecting,
        power::PowerChain,
        update,
        watchdog::{self, restore_watchdogs},
    },
    device_state::UpdateState,
    graphql::connect_for_job,
    job::{self, JobKind, Log},
    journal::Started,
    saved_job::{self, SavedWork},
    topology::{
        DeviceId, TopologyHolder,
        access::{DeviceTarget, device::DeviceAccess},
    },
};
use std::time::SystemTime;

/// Goes on with the jobs saved before this start whose device NetBox has (`saved_job`), each
/// as a job of its own, a batch's devices together; the others stay in `savedJobs`. Called
/// once, after the topology is read.
pub async fn resume_saved_jobs(topology: &TopologyHolder) {
    let waiting = saved_job::waiting();
    let mut batches = std::collections::BTreeSet::new();
    for entry in &waiting {
        let Ok(saved) = &entry.saved else {
            continue;
        };
        let Some(device) = topology.devices_by_id(entry.device).await else {
            saved_job::hold_back(entry.device, saved_job::HeldBack::DeviceGone);
            continue;
        };
        let after = SystemTime::now()
            .duration_since(saved.saved_at)
            .unwrap_or_default();
        let started = match &saved.work {
            SavedWork::Apply { apply, .. } if apply.batch.is_some() => {
                if let Some(batch) = &apply.batch {
                    batches.insert(batch.devices.clone());
                }
                continue;
            }
            SavedWork::Apply { apply, .. } => {
                let (saved, apply) = (saved.clone(), apply.clone());
                saved_job::take(entry.device);
                job::start(
                    &device.clone(),
                    JobKind::Apply,
                    Started::Resumed {
                        entry: saved.journal_entry,
                    },
                    |log| async move {
                        log.emit(job::Resumed {
                            after: after.into(),
                        });
                        if let Some(hold) = &apply.hold {
                            crate::device::radio_hold::RadioHold::announce_saved(
                                device.name(),
                                hold,
                                &log,
                            );
                        }
                        log.keep(saved);
                        // saved taking the fallback away too: that only follows a look at the
                        // marks, which comes first again
                        log.resume_in(crate::device_state::ApplyState::ScriptRunning);
                        resume_apply(&device, &apply, &log).await
                    },
                )
            }
            SavedWork::Update { state, update } => {
                let (saved, state, update) = (saved.clone(), *state, update.clone());
                saved_job::take(entry.device);
                job::start(
                    &device.clone(),
                    JobKind::Update,
                    Started::Resumed {
                        entry: saved.journal_entry,
                    },
                    |log| async move {
                        log.emit(job::Resumed {
                            after: after.into(),
                        });
                        for (device, watching) in &update.watchers {
                            log.emit(watchdog::WatchdogSuspended {
                                device: device.clone(),
                                watching: watching.clone(),
                                safety_net: watchdog::SAFETY_NET.into(),
                            });
                        }
                        let rebooting_since = saved_job::instant(saved.saved_at);
                        log.keep(saved);
                        log.resume_in(state);
                        resume_update(&device, state, &update, rebooting_since, &log).await
                    },
                )
            }
        };
        if let Err(error) = started {
            log::warn!(
                "Cannot go on with the job of device {}: {error}",
                entry.device.0
            );
        }
    }
    for batch in batches {
        resume_batch(topology, &waiting, &batch).await;
    }
    // what is still waiting didn't go on: its entry is closed with the others
    let still_waiting: Vec<DeviceId> = saved_job::waiting()
        .iter()
        .map(|entry| entry.device)
        .collect();
    let resumed: Vec<u64> = waiting
        .iter()
        .filter(|entry| !still_waiting.contains(&entry.device))
        .filter_map(|entry| entry.saved.as_ref().ok()?.journal_entry)
        .collect();
    crate::journal::close_interrupted(&resumed).await;
}

/// A batch saved before the restart, its devices `batch` ([`resume_saved_jobs`]): all of
/// them saved and in NetBox, they go on together; else none does, and their fallbacks take
/// all of it back.
async fn resume_batch(
    topology: &TopologyHolder,
    waiting: &[saved_job::Waiting],
    batch: &[DeviceId],
) {
    let mut members = Vec::new();
    for id in batch {
        let saved = waiting
            .iter()
            .find(|entry| entry.device == *id)
            .and_then(|entry| entry.saved.as_ref().ok());
        let apply = saved.and_then(|saved| match &saved.work {
            SavedWork::Apply { apply, .. }
                if apply.batch.as_ref().is_some_and(|b| b.devices == batch) =>
            {
                Some(apply)
            }
            _ => None,
        });
        match (saved, apply, topology.devices_by_id(*id).await) {
            (Some(saved), Some(apply), Some(device)) => {
                members.push((device, saved.clone(), (**apply).clone()))
            }
            _ => {
                for id in batch {
                    saved_job::hold_back(*id, saved_job::HeldBack::BatchIncomplete);
                }
                return;
            }
        }
    }
    let Some(delay) = members
        .first()
        .and_then(|(_, _, apply)| apply.batch.as_ref())
        .map(|batch| batch.delay)
    else {
        return;
    };
    let fires = members
        .iter()
        .map(|(_, _, apply)| saved_job::instant(apply.script.fires))
        .min()
        .unwrap_or_else(std::time::Instant::now);
    for id in batch {
        saved_job::take(*id);
    }
    let devices: Vec<DeviceAccess> = members
        .iter()
        .map(|(device, _, _)| device.clone())
        .collect();
    let entries: Vec<Option<u64>> = members
        .iter()
        .map(|(_, saved, _)| saved.journal_entry)
        .collect();
    let started = job::start_all(
        &devices,
        JobKind::Apply,
        |index| Started::Resumed {
            entry: entries[index],
        },
        |logs| async move {
            for (log, (device, saved, apply)) in logs.iter().zip(&members) {
                let after = SystemTime::now()
                    .duration_since(saved.saved_at)
                    .unwrap_or_default();
                log.emit(job::Resumed {
                    after: after.into(),
                });
                if let Some(hold) = &apply.hold {
                    crate::device::radio_hold::RadioHold::announce_saved(device.name(), hold, log);
                }
                log.keep(saved.clone());
                log.resume_in(crate::device_state::ApplyState::ScriptRunning);
            }
            let members = members
                .into_iter()
                .map(|(device, _, apply)| (device, apply))
                .collect();
            crate::device::batch::resume(members, logs, fires, delay).await
        },
    );
    if let Err(error) = started {
        log::warn!("Cannot go on with the batch of devices {batch:?}: {error}");
    }
}

/// What an update with its fallback set saves (`update::update_keeping`).
pub(crate) fn saved_update(
    device: &DeviceAccess,
    credentials: &str,
    watchers: &[(Box<str>, watchdog::Watching)],
    run: &saved_job::UpdateRun,
) -> saved_job::SavedJob {
    use crate::topology::access::AccessTopology;
    saved_job::SavedJob {
        device: device.id(),
        device_name: device.name().into(),
        site: device.site().map(|site| site.id()),
        saved_at: SystemTime::now(),
        journal_entry: None,
        work: SavedWork::Update {
            state: UpdateState::Rebooting,
            update: Box::new(saved_job::SavedUpdate {
                credentials: Some(credentials.into()),
                watchers: watchers.to_vec(),
                run: run.clone(),
            }),
        },
    }
}

/// Connects to `device` until it answers or `until` passes, quietly between the tries (it may
/// be cut off or rebooting): `Connecting` once, `Connected` when it answers.
async fn connect_until(
    device: &DeviceAccess,
    credentials: Option<Box<str>>,
    until: std::time::Instant,
    log: &Log,
) -> Result<AccessibleDevice, Error> {
    log.emit(Connecting {
        device: device.name().into(),
    });
    loop {
        let reached = log
            .unless_cancelled(device.connect(DeviceTarget::named(credentials.clone())))
            .await?;
        match reached {
            Ok(Some(link)) => {
                log.emit(link.connected_device());
                return Ok(link);
            }
            Ok(None) => return Err(Error::MissingCredentials),
            Err(_) if std::time::Instant::now() < until => {
                log.unless_cancelled(tokio::time::sleep(RESUME_RETRY))
                    .await?;
            }
            Err(error) => return Err(error),
        }
    }
}

/// An apply saved before the restart ([`resume_saved_jobs`]): connects until the device
/// answers, the script may have cut it off for a while. Not back before the fallback would
/// have brought it back, it ends as an apply not reached does, the fallback left.
async fn resume_apply(
    device: &DeviceAccess,
    saved: &saved_job::SavedApply,
    log: &Log,
) -> Result<(), Error> {
    use crate::device::ros::apply::{COME_BACK, FallbackLeftReason, fallback_left, give_up};
    let fires = saved_job::instant(saved.script.fires);
    let link = match connect_until(device, saved.credentials.clone(), fires + COME_BACK, log).await
    {
        Ok(link) => link,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => {
            log.enter(crate::device_state::ApplyState::NotReached);
            if saved.script.armed && std::time::Instant::now() < give_up(fires) {
                fallback_left(log, fires, FallbackLeftReason::NotReached, None);
            }
            if let Some(hold) = &saved.hold {
                crate::device::radio_hold::RadioHold::leave_saved(device.name(), hold, log);
            }
            return Err(error);
        }
    };
    link.resume_apply(saved, log).await
}

/// An update saved before the restart ([`resume_saved_jobs`]): connects until the device
/// answers (it may be rebooting) or the reboot's wait is over, connects again to the devices
/// whose watchdogs it suspended, goes on ([`update::resume`]) and restores those at its end
/// (one not reached is left to its scheduler entry).
async fn resume_update(
    device: &DeviceAccess,
    state: UpdateState,
    saved: &saved_job::SavedUpdate,
    rebooting_since: std::time::Instant,
    log: &Log,
) -> Result<(), Error> {
    let mut suspended = Vec::new();
    for (name, watching) in &saved.watchers {
        let reached = match device
            .topology()
            .list_devices()
            .find(|d| d.name() == &**name)
        {
            Some(watcher) => connect_for_job(&watcher, DeviceTarget::default(), log).await,
            None => Err(Error::UnknownDeviceName(name.clone())),
        };
        match reached {
            Ok(link) => suspended.push(watchdog::Suspended {
                device: name.clone(),
                link,
                watching: watching.clone(),
            }),
            Err(error) => log.emit(watchdog::WatchdogNotRestored {
                device: name.clone(),
                watching: watching.clone(),
                error: (&error).into(),
            }),
        }
    }
    let power = PowerChain::of(device);
    let until = rebooting_since + saved.run.wait;
    let resumed = async {
        let link = connect_until(device, saved.credentials.clone(), until, log).await?;
        let credentials = saved.credentials.clone().unwrap_or_default();
        let keep = |run: &saved_job::UpdateRun| {
            log.keep(saved_update(device, &credentials, &saved.watchers, run))
        };
        let since = tokio::time::Instant::from_std(rebooting_since);
        update::resume(&link, &power, state, &saved.run, since, &keep, log).await
    }
    .await;
    restore_watchdogs(suspended, log, UpdateState::RestoringWatchdogs).await;
    resumed
}

/// How often a job going on after a restart tries to reach its device.
const RESUME_RETRY: std::time::Duration = std::time::Duration::from_secs(10);
