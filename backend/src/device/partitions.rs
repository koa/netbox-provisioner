//! Partitions of the NAND flash (`/partitions`), for the planned update fallback
//! (`docs/concepts/update-fallback.md`): a second partition keeps the RouterOS from before. A
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
        power::PowerChain,
        raw::{Row, command, field, rows},
        update::{DeviceLink, REBOOT_TIMEOUT, Reboot, RebootReason, read_state, wait_until_back},
    },
    job::Log,
};
use async_graphql::SimpleObject;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
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
    pub version: Box<str>,
    pub size_mib: u64,
    /// the one the device boots from
    pub active: bool,
    /// the one the device runs now
    pub running: bool,
    /// where RouterBOOT goes when this one doesn't boot (`next`, `etherboot`, a name)
    pub fallback_to: Box<str>,
}

/// The partitions of a device and whether [`repartition`] can split them.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Partitions {
    pub partitions: Vec<Partition>,
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
    pub partitions: Vec<Partition>,
    pub missing_files: Box<[Box<str>]>,
}

fn partition(row: &Row) -> Partition {
    let flag = |name| field(row, name) == Some("true");
    Partition {
        name: field(row, "name").unwrap_or_default().into(),
        version: field(row, "version").unwrap_or_default().into(),
        size_mib: field(row, "size").and_then(|s| s.parse().ok()).unwrap_or_default(),
        active: flag("active"),
        running: flag("running"),
        fallback_to: field(row, "fallback-to").unwrap_or_default().into(),
    }
}

/// The partitions of the device; empty without NAND for them (and on a CHR, which has no
/// `/partitions`).
pub async fn list(device: &RawDevice<SimpleResult>) -> Vec<Partition> {
    rows(device, "/partitions/print")
        .await
        .map(|rows| rows.iter().map(partition).collect())
        .unwrap_or_default()
}

/// Whether `partitions` can be split in [`COUNT`] (`allowed`: the device mode has
/// `partitions`).
fn can_split(partitions: &[Partition], allowed: bool) -> bool {
    allowed
        && partitions.len() == 1
        && partitions.iter().map(|p| p.size_mib).sum::<u64>() >= MIN_MIB
}

async fn allowed(device: &RawDevice<SimpleResult>) -> Result<bool, Error> {
    Ok(rows(device, "/system/device-mode/print")
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .is_some_and(|mode| field(&mode, "partitions") == Some("true")))
}

/// The partitions; `None` on a device without (a CHR, 16 MiB of flash).
pub async fn read(device: &RawDevice<SimpleResult>) -> Result<Option<Partitions>, Error> {
    let partitions = list(device).await;
    if partitions.is_empty() {
        return Ok(None);
    }
    let can_split = can_split(&partitions, allowed(device).await?);
    Ok(Some(Partitions {
        partitions,
        can_split,
    }))
}

/// The `.id` of partition `name`, for `number` of `activate` and `copy-to`.
pub(crate) async fn id_of(device: &RawDevice<SimpleResult>, name: &str) -> Result<Box<str>, Error> {
    rows(device, "/partitions/print")
        .await?
        .iter()
        .find(|row| field(row, "name") == Some(name))
        .and_then(|row| field(row, ".id"))
        .map(Box::from)
        .ok_or_else(|| Error::Refused(format!("no partition {name}").into()))
}

async fn files(device: &RawDevice<SimpleResult>) -> Result<Vec<Box<str>>, Error> {
    Ok(rows(device, "/file/print")
        .await?
        .iter()
        .filter_map(|row| field(row, "name").map(Box::from))
        .collect())
}

async fn identity(device: &RawDevice<SimpleResult>) -> Result<Box<str>, Error> {
    Ok(rows(device, "/system/identity/print")
        .await?
        .first()
        .and_then(|row| field(row, "name"))
        .unwrap_or_default()
        .into())
}

/// Splits the flash in [`COUNT`] partitions: the device reformats it and reboots, and is back
/// (reached as before, with the same credentials) with the running partition and an empty one.
/// `power`: what the device powers goes down with it.
pub async fn repartition(link: &impl DeviceLink, power: &PowerChain, log: &Log) -> Result<(), Error> {
    let device = link.api().await?;
    let partitions = list(&device).await;
    if !can_split(&partitions, allowed(&device).await?) {
        return Err(Error::CannotRepartition(partitions.len() as u32));
    }
    let before_files = files(&device).await?;
    let before_identity = identity(&device).await?;
    let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    let uptime = read_state(&device).await?.uptime;
    log.emit(RepartitionStarted {
        count: COUNT as u32,
        files: before_files.len() as u32,
    });
    let sent = Instant::now();
    let count = COUNT.to_string();
    // the answer, or the connection breaking with the reboot
    if let Ok(Err(error @ Error::Refused(_))) = timeout(
        Duration::from_secs(30),
        command(&device, "/partitions/repartition", &[("partitions", &count)]),
    )
    .await
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
            format!("identity {after_identity} after repartitioning, {before_identity} before").into(),
        ));
    }
    if partitions.len() != COUNT {
        return Err(Error::CannotRepartition(partitions.len() as u32));
    }
    log.emit(Repartitioned {
        partitions,
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
    let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    let uptime = read_state(&device).await?.uptime;
    let id = id_of(&device, name).await?;
    command(&device, "/partitions/activate", &[("number", &id)]).await?;
    let sent = Instant::now();
    // the answer, or the connection breaking with the reboot
    let _ = timeout(Duration::from_secs(10), command(&device, "/system/reboot", &[])).await;
    drop(device);
    log.emit(Reboot {
        reason: RebootReason::Partition,
        timeout: REBOOT_TIMEOUT.into(),
    });
    wait_until_back(link, log, uptime, sent, REBOOT_TIMEOUT, |_| Some(())).await?;
    drop(rebooting);
    let running = list(&link.api().await?).await.into_iter().find(|p| p.running);
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
    use super::{Partition, can_split, partition};

    #[test]
    fn reads_lab_cores_partition_and_splits_only_one() {
        // `/partitions/print` on lab-core
        let row = [
            ("name", "part0"),
            ("fallback-to", "next"),
            ("version", "RouterOS v7.23.8 2026-10-08 09:46:16"),
            ("size", "128"),
            ("active", "true"),
            ("running", "true"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), Some(v.into())))
        .collect();
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
    }
}
