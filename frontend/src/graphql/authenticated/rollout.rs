//! Updates across the network (`pages::rollout`): what a rollout would do (`rolloutPlan`),
//! starting and steering it, and its job's events.
use super::{
    schema::schema,
    types::{Channel, ErrorInfo, JobState, UpdateState, WideInt},
};

#[derive(cynic::QueryVariables, Debug)]
pub struct RolloutPlanVariables {
    pub sites: Vec<i32>,
    pub channel: Option<Channel>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "RolloutPlanVariables"
)]
pub struct RolloutPlanQuery {
    #[arguments(siteIds: $sites, channel: $channel)]
    pub rollout_plan: RolloutPlan,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RolloutPlan {
    pub channel: Channel,
    pub target: String,
    pub devices: Vec<PlannedDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PlannedDevice {
    pub id: i32,
    pub name: String,
    pub site_id: i32,
    pub site: String,
    pub version: Option<String>,
    pub architecture: Option<String>,
    pub files: Vec<String>,
    pub download: Option<WideInt>,
    pub free_space: Option<WideInt>,
    pub in_ram: bool,
    pub cut_by: Vec<i32>,
    pub fallback: RolloutFallback,
    pub skip: Option<RolloutSkip>,
    pub error: Option<ErrorInfo>,
    pub pilot: bool,
    pub depth: Option<i32>,
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RolloutFallback {
    Partition,
    Neighbor,
    None,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RolloutSkip {
    UpToDate,
    Unreachable,
    Leftovers,
    NoFallback,
    NoSpace,
    #[cynic(fallback)]
    Other(String),
}

/// Whether writes are allowed, and the newest rollout job, to follow on opening the page.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct RolloutStatus {
    pub allow_apply: bool,
    pub rollout_job: Option<RolloutJobId>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Job")]
pub struct RolloutJobId {
    pub id: i32,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct StartRolloutVariables {
    pub sites: Vec<i32>,
    pub channel: Option<Channel>,
    pub pilots: Option<Vec<i32>>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "StartRolloutVariables"
)]
pub struct StartRollout {
    #[arguments(siteIds: $sites, channel: $channel, pilots: $pilots)]
    pub start_rollout: RolloutJobId,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ContinueRolloutVariables {
    pub job: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "ContinueRolloutVariables"
)]
pub struct ContinueRollout {
    #[arguments(jobId: $job)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub continue_rollout: bool,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SkipRolloutDeviceVariables {
    pub job: i32,
    pub device: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "SkipRolloutDeviceVariables"
)]
pub struct SkipRolloutDevice {
    #[arguments(jobId: $job, deviceId: $device)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub skip_rollout_device: bool,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct RolloutEventsVariables {
    pub id: i32,
    pub after: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "RolloutEventsVariables"
)]
pub struct RolloutEvents {
    #[arguments(id: $id)]
    pub job: Option<RolloutJob>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "Job",
    variables = "RolloutEventsVariables"
)]
pub struct RolloutJob {
    pub state: JobState,
    pub error: Option<ErrorInfo>,
    #[arguments(after: $after)]
    pub events: Vec<RolloutLogged>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Logged")]
pub struct RolloutLogged {
    pub index: i32,
    pub event: RolloutEvent,
}

/// The events of a rollout; the job has no others.
#[derive(cynic::InlineFragments, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Event")]
pub enum RolloutEvent {
    RolloutPlanned(RolloutPlanned),
    RolloutDevice(RolloutDevice),
    RolloutPaused(RolloutPaused),
    RolloutContinued(RolloutContinued),
    RolloutDone(RolloutDone),
    PackageDownloaded(RolloutDownloaded),
    #[cynic(fallback)]
    Other,
}

/// A package the rollout downloaded before its first device.
#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "PackageDownloaded")]
pub struct RolloutDownloaded {
    pub file: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RolloutPlanned {
    pub plan: RolloutPlan,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RolloutDevice {
    pub device: i32,
    pub state: RolloutDeviceState,
    pub job: Option<i32>,
    pub phase: Option<UpdateState>,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RolloutDeviceState {
    Waiting,
    Started,
    Done,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RolloutPaused {
    pub reason: RolloutPause,
    pub device: Option<String>,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RolloutPause {
    AfterPilots,
    Failed,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RolloutContinued {
    pub after: RolloutPause,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RolloutDone {
    pub updated: i32,
    pub skipped: i32,
    pub failed: i32,
}
