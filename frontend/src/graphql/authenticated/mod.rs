//! The queries of the authenticated endpoint (`/graphql`), checked against its schema
//! (`schema.graphql`, `schema`); one module per page or component.

pub mod adjust_target;
pub mod batch_apply;
pub mod device;
pub mod findings;
pub mod helper;
pub mod helper_options;
pub mod history;
pub mod interface_live;
pub mod interfaces;
pub mod job;
pub mod list_devices;
pub mod neighbor_findings;
pub mod netbox_plan;
pub mod network_map;
pub mod saved_jobs;
pub mod schema;
pub mod site_diff;
pub mod types;
