//! Scope: The words of how far a saved change has reached each of its devices, for the site's
//! automatic rollout and for a resident: the device's state, why it is held, what an automatic
//! device lacks; nothing else.
use crate::graphql::{
    authenticated::auto_rollout::{
        ChangeDevice as SiteDevice, HoldReason as SiteHold, Lack, RolloutState as SiteState,
    },
    resident::changes::{
        ChangeDevice as ResidentDevice, HoldReason as ResidentHold, RolloutState as ResidentState,
    },
};
use ::i18n::{
    auto_rollout::{self as t, HoldReason, RolloutState},
    resident,
};

/// A device of a change on the site page: its name and how far the change is there.
pub fn site_device(device: &SiteDevice) -> String {
    let state = match &device.state {
        SiteState::Pending => RolloutState::Pending,
        SiteState::RolledOut => RolloutState::RolledOut,
        SiteState::Waiting if !device.reasons.is_empty() => RolloutState::WaitingForAdmin,
        SiteState::Waiting => RolloutState::Waiting,
        SiteState::Deferred => RolloutState::Deferred,
        SiteState::Failed => RolloutState::Failed,
        SiteState::Manual | SiteState::Other(_) => RolloutState::Manual,
    };
    format!("{}: {}", device.device.name, t::rollout_state(state))
}

/// How far a resident's change is at the device.
pub fn resident_state(device: &ResidentDevice) -> RolloutState {
    match &device.state {
        ResidentState::Pending => RolloutState::Pending,
        ResidentState::RolledOut => RolloutState::RolledOut,
        ResidentState::Waiting if !device.reasons.is_empty() => RolloutState::WaitingForAdmin,
        ResidentState::Waiting => RolloutState::Waiting,
        ResidentState::Deferred => RolloutState::Deferred,
        ResidentState::Failed => RolloutState::Failed,
        ResidentState::Manual | ResidentState::Other(_) => RolloutState::Manual,
    }
}

/// Why an automatic device holds the site's changes back, for an admin.
pub fn site_hold_reason(reason: &SiteHold) -> String {
    t::hold_reason(match reason {
        SiteHold::PoeCut => HoldReason::PoeCut,
        SiteHold::CapOff => HoldReason::CapOff,
        SiteHold::ApiClientsOutside => HoldReason::ApiClientsOutside,
        SiteHold::RadioLink => HoldReason::RadioLink,
        SiteHold::Rewire => HoldReason::Rewire,
        SiteHold::Outward => HoldReason::Outward,
        SiteHold::Other(other) => HoldReason::Other(other),
    })
}

/// Why a device holds a resident's change back, as the resident reads it: a radio link as a
/// complex change for support (Z58), the others as the site page says them.
pub fn resident_hold_reason(reason: &ResidentHold) -> String {
    t::hold_reason(match reason {
        ResidentHold::RadioLink => return resident::RADIO_LINK_HOLD.to_string(),
        ResidentHold::PoeCut => HoldReason::PoeCut,
        ResidentHold::CapOff => HoldReason::CapOff,
        ResidentHold::ApiClientsOutside => HoldReason::ApiClientsOutside,
        ResidentHold::Rewire => HoldReason::Rewire,
        ResidentHold::Outward => HoldReason::Outward,
        ResidentHold::Other(other) => HoldReason::Other(other),
    })
}

/// What an automatic device lacks for the precondition.
pub fn lack(lack: &Lack) -> String {
    t::lack(match lack {
        Lack::LoopbackIpv4 => t::Lack::LoopbackIpv4,
        Lack::LoopbackIpv6 => t::Lack::LoopbackIpv6,
        Lack::Connection => t::Lack::Connection,
        Lack::Scheduler => t::Lack::Scheduler,
        Lack::Other(other) => t::Lack::Other(other),
    })
}
