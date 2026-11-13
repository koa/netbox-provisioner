//! Scope: The NAND partitions: reading, splitting, copying to the other one and booting from it;
//! not the update fallback using them (`update_fallback`).
//!
//! Partitions of the NAND flash (`/partitions`), for the planned update fallback
//! (`docs/update-fallback.md`): a second partition keeps the RouterOS from before. A
//! device with at least 128 MiB of NAND lists its one partition there; a CHR has no such menu, a
//! 16 MiB device lists none. Splitting needs `partitions` in the device mode
//! ([`device_mode`](super::device_mode)).
//!
//! `/partitions repartition` reboots the device and reformats the NAND, "leaving only active
//! partition" (MikroTik's documentation, which says no more). So [`repartition`] notes what it
//! can check before (identity, files) and compares after the reboot; the job's events show what
//! went missing.
use crate::{
    Error,
    device::{
        access::act,
        dependency::PowerChain,
        reboot::{REBOOT_TIMEOUT, Reboot, RebootReason, wait_until_back},
        update::{DeviceLink, read_state},
    },
    device_state::RebootState,
    error_info::ErrorInfo,
    job::Log,
};
use async_graphql::SimpleObject;
use mikrotik_model::{
    action::{
        PartitionActivate, PartitionCopyTo, PartitionSaveConfigTo, Reboot as RebootNow, Repartition,
    },
    command::{Reads, Writes},
    model::{File, PartitionsState, SystemDeviceModeState, SystemIdentityCfg},
    resource::{SingleResource, collect_resource},
    value::RosValue,
};
use std::time::Duration;
use tokio::time::{Instant, timeout};

/// The flash RouterOS repartitions: at least this many MiB (from 7.20 on).
pub const MIN_MIB: u64 = 128;
/// Partitions [`repartition`] makes: the running one and one for the RouterOS from before an
/// update (two of 64 MiB on 128 MiB, the smallest RouterOS allows is 60).
pub const COUNT: usize = 2;

/// A partition, as `/partitions/print` lists it.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Partition {
    pub name: Box<str>,
    /// the RouterOS on it, e.g. `RouterOS v7.23.8 2026-10-08 09:46:16`; empty when none
    /// (RouterOS reports that as `EMPTY` or nothing)
    pub version: Box<str>,
    pub size_mib: u64,
    /// the one the device boots from
    pub active: bool,
    /// the one the device runs now
    pub running: bool,
    /// where RouterBOOT goes when this one doesn't boot (`next`, `etherboot`, a name)
    pub fallback_to: Box<str>,
}

/// Copying RouterOS between partitions: a few tens of MiB on NAND.
const COPY_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The partitions of a device and whether [`repartition`] can split them.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Partitions {
    pub partitions: Box<[Partition]>,
    /// one partition on enough flash, and the device mode allows partitions
    pub can_split: bool,
}

/// `/partitions repartition` is sent: the device reformats its flash and reboots.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RepartitionStarted {
    pub count: u32,
    /// the files on the device before, to compare with afterwards
    pub files: u32,
}

/// The device is back with these partitions; `missing_files`: files there before that are gone.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Repartitioned {
    pub partitions: Box<[Partition]>,
    pub missing_files: Box<[Box<str>]>,
}

fn partition(read: &PartitionsState) -> Partition {
    Partition {
        name: read.name.to_string().into(),
        version: read
            .version
            .as_ref()
            .map(ToString::to_string)
            .filter(|version| version != "EMPTY")
            .unwrap_or_default()
            .into(),
        size_mib: read.size.unwrap_or_default(),
        active: read.active == Some(true),
        running: read.running == Some(true),
        fallback_to: read
            .fallback_to
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
            .into(),
    }
}

/// The partitions of the device; empty without NAND for them (and on a CHR, which has no
/// `/partitions`).
pub async fn list(device: &impl Reads) -> Vec<Partition> {
    collect_resource::<PartitionsState>(device)
        .await
        .map(|read| read.iter().map(partition).collect())
        .unwrap_or_default()
}

/// Whether `partitions` can be split in [`COUNT`] (`allowed`: the device mode has
/// `partitions`).
fn can_split(partitions: &[Partition], allowed: bool) -> bool {
    allowed
        && partitions.len() == 1
        && partitions.iter().map(|p| p.size_mib).sum::<u64>() >= MIN_MIB
}

async fn allowed(device: &impl Reads) -> Result<bool, Error> {
    Ok(SystemDeviceModeState::fetch(device)
        .await
        .ok()
        .flatten()
        .is_some_and(|mode| mode.partitions == Some(true)))
}

