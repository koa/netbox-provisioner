//! The way back from a RouterOS update (`docs/concepts/update-fallback.md`, stage 1): on a
//! device with two [`partitions`](super::partitions), the running RouterOS is copied to the
//! other one with its configuration (`/partitions copy-to`) before the update. A scheduler run
//! at every boot then waits [`Settings::update_fallback`](crate::config::Settings) and, unless
//! the provisioner removed it in the meantime, activates the old partition and reboots. It is
//! added after the copy, so the old partition doesn't carry it. A partition that doesn't boot
//! at all RouterBOOT leaves by itself (`fallback-to=next`).
//!
//! The update counts as done when the device is back with the version and the OSPF neighbors
//! it was adjacent with before are again ([`ospf_up`], [`missing`]).
use crate::{
    Error,
    device::{
        partitions::{self, Partition},
        raw::{Row, command, field, rows},
    },
    graphql::scalars::ScalarDuration,
    job::Log,
};
use async_graphql::{SimpleObject, futures_util::StreamExt};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::{Instant, sleep, timeout};

/// The name of the scheduler entry.
const NAME: &str = "netbox-provisioner-update-fallback";
/// Copying RouterOS between partitions: a few tens of MiB on NAND.
const COPY_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// OSPF takes its dead interval and the exchange to get adjacent after a boot.
pub const OSPF_TIMEOUT: Duration = Duration::from_secs(3 * 60);

/// The running RouterOS and its configuration are on `partition`, and the device switches back
/// to it `delay` after a boot unless the provisioner reaches it.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateFallbackArmed {
    pub partition: Box<str>,
    pub version: Box<str>,
    pub delay: ScalarDuration,
}

/// The device runs the update and is reached: the scheduler is removed, `partition` stays as
/// the way back until the next update.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateFallbackDisarmed {
    pub partition: Box<str>,
}

/// The device updates without a way back: it has no second partition, or the device mode
/// doesn't allow partitions.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateWithoutFallback {
    pub partitions: u32,
}

/// The OSPF neighbors this device was adjacent with before the update are again.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct OspfNeighborsBack {
    pub neighbors: Box<[Box<str>]>,
}

/// The partition to copy to: the other one of two, while the device runs the active one.
fn other(partitions: &[Partition]) -> Option<&Partition> {
    match partitions {
        [a, b] if a.running && a.active => Some(b),
        [a, b] if b.running && b.active => Some(a),
        _ => None,
    }
}

/// Copies the running RouterOS to the other partition and schedules switching to it, where
/// the device has two partitions; `None` (and [`UpdateWithoutFallback`]) where not.
pub async fn arm(
    device: &RawDevice<SimpleResult>,
    delay: Duration,
    log: &Log,
) -> Result<Option<Box<str>>, Error> {
    let list = partitions::list(device).await;
    let Some(target) = other(&list).map(|p| p.name.clone()) else {
        log.emit(UpdateWithoutFallback {
            partitions: list.len() as u32,
        });
        return Ok(None);
    };
    // an entry left from an update the provisioner didn't see to the end
    remove_scheduler(device).await?;
    copy_to(device, &target).await?;
    let copied = partitions::list(device).await;
    let running = copied.iter().find(|p| p.running).map(|p| p.version.clone());
    let version = copied
        .iter()
        .find(|p| p.name == target)
        .map(|p| p.version.clone())
        .filter(|version| !version.is_empty() && Some(version) == running.as_ref())
        .ok_or_else(|| {
            Error::Refused(format!("{target} holds no copy of the running RouterOS after copy-to").into())
        })?;
    let on_event = script(&target, delay);
    command(
        device,
        "/system/scheduler/add",
        &[
            ("name", NAME),
            ("start-time", "startup"),
            ("on-event", &on_event),
            ("comment", "boots the RouterOS from before the update, unless the provisioner stops it"),
        ],
    )
    .await?;
    log.emit(UpdateFallbackArmed {
        partition: target.clone(),
        version,
        delay: delay.into(),
    });
    Ok(Some(target))
}

/// The scheduler's script: it checks its own entry after the delay, since removing a scheduler
/// doesn't stop a script it runs, and removes it before switching, so the new partition doesn't
/// switch back again should it be activated later.
fn script(target: &str, delay: Duration) -> String {
    format!(
        ":delay {}s; \
         :if ([:len [/system scheduler find name=\"{NAME}\"]] > 0) do={{ \
         /system scheduler remove [find name=\"{NAME}\"]; \
         /partitions activate [find name=\"{target}\"]; \
         /system reboot }}",
        delay.as_secs().max(1)
    )
}

