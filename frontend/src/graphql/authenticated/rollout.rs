//! Updates across the network (`pages::rollout`): what a rollout would do (`rolloutPlan`).
use super::{
    schema::schema,
    types::{Channel, ErrorInfo, WideInt},
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
