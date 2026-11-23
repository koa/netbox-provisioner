//! Scope: The queries and mutations of a site's automatic rollout card
//! (`components::site::auto_rollout`); nothing else.
use super::schema::schema;

#[derive(cynic::QueryVariables, Debug)]
pub struct SiteVariables {
    pub site: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "SiteVariables"
)]
pub struct SiteAutoRolloutQuery {
    pub topology: Topology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "SiteVariables"
)]
pub struct Topology {
    #[arguments(id: $site)]
    pub site_by_id: Option<Site>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct Site {
    pub auto_rollout: Option<SiteAutoRollout>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SiteAutoRollout {
    pub seen: bool,
    pub error: Option<SiteAutoRolloutError>,
    pub last_round: Option<LastRound>,
    pub released: Vec<i32>,
    pub holding: Vec<i32>,
    pub unmet: Vec<Unmet>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Unmet {
    pub device: RoundDevice,
    pub lacks: Vec<Lack>,
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum Lack {
    #[cynic(rename = "LOOPBACK_IPV_4")]
    LoopbackIpv4,
    #[cynic(rename = "LOOPBACK_IPV_6")]
    LoopbackIpv6,
    Connection,
    Scheduler,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SiteAutoRolloutError {
    pub reason: SiteError,
    pub devices: Vec<RoundDevice>,
    pub age: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LastRound {
    pub outcome: RoundOutcome,
    pub applied: Vec<RoundDevice>,
    pub deferred: Vec<RoundDevice>,
    pub held_by: Vec<RoundDevice>,
    pub held: Vec<HeldDevice>,
    pub age: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct HeldDevice {
    pub device: RoundDevice,
    pub reasons: Vec<HoldReason>,
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum HoldReason {
    PoeCut,
    CapOff,
    ApiClientsOutside,
    RadioLink,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RoundDevice {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum SiteError {
    NotInLine,
    ApplyFailed,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RoundOutcome {
    NothingToDo,
    NotInLine,
    Applied,
    HeldBack,
    NoInternet,
    Busy,
    Failed,
    NetboxUnreadable,
    PreconditionUnmet,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "SiteVariables"
)]
pub struct ClearAutoRolloutError {
    #[arguments(siteId: $site)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub clear_auto_rollout_error: bool,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ReleaseVariables {
    pub devices: Vec<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "ReleaseVariables"
)]
pub struct ReleaseAutoRollout {
    #[arguments(deviceIds: $devices)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub release_auto_rollout: bool,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "ReleaseVariables"
)]
pub struct AcceptAutoRolloutDiff {
    #[arguments(deviceIds: $devices)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub accept_auto_rollout_diff: bool,
}
