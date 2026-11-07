//! Scope: The queries for how far each device of a site is from NetBox; nothing else.
//!
//! How far each device of a site is from NetBox (`components::site::site_diff`): the job started
//! (`diffSite`), the site's last one (`jobs`), its events polled, and what the provisioner knows
//! of each device since (`knownDiff`, subscribed).
use super::{
    schema::schema,
    types::{ErrorInfo, JobId, JobKind, JobState},
};

#[derive(cynic::QueryVariables, Debug)]
pub struct SiteVariables {
    pub site: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "SiteVariables"
)]
pub struct DiffSite {
    #[arguments(siteId: $site)]
    pub diff_site: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "SiteVariables"
)]
pub struct SiteDiffJobs {
    #[arguments(siteId: $site)]
    pub jobs: Vec<SiteJob>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Job")]
pub struct SiteJob {
    pub id: i32,
    pub kind: JobKind,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "SiteVariables"
)]
pub struct SiteDiffKnown {
    pub topology: KnownTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "SiteVariables"
)]
pub struct KnownTopology {
    #[arguments(id: $site)]
    pub site_by_id: Option<KnownSite>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct KnownSite {
    pub devices: Vec<KnownDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct KnownDevice {
    pub id: i32,
    pub known_diff: KnownDiff,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SiteDiffEventsVariables {
    pub id: i32,
    pub after: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "SiteDiffEventsVariables"
)]
pub struct SiteDiffEvents {
    #[arguments(id: $id)]
    pub job: Option<DiffJob>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "Job",
    variables = "SiteDiffEventsVariables"
)]
pub struct DiffJob {
    pub state: JobState,
    pub age: i32,
    pub error: Option<ErrorInfo>,
    #[arguments(after: $after)]
    pub events: Vec<DiffLogged>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Logged")]
pub struct DiffLogged {
    pub index: i32,
    pub event: DiffEvent,
}

/// The events of a site's diff; the job has no others.
#[derive(cynic::InlineFragments, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Event")]
pub enum DiffEvent {
    SiteDiffStarted(SiteDiffStarted),
    DeviceDiffed(DeviceDiffed),
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct KnownDiff {
    pub menus: Option<Vec<MenuChanges>>,
    pub error: Option<ErrorInfo>,
    pub age: Option<i32>,
    pub changed_age: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SiteDiffStarted {
    pub devices: Vec<DiffedDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceDiffed {
    pub device: DiffedDevice,
    pub menus: Vec<MenuChanges>,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MenuChanges {
    pub menu: String,
    pub added: i32,
    pub changed: i32,
    pub removed: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DiffedDevice {
    pub id: i32,
    pub name: String,
}
