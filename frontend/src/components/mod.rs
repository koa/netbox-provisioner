//! The components, by what they show: `frame` is around every page (breadcrumb, login),
//! `widgets` are general building blocks without a subject of their own, `device` and `site`
//! show what belongs to one device or one site. A component shown for several subjects (a job,
//! findings, what devices see, the NetBox helpers, the network map, the saved jobs) is a module of its own here.
pub mod device;
pub mod findings;
pub mod frame;
pub mod helper_flow;
pub mod job;
pub mod neighbors;
pub mod netbox_helper;
pub mod network_map;
pub mod saved_jobs;
pub mod site;
pub mod widgets;
