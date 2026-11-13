//! Scope: The way back from an update: the scheduler on the old partition and the OSPF neighbors
//! after it; not the update (`update`).
//!
//! The way back from a RouterOS update (`docs/update-fallback.md`, stage 1): on a
//! device with two [`partitions`](super::partitions), the running RouterOS is copied to the
//! other one with its configuration (`/partitions copy-to`) before the update. A scheduler run
//! at every boot then waits [`Settings::update_fallback`](crate::config::Settings) and, unless
//! the provisioner removed it in the meantime, activates the old partition and reboots. It is
//! added after the copy, so the old partition doesn't carry it. A partition that doesn't boot
//! at all RouterBOOT leaves by itself (`fallback-to=next`).
//!
//! Without two partitions, the way back is a neighbor the device stays reachable through
//! (stage 2, [`WayBack`]); a device with neither is only updated when that was confirmed.
//!
//! The update counts as done when the device is back with the version and the OSPF neighbors
//! it was adjacent with before are again ([`ospf_up`], [`missing`]).
use crate::{
    Error,
    config::CONFIG,
    device::{
        access::remove_if_present,
        bootstrap,
        partitions::{self, copy_to, other},
        ros::apply::RADIO_RESTART,
    },
    graphql::scalars::ScalarDuration,
    job::Log,
    topology::access::device::DeviceAccess,
};
use async_graphql::SimpleObject;
use mikrotik_model::{
    ascii::AsciiString,
    command::{Reads, Writes},
    model::{
        InterfaceWifiRadioState, InterfaceWireless, OspfNeighborState, RoutingOspfNeighborState,
        SystemSchedulerByName, SystemSchedulerCfg,
    },
    resource::{Creatable, apply_mutation, collect_resource},
};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::{Instant, sleep};

/// The name of the scheduler entry.
pub(crate) const NAME: &str = "netbox-provisioner-update-fallback";
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

/// The device updates without a second partition to switch back to: it has `partitions`
/// (none or one). `neighbors` are those it stays reachable through (empty: confirmed without).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateWithoutFallback {
    pub partitions: u32,
    pub neighbors: Box<[Box<str>]>,
}

/// The way back of an update besides partitions: the neighbors on its cables that reach the
/// device through a tunnel when OSPF doesn't come up after it, and whether updating without
/// any was confirmed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WayBack {
    pub device: Box<str>,
    pub neighbors: Box<[Box<str>]>,
    pub confirmed: bool,
}

impl WayBack {
    pub fn of(device: &DeviceAccess, confirmed: bool) -> Self {
        Self {
            device: device.name().into(),
            neighbors: neighbors(device),
            confirmed,
        }
    }
    /// The update goes on without partitions to switch back to: there is a neighbor, or it
    /// was confirmed.
    fn without_partitions(&self, partitions: u32, log: &Log) -> Result<(), Error> {
        if self.neighbors.is_empty() && !self.confirmed {
            return Err(Error::NoUpdateFallback(self.device.clone()));
        }
        log.emit(UpdateWithoutFallback {
            partitions,
            neighbors: self.neighbors.clone(),
        });
        Ok(())
    }
}

/// The neighbors `device` stays reachable through when its primary IP doesn't answer (those of
/// its [`bootstrap::plans`]); none for the management proxy's device, through which the
/// provisioner reaches every other.
pub fn neighbors(device: &DeviceAccess) -> Box<[Box<str>]> {
    let proxy = CONFIG
        .management_proxy
        .as_ref()
        .map(|proxy| proxy.device.as_ref());
    if proxy == Some(device.name()) {
        return Box::new([]);
    }
    let mut neighbors = bootstrap::plans(device)
        .unwrap_or_default()
        .into_iter()
        .map(|plan| plan.neighbor.into_boxed_str())
        .collect::<Vec<_>>();
    neighbors.dedup();
    neighbors.into()
}

/// An update of RouterOS 6, which has no partitions in this menu.
pub fn arm_v6(way_back: &WayBack, log: &Log) -> Result<(), Error> {
    way_back.without_partitions(0, log)
}

/// A downgrade copies nothing to the other partition, which may hold the RouterOS to go back
/// to; without two partitions its way back is as for an update ([`WayBack`]).
pub async fn check_downgrade(
    device: &impl Reads,
    way_back: &WayBack,
    log: &Log,
) -> Result<(), Error> {
    let list = partitions::list(device).await;
    if other(&list).is_none() {
        way_back.without_partitions(list.len() as u32, log)?;
    }
    Ok(())
}

/// The OSPF neighbors this device was adjacent with before the update are again.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct OspfNeighborsBack {
    pub neighbors: Box<[Box<str>]>,
}

