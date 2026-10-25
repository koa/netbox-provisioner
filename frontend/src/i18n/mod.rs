//! Every text the UI shows, in German. Most come from the crate `i18n`, shared with the
//! backend, and are passed on here, so components keep writing `crate::i18n::…`; the modules
//! below build `Html` or word the frontend's own types, so they stay here. Components write no
//! text inline.

pub use ::i18n::{
    batch, common, device, error, helper, helper_flow, job, login, menu, pages, saved_jobs, site_diff,
};

/// What NetBox lacks or what doesn't fit together (`components::findings`). The objects come in
/// as links to NetBox.
pub mod findings;

/// What the devices see at their ports against NetBox's cables (`components::neighbor_check`).
pub mod neighbors;

/// Choosing another address or other credentials (`components::adjust_target`).
pub mod target;

/// The device's interfaces by task (`components::interfaces`).
pub mod interfaces;
