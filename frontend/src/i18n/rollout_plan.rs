//! Scope: The words of a rollout's plan per device: why it is left out, its way back; nothing
//! else.
use crate::graphql::authenticated::rollout::{RolloutFallback, RolloutSkip};
use ::i18n::rollout as t;

/// Why the rollout leaves a device out; empty for a reason of a newer server.
pub fn skip(skip: &RolloutSkip) -> &'static str {
    match skip {
        RolloutSkip::UpToDate => t::SKIP_UP_TO_DATE,
        RolloutSkip::Unreachable => t::SKIP_UNREACHABLE,
        RolloutSkip::Leftovers => t::SKIP_LEFTOVERS,
        RolloutSkip::NoFallback => t::SKIP_NO_FALLBACK,
        RolloutSkip::NoSpace => t::SKIP_NO_SPACE,
        RolloutSkip::Other(_) => "",
    }
}

/// The way back after the device's update; empty for one of a newer server.
pub fn fallback(fallback: &RolloutFallback) -> &'static str {
    match fallback {
        RolloutFallback::Partition => t::FALLBACK_PARTITION,
        RolloutFallback::Neighbor => t::FALLBACK_NEIGHBOR,
        RolloutFallback::None => t::FALLBACK_NONE,
        RolloutFallback::Other(_) => "",
    }
}
