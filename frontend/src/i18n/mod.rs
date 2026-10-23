//! Every text the UI shows, in German, by the page or component showing it. Components take
//! their texts from here and write none inline; the backend sends data, which is worded here
//! (job events, warnings). A second language replaces these functions, or makes them choose
//! by language, without touching the components. Names RouterOS or NetBox use themselves
//! (`RouterOS`, `Board`, `long-term`) stay as they are.

/// Texts used in several places.
pub mod common;

/// `crate::error::FrontendError` as the user reads it.
pub mod error;

pub mod login;

/// The breadcrumb of dropdowns (`components::menu`).
pub mod menu;

pub mod pages;

/// What NetBox lacks or what doesn't fit together (`components::findings`). The objects come in
/// as links to NetBox.
pub mod findings;

/// What the devices see at their ports against NetBox's cables (`components::neighbor_check`).
pub mod neighbors;

/// Choosing another address or other credentials (`components::adjust_target`).
pub mod target;

/// The device page (`pages::devices::show`).
pub mod device;

/// Applying together with other devices (`components::batch_apply`).
pub mod batch;

/// A job and its events (`components::job`), as the backend reports them.
pub mod job;

/// The helpers that write to NetBox (`components::netbox_helper`).
pub mod helper;
