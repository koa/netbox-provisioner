//! Scope: The pages, one per route; not a component shown on several (`components`).
//!
//! The pages, one per route, as the breadcrumb leads to them (`routes`): the sites, the map of
//! all sites, the residents (support view, and a resident's own page), and below a site its
//! devices, its map and each device.
pub mod network_map;
pub mod residents;
pub mod rollout;
pub mod site;
pub mod sites;