/// Copies the running RouterOS to the other partition and schedules switching to it, where
/// the device has two partitions; `None` (and [`UpdateWithoutFallback`]) where not, or
/// [`Error::NoUpdateFallback`] without `way_back` either, before anything changes.
pub async fn arm(
    device: &impl Writes,
    delay: Duration,
    way_back: &WayBack,
    log: &Log,
) -> Result<Option<Box<str>>, Error> {
    let list = partitions::list(device).await;
    let Some(target) = other(&list).map(|p| p.name.clone()) else {
        way_back.without_partitions(list.len() as u32, log)?;
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
            Error::Refused(
                format!("{target} holds no copy of the running RouterOS after copy-to").into(),
            )
        })?;
    let on_event = script(&target, delay);
    let mut entry = SystemSchedulerCfg::new(NAME.into());
    entry.start_time = Some("startup".into());
    entry.on_event = Some(on_event.into());
    entry.comment =
        Some("boots the RouterOS from before the update, unless the provisioner stops it".into());
    apply_mutation(device, &entry.calculate_create()).await?;
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

/// Removes the scheduler: the device is reached with the update. The old partition stays.
pub async fn disarm(device: &impl Writes, partition: &str, log: &Log) -> Result<(), Error> {
    remove_scheduler(device).await?;
    log.emit(UpdateFallbackDisarmed {
        partition: partition.into(),
    });
    Ok(())
}

pub(crate) async fn remove_scheduler(device: &impl Writes) -> Result<(), Error> {
    remove_if_present::<SystemSchedulerByName>(device, &AsciiString::from(NAME)).await
}

/// The partition the device runs; `None` without partitions.
pub async fn running(device: &impl Reads) -> Option<Box<str>> {
    partitions::list(device)
        .await
        .into_iter()
        .find(|p| p.running)
        .map(|p| p.name)
}

/// The OSPF neighbors this device is adjacent with (see [`adjacent`]), as `<instance>
/// <router-id>` (an instance per OSPF version); empty without OSPF.
pub async fn ospf_up(device: &impl Reads) -> Result<BTreeSet<Box<str>>, Error> {
    Ok(adjacent(
        &collect_resource::<RoutingOspfNeighborState>(device).await?,
    ))
}

/// The neighbors in `rows` (`/routing/ospf/neighbor`) that are where OSPF leaves them: `Full`,
/// or `TwoWay` where the DR and the BDR are two others on the segment. Two DROthers stay in
/// `TwoWay` for good, and a reboot elects anew, so one that was `Full` as DR may end there
/// (berghof-stall-haus, 7.23.8). v2 names the DR by its address, v3 by its router ID.
fn adjacent(rows: &[RoutingOspfNeighborState]) -> BTreeSet<Box<str>> {
    rows.iter()
        .filter(|row| is_adjacent(rows, row))
        .map(|row| format!("{} {}", row.instance, row.router_id).into())
        .collect()
}

/// Whether `row` of `rows` (`/routing/ospf/neighbor`) is where OSPF leaves it (see
/// [`adjacent`]).
pub(crate) fn is_adjacent(
    rows: &[RoutingOspfNeighborState],
    row: &RoutingOspfNeighborState,
) -> bool {
    let same_segment = |a: &RoutingOspfNeighborState, b: &RoutingOspfNeighborState| {
        a.instance == b.instance && a.interface == b.interface
    };
    // `id` (the DR or BDR, an address in v2, a router ID in v3) is none, or a neighbor of the
    // segment other than `row`
    let another = |id: Option<&AsciiString>| {
        let id = id.map(ToString::to_string).unwrap_or_default();
        match id.as_str() {
            "" | "0.0.0.0" => true,
            id => rows.iter().any(|other| {
                !std::ptr::eq(other, row)
                    && same_segment(other, row)
                    && (other.address.to_string() == id || other.router_id.to_string() == id)
            }),
        }
    };
    match row.state {
        OspfNeighborState::Full => true,
        OspfNeighborState::TwoWay => another(row.dr.as_ref()) && another(row.bdr.as_ref()),
        _ => false,
    }
}

/// Whether the device has a 5 GHz radio on: `/interface/wireless` with a `5ghz` band, or an
/// `/interface/wifi` radio with `5ghz` among its bands. A menu its packages lack doesn't count.
async fn has_5ghz_radio(device: &impl Reads) -> bool {
    use mikrotik_model::model::WirelessBand as Band;
    let wireless = collect_resource::<InterfaceWireless>(device)
        .await
        .unwrap_or_default()
        .iter()
        .any(|radio| {
            !radio.cfg.disabled
                && matches!(
                    radio.cfg.band,
                    Some(
                        Band::_5GhzA
                            | Band::_5GhzAN
                            | Band::_5GhzANAc
                            | Band::_5GhzNAc
                            | Band::_5GhzOnlyac
                            | Band::_5GhzOnlyn
                    )
                )
        });
    // `bands` is text in the model: `5ghz-a`, `5ghz-ax`, ...
    wireless
        || collect_resource::<InterfaceWifiRadioState>(device)
            .await
            .unwrap_or_default()
            .iter()
            .any(|radio| radio.bands.iter().any(|band| band.0.starts_with(b"5ghz")))
}