/// The partitions; `None` on a device without (a CHR, 16 MiB of flash).
pub async fn read(device: &impl Reads) -> Result<Option<Partitions>, Error> {
    let partitions = list(device).await;
    if partitions.is_empty() {
        return Ok(None);
    }
    let can_split = can_split(&partitions, allowed(device).await?);
    Ok(Some(Partitions {
        partitions: partitions.into(),
        can_split,
    }))
}

/// The partition to copy to: the other one of two, while the device runs the active one.
pub(crate) fn other(partitions: &[Partition]) -> Option<&Partition> {
    match partitions {
        [a, b] if a.running && a.active => Some(b),
        [a, b] if b.running && b.active => Some(a),
        _ => None,
    }
}

/// `/partitions copy-to`: the running RouterOS with its configuration to `target`.
pub(crate) async fn copy_to(device: &impl Writes, target: &str) -> Result<(), Error> {
    to_partition(device, ToPartition::CopyTo, target).await
}

/// What [`to_partition`] puts on another partition.
#[derive(Debug, Clone, Copy)]
enum ToPartition {
    /// `copy-to`: RouterOS with its configuration
    CopyTo,
    /// `save-config-to`: the configuration alone
    SaveConfigTo,
}

/// `copy-to` or `save-config-to` with `target` as `number`. It runs like a monitor: it answers
/// while it copies and is done at the end.
async fn to_partition(device: &impl Writes, what: ToPartition, target: &str) -> Result<(), Error> {
    let id = id_of(device, target).await?;
    // copying slow flash may stay silent for a while; the copy's own limit holds
    let copied = timeout(COPY_TIMEOUT, async {
        match what {
            ToPartition::CopyTo => {
                let copy = PartitionCopyTo {
                    target: &id,
                    reply_timeout: COPY_TIMEOUT,
                };
                act(device, &copy).await
            }
            ToPartition::SaveConfigTo => {
                let save = PartitionSaveConfigTo {
                    target: &id,
                    reply_timeout: COPY_TIMEOUT,
                };
                act(device, &save).await
            }
        }
    })
    .await;
    let rows = copied
        .map_err(|_| Error::Refused(format!("{what:?} {target} not done in time").into()))??;
    for row in rows {
        log::info!("{what:?} {target}: {row:?}");
    }
    Ok(())
}

/// After an apply the other partition holds what the device runs now: with the same RouterOS
/// only its configuration (`config_only`), else a copy of all, which overwrites a RouterOS from
/// before an update (Z28). `partition` is `None` where the device couldn't be asked.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PartitionSynced {
    pub partition: Option<Box<str>>,
    pub config_only: bool,
    pub error: Option<ErrorInfo>,
}

/// Brings the other of two partitions to what the device runs (`PartitionSynced`), so a device
/// that boots it by itself (RouterBOOT takes it after a boot it thinks failed, as on unsteady
/// power) runs the same configuration. Nothing where there is no other partition.
pub(crate) async fn sync_other(device: &impl Writes, log: &Log) {
    let list = list(device).await;
    let Some(ToSync {
        partition,
        config_only,
    }) = to_sync(&list)
    else {
        return;
    };
    let running = list.iter().find(|p| p.running).map(|p| p.version.clone());
    let synced = if config_only {
        to_partition(device, ToPartition::SaveConfigTo, &partition).await
    } else {
        sync_copy(device, &partition, running).await
    };
    log.emit(PartitionSynced {
        partition: Some(partition),
        config_only,
        error: synced.err().as_ref().map(ErrorInfo::from),
    });
}

/// The partition [`sync_other`] brings in line.
#[derive(Debug, PartialEq)]
struct ToSync {
    partition: Box<str>,
    /// only its configuration: it holds the RouterOS the device runs
    config_only: bool,
}

/// The partition [`sync_other`] brings in line, if any.
fn to_sync(partitions: &[Partition]) -> Option<ToSync> {
    let target = other(partitions)?;
    let running = partitions.iter().find(|p| p.running)?;
    let config_only = !target.version.is_empty() && target.version == running.version;
    Some(ToSync {
        partition: target.name.clone(),
        config_only,
    })
}

