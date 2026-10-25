//! Every text the provisioner shows, in German, by the page, component or record showing it.
//! The frontend takes its texts from here and writes none inline, and so does the backend
//! where it writes text for people (the NetBox journal): it sends data, which is worded here
//! (job events, warnings, errors). A second language replaces these functions, or makes them
//! choose by language, without touching the code using them. Names RouterOS or NetBox use
//! themselves (`RouterOS`, `Board`, `long-term`) stay as they are.
//!
//! Only plain values go in (`&str`, numbers, the enums defined here); what builds `Html` or
//! knows the frontend's types stays in the frontend's `i18n` module, which passes these on.

/// Texts used in several places.
pub mod common;

/// An error the backend reports, and the frontend's own.
pub mod error;

pub mod login;

/// The breadcrumb of dropdowns (`components::menu`).
pub mod menu;

pub mod pages;

/// Jobs saved before a restart that didn't go on (`components::saved_jobs`).
pub mod saved_jobs;

/// How far each device of a site is from NetBox (`components::site_diff`).
pub mod site_diff;

/// The device page (`pages::devices::show`).
pub mod device;

/// Applying together with other devices (`components::batch_apply`).
pub mod batch;

/// A job and its events (`components::job`), as the backend reports them.
pub mod job;

/// The helpers that write to NetBox (`components::netbox_helper`).
pub mod helper;

/// A helper in a modal with its steps (`components::helper_flow`).
pub mod helper_flow;