/// `/partitions copy-to` runs like a monitor: it answers while it copies and is done at the end.
async fn copy_to(device: &RawDevice<SimpleResult>, target: &str) -> Result<(), Error> {
    let path = "/partitions/copy-to";
    let id = partitions::id_of(device, target).await?;
    // copying slow flash may stay silent for a while; the copy's own limit holds
    let mut stream = device
        .send_command_with_reply_timeout(
            path.as_bytes(),
            |cmd| cmd.attribute(b"number", id.as_bytes()),
            (),
            Some(COPY_TIMEOUT),
        )
        .await;
    let copied = timeout(COPY_TIMEOUT, async {
        while let Some(result) = stream.next().await {
            match result {
                SimpleResult::Sentence(row) => log::info!("{path} {target}: {row:?}"),
                SimpleResult::Error(error) => return Err(error.into()),
                SimpleResult::Trap { message, .. } => {
                    return Err(Error::Refused(format!("{path}: {message}").into()));
                }
            }
        }
        Ok(())
    })
    .await;
    copied.map_err(|_| Error::Refused(format!("{path} {target} not done in time").into()))?
}

/// Removes the scheduler: the device is reached with the update. The old partition stays.
pub async fn disarm(device: &RawDevice<SimpleResult>, partition: &str, log: &Log) -> Result<(), Error> {
    remove_scheduler(device).await?;
    log.emit(UpdateFallbackDisarmed {
        partition: partition.into(),
    });
    Ok(())
}

async fn remove_scheduler(device: &RawDevice<SimpleResult>) -> Result<(), Error> {
    let ids = rows(device, "/system/scheduler/print")
        .await?
        .iter()
        .filter(|row| field(row, "name") == Some(NAME))
        .filter_map(|row| field(row, ".id").map(String::from))
        .collect::<Vec<_>>();
    for id in ids {
        command(device, "/system/scheduler/remove", &[(".id", &id)]).await?;
    }
    Ok(())
}

/// The partition the device runs; `None` without partitions.
pub async fn running(device: &RawDevice<SimpleResult>) -> Option<Box<str>> {
    partitions::list(device)
        .await
        .into_iter()
        .find(|p| p.running)
        .map(|p| p.name)
}

/// The OSPF neighbors this device is adjacent with (see [`adjacent`]), as `<instance>
/// <router-id>` (an instance per OSPF version); empty without OSPF.
pub async fn ospf_up(device: &RawDevice<SimpleResult>) -> Result<BTreeSet<Box<str>>, Error> {
    Ok(adjacent(&rows(device, "/routing/ospf/neighbor/print").await?))
}

/// The neighbors in `rows` (`/routing/ospf/neighbor`) that are where OSPF leaves them: `Full`,
/// or `TwoWay` where the DR and the BDR are two others on the segment. Two DROthers stay in
/// `TwoWay` for good, and a reboot elects anew, so one that was `Full` as DR may end there
/// (berghof-stall-haus, 7.23.8). v2 names the DR by its address, v3 by its router ID.
fn adjacent(rows: &[Row]) -> BTreeSet<Box<str>> {
    let same_segment = |a: &Row, b: &Row| {
        field(a, "instance") == field(b, "instance") && field(a, "interface") == field(b, "interface")
    };
    // `id` (the DR or BDR) is none, or a neighbor of the segment other than `row`
    let another = |row: &Row, id: Option<&str>| match id {
        None | Some("" | "0.0.0.0") => true,
        Some(id) => rows.iter().any(|other| {
            !std::ptr::eq(other, row)
                && same_segment(other, row)
                && (field(other, "address") == Some(id) || field(other, "router-id") == Some(id))
        }),
    };
    rows
        .iter()
        .filter(|row| match field(row, "state") {
            Some("Full") => true,
            Some("TwoWay") => another(row, field(row, "dr")) && another(row, field(row, "bdr")),
            _ => false,
        })
        .map(|row| {
            format!(
                "{} {}",
                field(row, "instance").unwrap_or_default(),
                field(row, "router-id").unwrap_or_default()
            )
            .into()
        })
        .collect()
}

/// What of `before` isn't in `now`.
pub fn missing(before: &BTreeSet<Box<str>>, now: &BTreeSet<Box<str>>) -> Vec<Box<str>> {
    before.difference(now).cloned().collect()
}

