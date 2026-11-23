//! Scope: Why the automatic rollout holds an automatic device with changes back for an admin
//! (`docs/auto-rollout.md`, „Anhalten“), from what the round read of it; not deciding the round
//! (`decide`) nor reading (`auto_rollout`).
//!
//! What the fallback doesn't take back safely, or what needs someone on site, waits for an
//! admin: released (for exactly that diff), or applied by hand. In doubt it holds.
use crate::{
    device::{batch, ros::warning::ApplyWarning},
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
}

/// The reasons to hold `device` back, whose planned apply has `warnings` and changes a radio
/// (`changes_radio`).
pub fn reasons(
    device: &DeviceAccess,
    warnings: &[ApplyWarning],
    changes_radio: bool,
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
    reasons.sort();
    reasons.dedup();
    reasons.into()
}