/// [`copy_to`] `target`, which then holds the `running` RouterOS.
async fn sync_copy(
    device: &impl Writes,
    target: &str,
    running: Option<Box<str>>,
) -> Result<(), Error> {
    copy_to(device, target).await?;
    let copied = list(device).await;
    let version = copied
        .iter()
        .find(|p| p.name.as_ref() == target)
        .map(|p| &p.version);
    match version {
        Some(version) if !version.is_empty() && Some(version) == running.as_ref() => Ok(()),
        _ => Err(Error::Refused(
            format!("{target} holds no copy of the running RouterOS after copy-to").into(),
        )),
    }
}

/// The `.id` of partition `name`, for `number` of `activate` and `copy-to`.
pub(crate) async fn id_of(device: &impl Reads, name: &str) -> Result<Box<str>, Error> {
    collect_resource::<PartitionsState>(device)
        .await?
        .iter()
        .find(|partition| *partition.name.0 == *name.as_bytes())
        .map(|partition| String::from_utf8_lossy(&partition.id.encode_ros()).into())
        .ok_or_else(|| Error::Refused(format!("no partition {name}").into()))
}

async fn files(device: &impl Reads) -> Result<Vec<Box<str>>, Error> {
    Ok(collect_resource::<File>(device)
        .await?
        .iter()
        .map(|file| file.cfg.name.to_string().into())
        .collect())
}

async fn identity(device: &impl Reads) -> Result<Box<str>, Error> {
    Ok(SystemIdentityCfg::fetch(device)
        .await?
        .map(|identity| identity.name.to_string().into())
        .unwrap_or_default())
}

/// Splits the flash in [`COUNT`] partitions: the device reformats it and reboots, and is back
/// (reached as before, with the same credentials) with the running partition and an empty one.
/// `power`: what the device powers goes down with it.
pub async fn repartition(
    link: &impl DeviceLink,
    power: &PowerChain,
    log: &Log,
) -> Result<(), Error> {
    let device = link.api().await?;
    let partitions = list(&device).await;
    if !can_split(&partitions, allowed(&device).await?) {
        return Err(Error::CannotRepartition(partitions.len() as u32));
    }
    let before_files = files(&device).await?;
    let before_identity = identity(&device).await?;
    log.enter(RebootState::WaitingForChain);
    let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    log.enter(RebootState::Rebooting);
    let uptime = read_state(&device).await?.uptime;
    log.emit(RepartitionStarted {
        count: COUNT as u32,
        files: before_files.len() as u32,
    });
    let sent = Instant::now();
    let split = Repartition {
        partitions: COUNT as u8,
    };
    // the answer, or the connection breaking with the reboot
    if let Ok(Err(error @ Error::Refused(_))) =
        timeout(Duration::from_secs(30), act(&device, &split)).await
    {
        return Err(error);
    }
    drop(device);
    log.emit(Reboot {
        reason: RebootReason::Repartition,
        timeout: REBOOT_TIMEOUT.into(),
    });
    wait_until_back(link, log, uptime, sent, REBOOT_TIMEOUT, |_| Some(())).await?;
    drop(rebooting);
    let device = link.api().await?;
    let partitions = list(&device).await;
    let after_files = files(&device).await?;
    let missing_files = before_files
        .into_iter()
        .filter(|file| !after_files.contains(file))
        .collect();
    let after_identity = identity(&device).await?;
    if after_identity != before_identity {
        return Err(Error::Refused(
            format!("identity {after_identity} after repartitioning, {before_identity} before")
                .into(),
        ));
    }
    if partitions.len() != COUNT {
        return Err(Error::CannotRepartition(partitions.len() as u32));
    }
    log.emit(Repartitioned {
        partitions: partitions.into(),
        missing_files,
    });
    Ok(())
}

/// The device is back, running `name` with `version`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PartitionRunning {
    pub name: Box<str>,
    pub version: Box<str>,
}

