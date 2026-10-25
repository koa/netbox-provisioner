//! The ping watchdogs an apply suspends while a radio it changes stays off
//! (`docs/concepts/watchdog.md`). On a DFS channel the AP listens for radar 10 minutes or longer
//! after a change ([`RADIO_RESTART`](super::ros::graphql::RADIO_RESTART)), as long as
//! `ping-timeout`: the station, and whoever reaches its parent through the link, would reboot
//! meanwhile, and the fallback of a rebooted device counts its delay anew.
//!
//! So an apply changing a radio suspends the `watch-address` of the device itself and of the
//! RouterOS devices depending on it on the way to the management roots
//! (`ManagementTree::depending_on`), as an update does ([`watchdog::suspend`], with its
//! scheduler entry restoring them after [`SAFETY_NET`]). A PoE ping stays: a port only pings
//! the device on its own cable, which stays reachable.
//!
//! The device's own stays as the device has it also where it watches nothing yet, else the script
//! would set NetBox' address while the radio is off. Reached again (or nothing applied), the
//! apply restores them: the device's own as NetBox has it, where it answers, else as it was (the
//! rule of `keep_silent_watchdogs`). While the
//! fallback may still fire (not reached, cancelled after the script), they stay suspended for
//! their scheduler entry to restore ([`WatchdogKept`]): the old config's radio may listen
//! again.
use super::{
    AccessibleDevice,
    ros::graphql::PlannedApply,
    watchdog::{
        self, SAFETY_NET, Suspended, WatchdogLeft, WatchdogNotRestored, WatchdogRestored,
        WatchdogSuspended, WatcherLink, Watching,
    },
};
use crate::{
    Error,
    graphql::{connect_for_job, scalars::ScalarDuration},
    job::Log,
    saved_job::SavedHold,
    topology::{DeviceId, access::device::DeviceAccess, natural::naturalize},
};
use async_graphql::SimpleObject;
use mikrotik_api::simple::SimpleResult;
use std::{collections::BTreeSet, sync::Mutex};

/// The watchdog of `device` stays suspended while the fallback may still fire; its scheduler
/// entry restores `watching` after `safety_net`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogKept {
    pub device: Box<str>,
    pub watching: Watching,
    pub safety_net: ScalarDuration,
}

/// What an apply changing a radio suspended.
pub(crate) struct RadioHold {
    device: Box<str>,
    /// the applied device's own, as it was
    own: Option<Watching>,
    others: Vec<Suspended<AccessibleDevice>>,
}

impl RadioHold {
    /// Suspends the watchdog of `link` (and has `planned` and the checks after it keep it
    /// suspended) and those of the devices depending on it, but for the ones `applied` (applied
    /// with it, they hold their own) and those another device of the batch `claimed` already.
    pub(crate) async fn take(
        link: &AccessibleDevice,
        planned: &mut PlannedApply,
        applied: &BTreeSet<DeviceId>,
        claimed: &Mutex<BTreeSet<DeviceId>>,
        log: &Log,
    ) -> Result<RadioHold, Error> {
        let device: Box<str> = link.device_config.name().into();
        let own = async {
            let api = link.connect_again::<SimpleResult>().await?;
            let watching = watchdog::read(&api, &[]).await?;
            watchdog::suspend(&api, &watching, SAFETY_NET).await?;
            Ok::<_, Error>(watching)
        }
        .await;
        // also where it watches nothing yet: the script would set the watch address NetBox
        // has (lab-sxt-a/b had none), and the watchdog would run while the radio is off
        let own = match own {
            Ok(watching) => {
                link.hold_watchdog(true);
                planned.hold_watchdog(log)?;
                if !watching.is_empty() {
                    log.emit(WatchdogSuspended {
                        device: device.clone(),
                        watching: watching.clone(),
                        safety_net: SAFETY_NET.into(),
                    });
                }
                Some(watching)
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error) => {
                log.emit(WatchdogLeft {
                    device: device.clone(),
                    error: (&error).into(),
                });
                None
            }
        };
        let mut hold = RadioHold {
            device,
            own,
            others: Vec::new(),
        };
        let mut links = Vec::new();
        for watcher in depending(&link.device_config, applied, claimed) {
            let reached = connect_for_job(&watcher, None, None, None, log).await;
            if matches!(reached, Err(Error::Cancelled)) {
                hold.release(link, log).await;
                return Err(Error::Cancelled);
            }
            links.push(WatcherLink {
                device: watcher.name().into(),
                supplier: false,
                powered: Vec::new(),
                link: reached,
            });
        }
        match watchdog::suspend_all(links, log).await {
            Ok(others) => hold.others = others,
            Err(error) => {
                hold.release(link, log).await;
                return Err(error);
            }
        }
        Ok(hold)
    }

