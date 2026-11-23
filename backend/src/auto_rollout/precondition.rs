//! Scope: The automatic rollout's precondition (`docs/auto-rollout.md`, „Vorbedingung“): the
//! part NetBox tells, before a round reads a device, and the device mode's part from what the
//! round read; not reading nor what a round does when it is unmet (`auto_rollout`).
//!
//! Every automatic RouterOS device has a loopback for IPv4 (`/32`) and IPv6 (`/128`) on `lo`,
//! and an IP connection to every other one: over OSPF neighbors (both with an `ospf`
//! interface in the same IPv4 network, as in the management tree), also across devices outside
//! the automatic sites. A radio bridge without RouterOS is like a cable: the networks of its
//! ends meet anyway. It catches what a deletion in NetBox by mistake makes dangerous, a deleted
//! loopback or the last connection of a device.
use super::RoundDevice;
use crate::topology::{
    DeviceId, access::device::DeviceAccess, management::ManagementTree, natural::naturalize,
};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What an automatic device lacks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, async_graphql::Enum,
)]
pub enum Lack {
    /// no IPv4 `/32` on `lo`
    LoopbackIpv4,
    /// no IPv6 `/128` on `lo`
    LoopbackIpv6,
    /// no way over OSPF neighbors to the most of the other automatic devices
    Connection,
    /// the device mode has the scheduler off: no fallback for the apply (read with the diff)
    Scheduler,
}

/// An automatic device the precondition fails on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, async_graphql::SimpleObject)]
pub struct Unmet {
    pub device: RoundDevice,
    pub lacks: Box<[Lack]>,
}

/// The automatic devices among `automatic` the precondition fails on, in NetBox's order of the
/// names. Of devices split into groups without a connection between them, the biggest group
/// (at a tie the one with the first name) has its connection; every other device lacks one.
pub fn unmet(automatic: &[DeviceAccess]) -> Vec<Unmet> {
    let mut automatic = automatic.to_vec();
    automatic.sort_by_cached_key(|device| naturalize(device.name()));
    let Some(first) = automatic.first() else {
        return Vec::new();
    };
    let tree = ManagementTree::of_device(first);
    let ids: BTreeSet<DeviceId> = automatic.iter().map(DeviceAccess::id).collect();
    let mut connected: Option<BTreeSet<DeviceId>> = None;
    let mut grouped = BTreeSet::new();
    for device in &automatic {
        if grouped.contains(&device.id()) {
            continue;
        }
        let group: BTreeSet<DeviceId> = tree
            .reached([device.id()])
            .intersection(&ids)
            .copied()
            .chain([device.id()])
            .collect();
        grouped.extend(group.iter().copied());
        if connected
            .as_ref()
            .is_none_or(|biggest| group.len() > biggest.len())
        {
            connected = Some(group);
        }
    }
    let connected = connected.unwrap_or_default();
    automatic
        .iter()
        .filter_map(|device| {
            let mut lacks = Vec::new();
            let loopbacks: Vec<IpNet> = device
                .interfaces()
                .iter()
                .filter(|interface| interface.name() == "lo")
                .flat_map(|interface| interface.ips())
                .filter_map(|ip| ip.net())
                .collect();
            if !loopbacks
                .iter()
                .any(|net| matches!(net, IpNet::V4(net) if net.prefix_len() == 32))
            {
                lacks.push(Lack::LoopbackIpv4);
            }
            if !loopbacks
                .iter()
                .any(|net| matches!(net, IpNet::V6(net) if net.prefix_len() == 128))
            {
                lacks.push(Lack::LoopbackIpv6);
            }
            if !connected.contains(&device.id()) {
                lacks.push(Lack::Connection);
            }
            (!lacks.is_empty()).then(|| Unmet {
                device: RoundDevice::of(device),
                lacks: lacks.into(),
            })
        })
        .collect()
}

/// The automatic devices among `automatic` whose device mode has the scheduler off
/// (`without_scheduler`, from the round's reads): the apply would go without a fallback, and a
/// device cutting itself off stays away until someone is there.
pub fn without_scheduler(
    automatic: &[DeviceAccess],
    without_scheduler: &BTreeSet<DeviceId>,
) -> Vec<Unmet> {
    let mut automatic = automatic
        .iter()
        .filter(|device| without_scheduler.contains(&device.id()))
        .collect::<Vec<_>>();
    automatic.sort_by_cached_key(|device| naturalize(device.name()));
    automatic
        .into_iter()
        .map(|device| Unmet {
            device: RoundDevice::of(device),
            lacks: [Lack::Scheduler].into(),
        })
        .collect()
}

#[cfg(test)]
mod test;