/// Boots the device from `name`, a partition with a RouterOS on it (`/partitions activate`, a
/// reboot), e.g. back to the RouterOS from before an update. The partition booted from before
/// stays as it is.
pub async fn boot_from(
    link: &impl DeviceLink,
    power: &PowerChain,
    name: &str,
    log: &Log,
) -> Result<(), Error> {
    let device = link.api().await?;
    let partition = list(&device)
        .await
        .into_iter()
        .find(|p| p.name.as_ref() == name && !p.version.is_empty())
        .ok_or_else(|| Error::Refused(format!("no partition {name} with a RouterOS").into()))?;
    if partition.running {
        log.emit(PartitionRunning {
            name: partition.name,
            version: partition.version,
        });
        return Ok(());
    }
    log.enter(RebootState::WaitingForChain);
    let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    log.enter(RebootState::Rebooting);
    let uptime = read_state(&device).await?.uptime;
    let id = id_of(&device, name).await?;
    act(&device, &PartitionActivate { number: &id }).await?;
    let sent = Instant::now();
    // the answer, or the connection breaking with the reboot
    let _ = timeout(Duration::from_secs(10), act(&device, &RebootNow)).await;
    drop(device);
    log.emit(Reboot {
        reason: RebootReason::Partition,
        timeout: REBOOT_TIMEOUT.into(),
    });
    wait_until_back(link, log, uptime, sent, REBOOT_TIMEOUT, |_| Some(())).await?;
    drop(rebooting);
    let running = list(&link.api().await?)
        .await
        .into_iter()
        .find(|p| p.running);
    match running {
        Some(running) if running.name.as_ref() == name => {
            log.emit(PartitionRunning {
                name: running.name,
                version: running.version,
            });
            Ok(())
        }
        running => Err(Error::Refused(
            format!(
                "runs {} after activating {name}",
                running.map_or("no partition".into(), |p| p.name)
            )
            .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{Partition, ToSync, can_split, other, partition, to_sync};
    use crate::device::answer::parsed;
    use mikrotik_model::model::PartitionsState;

    fn named(name: &str, active: bool, running: bool) -> Partition {
        Partition {
            name: name.into(),
            version: "".into(),
            size_mib: 64,
            active,
            running,
            fallback_to: "next".into(),
        }
    }

    #[test]
    fn copies_to_the_other_of_two() {
        let part0 = named("part0", true, true);
        let part1 = named("part1", false, false);
        assert_eq!(other(&[part0.clone(), part1.clone()]), Some(&part1));
        assert_eq!(other(&[part1.clone(), part0.clone()]), Some(&part1));
        // one partition, or the device runs another than it boots from: no other to copy to
        assert_eq!(other(std::slice::from_ref(&part0)), None);
        assert_eq!(
            other(&[named("part0", false, true), named("part1", true, false)]),
            None
        );
    }

    #[test]
    fn syncs_the_config_of_the_same_routeros_else_all() {
        let with = |name, active, version: &str| Partition {
            version: version.into(),
            ..named(name, active, active)
        };
        let v24 = "RouterOS v7.24.6 2026-10-30 10:12:00";
        let v23 = "RouterOS v7.23.8 2026-10-08 09:46:16";
        assert_eq!(
            to_sync(&[with("part0", true, v24), with("part1", false, v24)]),
            Some(ToSync {
                partition: "part1".into(),
                config_only: true
            })
        );
        // the RouterOS from before an update, or none: all of it
        assert_eq!(
            to_sync(&[with("part0", false, v23), with("part1", true, v24)]),
            Some(ToSync {
                partition: "part0".into(),
                config_only: false
            })
        );
        assert_eq!(
            to_sync(&[with("part0", true, v24), with("part1", false, "")]),
            Some(ToSync {
                partition: "part1".into(),
                config_only: false
            })
        );
        // one partition, or the device runs one it doesn't boot from: nothing
        assert_eq!(to_sync(&[with("part0", true, v24)]), None);
        assert_eq!(to_sync(&[]), None);
    }

    #[test]
    fn reads_lab_cores_partition_and_splits_only_one() {
        // `/partitions/print` on lab-core
        let row: PartitionsState = parsed(&[
            (".id", "*1"),
            ("name", "part0"),
            ("fallback-to", "next"),
            ("version", "RouterOS v7.23.8 2026-10-08 09:46:16"),
            ("size", "128"),
            ("active", "true"),
            ("running", "true"),
        ]);
        let part0 = partition(&row);
        assert_eq!(
            part0,
            Partition {
                name: "part0".into(),
                version: "RouterOS v7.23.8 2026-10-08 09:46:16".into(),
                size_mib: 128,
                active: true,
                running: true,
                fallback_to: "next".into(),
            }
        );
        assert!(can_split(std::slice::from_ref(&part0), true));
        // not without the device mode, and not twice
        assert!(!can_split(std::slice::from_ref(&part0), false));
        assert!(!can_split(&[part0.clone(), part0], true));
        // 16 MiB devices list none
        assert!(!can_split(&[], true));
        // a partition without RouterOS, as a C52iG-5HaxD2HaxD on 7.23.8 lists it
        let row: PartitionsState = parsed(&[
            (".id", "*2"),
            ("name", "part1"),
            ("version", "EMPTY"),
            ("size", "64"),
        ]);
        assert_eq!(partition(&row).version.as_ref(), "");
    }
}