    /// Restores what [`Self::take`] suspended: the device's own as NetBox has it, where that
    /// answers, else as it was.
    pub(crate) async fn release(self, link: &AccessibleDevice, log: &Log) {
        if let Some(old) = self.own {
            let restored = async {
                let api = link.connect_again::<SimpleResult>().await?;
                let watching = link.watch_after_hold(&api, &old).await?;
                watchdog::restore(&api, &watching).await?;
                Ok::<_, Error>(watching)
            }
            .await;
            link.hold_watchdog(false);
            match restored {
                Ok(watching) if watching.is_empty() => {}
                Ok(watching) => log.emit(WatchdogRestored {
                    device: self.device,
                    watching,
                }),
                Err(error) => log.emit(WatchdogNotRestored {
                    device: self.device,
                    watching: old,
                    error: (&error).into(),
                }),
            }
        }
        watchdog::restore_all(self.others, log).await;
    }

    /// What it holds, to hold it again after a restart (`saved_job`).
    pub(crate) fn saved(&self) -> SavedHold {
        SavedHold {
            own: self.own.clone(),
            others: self.others.iter().map(|s| (s.device.clone(), s.watching.clone())).collect(),
        }
    }

    /// Holds again what an apply saved before a restart held: suspended still (says
    /// `WatchdogSuspended` again, so the other devices' pages show them held), each other
    /// device connected again for restoring it; one not reached is left to its scheduler
    /// entry (`WatchdogKept`).
    pub(crate) async fn resumed(link: &AccessibleDevice, saved: &SavedHold, log: &Log) -> RadioHold {
        let device: Box<str> = link.device_config.name().into();
        if let Some(watching) = &saved.own {
            link.hold_watchdog(true);
            if !watching.is_empty() {
                log.emit(WatchdogSuspended {
                    device: device.clone(),
                    watching: watching.clone(),
                    safety_net: SAFETY_NET.into(),
                });
            }
        }
        let topology = link.device_config.topology();
        let mut others = Vec::new();
        for (name, watching) in &saved.others {
            let reached = match topology.list_devices().find(|d| d.name() == &**name) {
                Some(watcher) => connect_for_job(&watcher, None, None, None, log).await.ok(),
                None => None,
            };
            match reached {
                Some(watcher) => {
                    log.emit(WatchdogSuspended {
                        device: name.clone(),
                        watching: watching.clone(),
                        safety_net: SAFETY_NET.into(),
                    });
                    others.push(Suspended {
                        device: name.clone(),
                        link: watcher,
                        watching: watching.clone(),
                    });
                }
                None => log.emit(WatchdogKept {
                    device: name.clone(),
                    watching: watching.clone(),
                    safety_net: SAFETY_NET.into(),
                }),
            }
        }
        RadioHold {
            device,
            own: saved.own.clone(),
            others,
        }
    }

    /// Leaves what an apply saved before a restart held to the scheduler entries
    /// (`WatchdogKept`), the device not reached again.
    pub(crate) fn leave_saved(device: &str, saved: &SavedHold, log: &Log) {
        let own = saved.own.iter().filter(|watching| !watching.is_empty()).map(|watching| (device.into(), watching));
        let others = saved.others.iter().map(|(device, watching)| (device.clone(), watching));
        for (device, watching) in own.chain(others) {
            log.emit(WatchdogKept {
                device,
                watching: watching.clone(),
                safety_net: SAFETY_NET.into(),
            });
        }
    }

    /// Leaves what [`Self::take`] suspended to the scheduler entries (`WatchdogKept`).
    pub(crate) fn leave(self, log: &Log) {
        let own = self.own.filter(|watching| !watching.is_empty()).map(|watching| (self.device, watching));
        let others = self.others.into_iter().map(|s| (s.device, s.watching));
        for (device, watching) in own.into_iter().chain(others) {
            log.emit(WatchdogKept {
                device,
                watching,
                safety_net: SAFETY_NET.into(),
            });
        }
    }
}

/// The RouterOS devices depending on `device` on the way to the management roots, by name, but
/// for the `applied` ones and the ones `claimed` (which they now are).
fn depending(
    device: &DeviceAccess,
    applied: &BTreeSet<DeviceId>,
    claimed: &Mutex<BTreeSet<DeviceId>>,
) -> Vec<DeviceAccess> {
    let topology = device.topology();
    let mut claimed = claimed.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut depending: Vec<DeviceAccess> = crate::graphql::management_tree(device)
        .depending_on(device.id())
        .into_iter()
        .filter(|id| !applied.contains(id))
        .filter_map(|id| topology.get_device_by_id(&id))
        .filter(DeviceAccess::has_routeros)
        .filter(|watcher| claimed.insert(watcher.id()))
        .collect();
    depending.sort_by_cached_key(|watcher| naturalize(watcher.name()));
    depending
}
