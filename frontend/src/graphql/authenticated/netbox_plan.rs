//! A helper's plan (`netboxPlan`): its steps for NetBox, what it cuts off or leaves behind; its
//! preview (the device's finding kinds after it, the script applying would run); writing it
//! (`writeNetboxPlan`), taking it back (`undoNetboxWrite`) and the device's helper jobs
//! (`components::netbox_helper::plan`).
use super::{
    findings::Finding,
    helper::{Helper, InterfaceTag},
    schema::schema,
    types::{
        AdhocCredentials, DeviceRef, InterfaceRef, IpVersion, JobKind, JobState, PoeMode,
        RadioRole, VlanRef,
    },
};

#[derive(cynic::QueryVariables, Debug)]
pub struct NetboxPlanVariables {
    pub helper: Helper,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "NetboxPlanVariables",
)]
pub struct NetboxPlanQuery {
    pub allow_netbox_write: bool,
    #[arguments(helper: $helper)]
    pub netbox_plan: NetboxPlan,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NetboxPlan {
    pub fingerprint: String,
    pub checked_with: Option<String>,
    pub undocumented: Vec<UndocumentedAddress>,
    pub cut_off: Vec<DeviceRef>,
    pub apply_together: Vec<DeviceRef>,
    pub emptied_bridge: Option<InterfaceRef>,
    pub steps: Vec<PlanStep>,
}

/// A step of a plan; one this page doesn't know yet is `Unknown`.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub enum PlanStep {
    CreateDevice(CreateDevice),
    CreateInterface(CreateInterface),
    CreateAddress(CreateAddress),
    SetPrimaryIp(SetPrimaryIp),
    CreateCable(CreateCable),
    CreatePrefix(CreatePrefix),
    UpdateInterface(UpdateInterface),
    DeleteCable(DeleteCable),
    DeleteAddress(DeleteAddress),
    DeletePrefix(DeletePrefix),
    UpdatePort(UpdatePort),
    CreateWirelessLink(CreateWirelessLink),
    UpdateWirelessLink(UpdateWirelessLink),
    DeleteWirelessLink(DeleteWirelessLink),
    UpdatePortVlan(UpdatePortVlan),
    DeleteInterface(DeleteInterface),
    #[cynic(fallback)]
    Unknown,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CreateDevice {
    pub name: String,
    pub site: String,
    pub device_type: String,
    pub role: String,
    pub wlan_group: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CreateInterface {
    pub device: DeviceRef,
    pub name: String,
    pub kind: String,
    pub tags: Vec<InterfaceTag>,
    pub mac: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CreateAddress {
    pub address: String,
    pub device: DeviceRef,
    pub interface: String,
    pub prefix: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SetPrimaryIp {
    pub device: DeviceRef,
    pub version: IpVersion,
    pub address: String,
    pub before: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CreateCable {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CreatePrefix {
    pub prefix: String,
    pub pool: String,
    pub description: String,
    pub dhcp_lease_time: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateInterface {
    pub interface: InterfaceRef,
    pub label: Option<String>,
    pub label_before: Option<String>,
    pub mtu: Option<i32>,
    pub mtu_before: Option<i32>,
    pub tags_added: Vec<InterfaceTag>,
    pub tags_removed: Vec<InterfaceTag>,
    pub poe: Option<PoeMode>,
    pub poe_before: Option<PoeMode>,
    pub poe_removed: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeleteCable {
    pub port: InterfaceRef,
    pub far: Option<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeleteAddress {
    pub address: String,
    pub interface: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeletePrefix {
    pub prefix: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdatePort {
    pub interface: InterfaceRef,
    pub name: Option<String>,
    pub rf_role: Option<RadioRole>,
    pub rf_role_before: Option<RadioRole>,
    pub bridge: Option<String>,
    pub leaves_bridge: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CreateWirelessLink {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
    pub ssid: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateWirelessLink {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
    pub ssid: Option<String>,
    pub ssid_before: String,
    pub psk_set: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeleteWirelessLink {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdatePortVlan {
    pub interface: InterfaceRef,
    pub vlan: Option<VlanRef>,
    pub vlan_before: Option<VlanRef>,
    pub tagged_before: Vec<VlanRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeleteInterface {
    pub interface: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UndocumentedAddress {
    pub address: String,
    pub route: String,
    pub gateway: Option<String>,
    pub router: String,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct NetboxPlanPreviewVariables {
    pub id: i32,
    pub helper: Helper,
    pub target: Option<String>,
    pub credential_name: Option<String>,
    pub adhoc_credentials: Option<AdhocCredentials>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "NetboxPlanPreviewVariables",
)]
pub struct NetboxPlanPreview {
    pub topology: PreviewTopology,
    #[arguments(helper: $helper)]
    pub netbox_plan: PlanPreview,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "NetboxPlanPreviewVariables",
)]
pub struct PreviewTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<FindingKinds>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "NetboxPlan",
    variables = "NetboxPlanPreviewVariables",
)]
pub struct PlanPreview {
    pub preview: FindingKinds,
    #[arguments(
        target: $target,
        credentialName: $credential_name,
        adhocCredentials: $adhoc_credentials,
    )]
    pub preview_access: Option<PreviewAccess>,
}

/// A device's findings, of which the preview shows the kinds.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct FindingKinds {
    pub findings: Vec<Finding>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "AccessibleDevice")]
pub struct PreviewAccess {
    pub generate_cfg: Option<PreviewCfg>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "GeneratedCfg")]
pub struct PreviewCfg {
    pub script: String,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct WriteNetboxPlanVariables {
    pub helper: Helper,
    pub fingerprint: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "WriteNetboxPlanVariables",
)]
pub struct WriteNetboxPlan {
    #[arguments(helper: $helper, fingerprint: $fingerprint)]
    pub write_netbox_plan: JobId,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct UndoNetboxWriteVariables {
    pub job_id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "UndoNetboxWriteVariables",
)]
pub struct UndoNetboxWrite {
    #[arguments(jobId: $job_id)]
    pub undo_netbox_write: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Job")]
pub struct JobId {
    pub id: i32,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct HelperJobsVariables {
    pub device_id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "HelperJobsVariables",
)]
pub struct HelperJobs {
    #[arguments(deviceId: $device_id)]
    pub jobs: Vec<HelperJob>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Job")]
pub struct HelperJob {
    pub id: i32,
    pub kind: JobKind,
    pub state: JobState,
    pub netbox_written_count: i32,
}
