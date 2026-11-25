//! Scope: The queries for applying several devices together; nothing else.
//!
//! Applying the config to a device together with others (`components::device::batch_apply`): the
//! devices to pick (`batchCandidates`), the plan of each as the batch would plan it
//! (`generateCfg(together:)`), the batch started (`applyBatch`).
use super::types::ApplyWarning;
use crate::graphql::schema::schema;

#[derive(cynic::QueryVariables, Debug)]
pub struct DeviceVariables {
    pub id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "DeviceVariables")]
pub struct BatchCandidates {
    pub topology: CandidatesTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "TopologyHolder", variables = "DeviceVariables")]
pub struct CandidatesTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<CandidatesDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "DeviceAccess")]
pub struct CandidatesDevice {
    pub batch_candidates: Vec<BatchCandidate>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct BatchCandidate {
    pub partner: bool,
    pub device: CandidateDevice,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(graphql_type = "DeviceAccess")]
pub struct CandidateDevice {
    pub id: i32,
    pub name: String,
    pub site: Option<CandidateSite>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(graphql_type = "SiteAccess")]
pub struct CandidateSite {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct PlanVariables {
    pub id: i32,
    /// every device of the batch
    pub together: Vec<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "PlanVariables")]
pub struct BatchDevice {
    pub topology: PlanTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "TopologyHolder", variables = "PlanVariables")]
pub struct PlanTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<PlanDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "DeviceAccess", variables = "PlanVariables")]
pub struct PlanDevice {
    pub access: Option<PlanAccess>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "AccessibleDevice", variables = "PlanVariables")]
pub struct PlanAccess {
    pub connected: Connected,
    #[arguments(together: $together)]
    pub generate_cfg: Option<PlannedCfg>,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct Connected {
    pub address: String,
    pub neighbor: Option<String>,
    pub identity: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "GeneratedCfg")]
pub struct PlannedCfg {
    pub diff: super::types::CfgDiff,
    pub warnings: Vec<ApplyWarning>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ApplyBatchVariables {
    pub ids: Vec<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Mutation", variables = "ApplyBatchVariables")]
pub struct ApplyBatch {
    #[arguments(ids: $ids)]
    pub apply_batch: Vec<BatchJob>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Job")]
pub struct BatchJob {
    pub id: i32,
    pub device_id: Option<i32>,
    pub device_name: Option<String>,
}