/// What of `before` isn't in `now`.
pub fn missing(before: &BTreeSet<Box<str>>, now: &BTreeSet<Box<str>>) -> Vec<Box<str>> {
    before.difference(now).cloned().collect()
}

/// Waits (up to [`OSPF_TIMEOUT`], [`RADIO_RESTART`] on a device with a 5 GHz radio) until the
/// neighbors `before` are adjacent again; the ones that aren't by then. A 5 GHz AP on a DFS
/// channel listens for radar after its boot before it sends, 10 min near the weather radar
/// band: lab-sxt-a's update failed twice with its neighbors back a few minutes later.
pub async fn wait_for_ospf(
    device: &impl Reads,
    before: &BTreeSet<Box<str>>,
    log: &Log,
) -> Result<Vec<Box<str>>, Error> {
    let limit = match has_5ghz_radio(device).await {
        true => RADIO_RESTART,
        false => OSPF_TIMEOUT,
    };
    let deadline = Instant::now() + limit;
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
    use super::{adjacent, missing, script};
    use crate::device::answer::parsed;
    use mikrotik_model::model::RoutingOspfNeighborState;
    use std::{collections::BTreeSet, time::Duration};

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
        let set = |neighbors: &[&str]| {
            neighbors
                .iter()
                .map(|&n| n.into())
                .collect::<BTreeSet<Box<str>>>()
        };
        let before = set(&["default-v2 10.0.0.1", "default-v3 10.0.0.1"]);
        let now = set(&["default-v2 10.0.0.1", "default-v2 10.0.0.9"]);
        assert_eq!(
            missing(&before, &now),
            vec![Box::from("default-v3 10.0.0.1")]
        );
        assert!(missing(&set(&[]), &now).is_empty());
    }

    fn neighbor(
        instance: &str,
        address: &str,
        router_id: &str,
        dr: &str,
        bdr: &str,
        state: &str,
    ) -> RoutingOspfNeighborState {
        parsed(&[
            (".id", "*1"),
            ("instance", instance),
            ("area", "backbone"),
            ("interface", "bridge-bridge"),
            ("address", address),
            ("router-id", router_id),
            ("dr", dr),
            ("bdr", bdr),
            ("state", state),
        ])
    }

    /// berghof-stall-haus after its update (7.23.8): v3 elected 172.16.1.15 and 172.16.1.7, so
    /// 172.16.1.60, `Full` before, is a DROther like this device and stays in `TwoWay`.
    #[test]
    fn two_drothers_in_two_way_are_adjacent() {
        let v3 = |address, router_id, state| {
            neighbor(
                "default-v3",
                address,
                router_id,
                "172.16.1.15",
                "172.16.1.7",
                state,
            )
        };
        let rows = [
            neighbor(
                "default-v2",
                "10.198.1.1",
                "172.16.1.7",
                "10.198.1.1",
                "10.198.1.3",
                "Full",
            ),
            neighbor(
                "default-v2",
                "10.198.1.3",
                "172.16.1.60",
                "10.198.1.1",
                "10.198.1.3",
                "Full",
            ),
            v3("fe80::1", "172.16.1.7", "Full"),
            v3("fe80::2", "172.16.1.15", "Full"),
            v3("fe80::3", "172.16.1.60", "TwoWay"),
        ];
        assert_eq!(
            adjacent(&rows),
            [
                "default-v2 172.16.1.60",
                "default-v2 172.16.1.7",
                "default-v3 172.16.1.15",
                "default-v3 172.16.1.60",
                "default-v3 172.16.1.7"
            ]
            .map(Box::<str>::from)
            .into()
        );
    }

    /// `TwoWay` with the DR or BDR, or where this device is one, is a stuck adjacency.
    #[test]
    fn two_way_with_the_dr_is_not_adjacent() {
        // the neighbor is the DR
        let rows = [neighbor(
            "default-v2",
            "10.0.0.1",
            "1.1.1.1",
            "10.0.0.1",
            "0.0.0.0",
            "TwoWay",
        )];
        assert!(adjacent(&rows).is_empty());
        // the DR is no neighbor: this device
        let rows = [neighbor(
            "default-v2",
            "10.0.0.2",
            "2.2.2.2",
            "10.0.0.9",
            "0.0.0.0",
            "TwoWay",
        )];
        assert!(adjacent(&rows).is_empty());
        let rows = [neighbor(
            "default-v2",
            "10.0.0.2",
            "2.2.2.2",
            "10.0.0.1",
            "10.0.0.2",
            "ExStart",
        )];
        assert!(adjacent(&rows).is_empty());
    }
}
