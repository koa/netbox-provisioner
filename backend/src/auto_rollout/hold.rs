//! Scope: Why the automatic rollout holds an automatic device with changes back for an admin
//! (`docs/auto-rollout.md`, „Anhalten“), from what the round read of it; not deciding the round
//! (`decide`) nor reading (`auto_rollout`).
//!
//! What the fallback doesn't take back safely, or what needs someone on site, waits for an
//! admin: released (for exactly that diff), or applied by hand. In doubt it holds.
use crate::{
    device::{
        batch,
        neighbors::{NeighborFinding, NeighborFindingKind},
        ros::warning::ApplyWarning,
    },
    topology::access::device::DeviceAccess,
};
use serde::{Deserialize, Serialize};

/// Why an automatic device's changes wait for an admin.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, async_graphql::Enum,
)]
pub enum HoldReason {
    /// the apply cuts PoE out of a port feeding something (`ApplyWarning::PoeCut`)
    PoeCut,
    /// its radios serve no WLAN after the apply (`ApplyWarning::CapOffOutsideWlanGroup`)
    CapOff,
    /// the API would be closed to a client the device has now
    /// (`ApplyWarning::ApiClientsOutside`)
    ApiClientsOutside,
    /// it changes a radio of a device with a radio link: the link and everything behind it may
    /// be away up to 24 min (DFS)
    RadioLink,
    /// a cable in NetBox goes elsewhere than the device sees it: the apply would build the
    /// link where nothing is plugged in yet; it goes once the device sees it there
    Rewire,
    /// it changes what OSPF announces to a router outside NetBox, a port facing outward, or
    /// adds an address outside NetBox's aggregates (`device::ros::outward`)
    Outward,
}

/// The reasons to hold `device` back, whose planned apply has `warnings`, changes a radio
/// (`changes_radio`) or what it announces outward (`outward`), and whose neighbor check has
/// `neighbors` (`None`: not read).
pub fn reasons(
    device: &DeviceAccess,
    warnings: &[ApplyWarning],
    changes_radio: bool,
    outward: bool,
    neighbors: Option<&[NeighborFinding]>,
) -> Box<[HoldReason]> {
    let mut reasons = warnings
        .iter()
        .filter_map(|warning| match warning {
            ApplyWarning::PoeCut(_) => Some(HoldReason::PoeCut),
            ApplyWarning::CapOffOutsideWlanGroup(_) => Some(HoldReason::CapOff),
            ApplyWarning::ApiClientsOutside(_) => Some(HoldReason::ApiClientsOutside),
            // intended states: a silent watch target keeps what the device has, OSPF
            // credentials in doubt stay
            ApplyWarning::WatchTargetSilent(_) | ApplyWarning::OspfAuthKept(_) => None,
        })
        .collect::<Vec<_>>();
    if changes_radio && !batch::wireless_partners(device).is_empty() {
        reasons.push(HoldReason::RadioLink);
    }
    if outward {
        reasons.push(HoldReason::Outward);
    }
    if neighbors.is_some_and(rewired) {
        reasons.push(HoldReason::Rewire);
    }
    reasons.sort();
    reasons.dedup();
    reasons.into()
}

/// Whether the neighbor check of a device says a cable is to be plugged elsewhere: NetBox's
/// cable at a port goes to another device or port than the device sees there, or the device
/// sees a NetBox device at a port without cable while NetBox's cable to it, at another port,
/// sees nothing. A device seen where NetBox has no cable at all is no rewiring (a link NetBox
/// lacks).
pub fn rewired(findings: &[NeighborFinding]) -> bool {
    let not_seen = findings
        .iter()
        .filter_map(|finding| match &finding.kind {
            NeighborFindingKind::NeighborNotSeen(not_seen) => Some(not_seen.far_end.device.id),
            _ => None,
        })
        .collect::<Vec<_>>();
    findings.iter().any(|finding| match &finding.kind {
        NeighborFindingKind::CableElsewhere(_) | NeighborFindingKind::OtherFarPort(_) => true,
        NeighborFindingKind::CableMissing(missing) => missing.seen.iter().any(|seen| {
            seen.device
                .as_ref()
                .is_some_and(|device| not_seen.contains(&device.id))
        }),
        NeighborFindingKind::NeighborNotSeen(_)
        | NeighborFindingKind::NeighborsNotInNetbox(_)
        | NeighborFindingKind::DiscoveryOff(_) => false,
    })
}

#[cfg(test)]
mod test;