/// Waits (up to [`OSPF_TIMEOUT`]) until the neighbors `before` are adjacent again; the ones that
/// aren't by then.
pub async fn wait_for_ospf(
    device: &RawDevice<SimpleResult>,
    before: &BTreeSet<Box<str>>,
    log: &Log,
) -> Result<Vec<Box<str>>, Error> {
    let deadline = Instant::now() + OSPF_TIMEOUT;
    loop {
        let missing = missing(before, &ospf_up(device).await?);
        if missing.is_empty() || Instant::now() > deadline {
            return Ok(missing);
        }
        log.unless_cancelled(sleep(Duration::from_secs(5))).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::{adjacent, missing, other, script};
    use crate::device::raw::Row;
    use std::{collections::BTreeSet, time::Duration};
    use crate::device::partitions::Partition;

    fn partition(name: &str, active: bool, running: bool) -> Partition {
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
        let part0 = partition("part0", true, true);
        let part1 = partition("part1", false, false);
        assert_eq!(other(&[part0.clone(), part1.clone()]), Some(&part1));
        assert_eq!(other(&[part1.clone(), part0.clone()]), Some(&part1));
        // one partition, or the device runs another than it boots from: no fallback
        assert_eq!(other(std::slice::from_ref(&part0)), None);
        assert_eq!(other(&[partition("part0", false, true), partition("part1", true, false)]), None);
    }

    #[test]
    fn switches_back_unless_removed() {
        assert_eq!(
            script("part1", Duration::from_secs(900)),
            ":delay 900s; :if ([:len [/system scheduler find name=\"netbox-provisioner-update-fallback\"]] > 0) \
             do={ /system scheduler remove [find name=\"netbox-provisioner-update-fallback\"]; \
             /partitions activate [find name=\"part1\"]; /system reboot }"
        );
    }

    #[test]
    fn misses_only_neighbors_that_were_adjacent() {
        let set = |neighbors: &[&str]| neighbors.iter().map(|&n| n.into()).collect::<BTreeSet<Box<str>>>();
        let before = set(&["default-v2 10.0.0.1", "default-v3 10.0.0.1"]);
        let now = set(&["default-v2 10.0.0.1", "default-v2 10.0.0.9"]);
        assert_eq!(missing(&before, &now), vec![Box::from("default-v3 10.0.0.1")]);
        assert!(missing(&set(&[]), &now).is_empty());
    }

    fn neighbor(instance: &str, address: &str, router_id: &str, dr: &str, bdr: &str, state: &str) -> Row {
        [
            ("instance", instance),
            ("interface", "bridge-bridge"),
            ("address", address),
            ("router-id", router_id),
            ("dr", dr),
            ("bdr", bdr),
            ("state", state),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), Some(v.into())))
        .collect()
    }

    /// berghof-stall-haus after its update (7.23.8): v3 elected 172.16.1.15 and 172.16.1.7, so
    /// 172.16.1.60, `Full` before, is a DROther like this device and stays in `TwoWay`.
    #[test]
    fn two_drothers_in_two_way_are_adjacent() {
        let v3 = |address, router_id, state| {
            neighbor("default-v3", address, router_id, "172.16.1.15", "172.16.1.7", state)
        };
        let rows = [
            neighbor("default-v2", "10.198.1.1", "172.16.1.7", "10.198.1.1", "10.198.1.3", "Full"),
            neighbor("default-v2", "10.198.1.3", "172.16.1.60", "10.198.1.1", "10.198.1.3", "Full"),
            v3("fe80::1", "172.16.1.7", "Full"),
            v3("fe80::2", "172.16.1.15", "Full"),
            v3("fe80::3", "172.16.1.60", "TwoWay"),
        ];
        assert_eq!(
            adjacent(&rows),
            ["default-v2 172.16.1.60", "default-v2 172.16.1.7", "default-v3 172.16.1.15", "default-v3 172.16.1.60", "default-v3 172.16.1.7"]
                .map(Box::<str>::from)
                .into()
        );
    }

    /// `TwoWay` with the DR or BDR, or where this device is one, is a stuck adjacency.
    #[test]
    fn two_way_with_the_dr_is_not_adjacent() {
        // the neighbor is the DR
        let rows = [neighbor("default-v2", "10.0.0.1", "1.1.1.1", "10.0.0.1", "0.0.0.0", "TwoWay")];
        assert!(adjacent(&rows).is_empty());
        // the DR is no neighbor: this device
        let rows = [neighbor("default-v2", "10.0.0.2", "2.2.2.2", "10.0.0.9", "0.0.0.0", "TwoWay")];
        assert!(adjacent(&rows).is_empty());
        let rows = [neighbor("default-v2", "10.0.0.2", "2.2.2.2", "10.0.0.1", "10.0.0.2", "ExStart")];
        assert!(adjacent(&rows).is_empty());
    }
}
