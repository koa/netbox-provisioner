//! Applying the config to a device together with others (`components::batch_apply`): the
//! devices to pick (`batchCandidates`), the plan of each (`generateCfg`), the batch started
//! (`applyBatch`).
use super::{schema::schema, types::ApplyWarning};

#[derive(cynic::QueryVariables, Debug)]
pub struct DeviceVariables {
    pub id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct BatchCandidates {
    pub topology: CandidatesTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct CandidatesTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<CandidatesDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct CandidatesDevice {
    pub batch_candidates: Vec<BatchCandidate>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BatchCandidate {
    pub partner: bool,
    pub device: CandidateDevice,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct CandidateDevice {
    pub id: i32,
    pub name: String,
    pub site: Option<CandidateSite>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct CandidateSite {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct BatchDevice {
    pub topology: PlanTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct PlanTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<PlanDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct PlanDevice {
    pub access: Option<PlanAccess>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "AccessibleDevice")]
pub struct PlanAccess {
    pub connected: Connected,
    pub generate_cfg: Option<PlannedCfg>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated")]
pub struct Connected {
    pub address: String,
    pub neighbor: Option<String>,
    pub identity: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "GeneratedCfg")]
pub struct PlannedCfg {
    pub script: String,
    pub warnings: Vec<ApplyWarning>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ApplyBatchVariables {
    pub ids: Vec<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "ApplyBatchVariables"
)]
pub struct ApplyBatch {
    #[arguments(ids: $ids)]
    pub apply_batch: Vec<BatchJob>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Job")]
pub struct BatchJob {
    pub id: i32,
    pub device_id: Option<i32>,
    pub device_name: Option<String>,
}
