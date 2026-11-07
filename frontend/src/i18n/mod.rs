//! Scope: The frontend's wording: the crate `i18n` passed on, and what builds `Html` or words the
//! frontend's own types; no text a backend could use too (crate `i18n`).
//!
//! Every text the UI shows, in German. Most come from the crate `i18n`, shared with the
//! backend, and are passed on here, so components keep writing `crate::i18n::…`; the modules
//! below build `Html` or word the frontend's own types, so they stay here. Components write no
//! text inline.

pub use ::i18n::{
    batch, common, device, error, helper, helper_flow, job, journal, login, menu, network_map,
    pages, rollout, saved_jobs, site_diff,
};

/// What NetBox lacks or what doesn't fit together (`components::findings`). The objects come in
/// as links to NetBox.
pub mod findings;

/// What the devices see at their ports against NetBox's cables (`components::neighbors`).
pub mod neighbors;

/// Choosing another address or other credentials (`components::device::adjust_target`).
pub mod target;

/// The device's interfaces by task (`components::device::interfaces`).
pub mod interfaces;

/// A warning of `generateCfg` (the device page, `components::device::batch_apply`).
pub mod apply_warning;

/// The features of a device-mode change, with why each is needed (the device page,
/// `components::job`).
pub mod device_mode;
