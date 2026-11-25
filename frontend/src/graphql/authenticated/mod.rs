//! Scope: The queries of the admins' root fields, one module per page or component; not sending
//! them (`graphql`).

pub mod adjust_target;
pub mod auto_rollout;
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
pub mod rollout;
pub mod saved_jobs;
pub mod site_diff;
pub mod types;
pub mod www_ca;
