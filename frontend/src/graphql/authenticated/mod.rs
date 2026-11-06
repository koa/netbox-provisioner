//! The queries of the authenticated endpoint (`/graphql`), checked against its schema
//! (`schema.graphql`, `schema`); one module per page or component. Those still bound through
//! graphql_client follow below.
use graphql_client::GraphQLQuery;

pub mod adjust_target;
pub mod batch_apply;
pub mod findings;
pub mod helper;
pub mod history;
pub mod interface_live;
pub mod interfaces;
pub mod job;
pub mod list_devices;
pub mod neighbor_findings;
pub mod network_map;
pub mod saved_jobs;
pub mod schema;
pub mod site_diff;
pub mod types;

/// `D4:01:C3:29:67:64`; the backend checks it
type ScalarMac = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct DeviceOverview;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct AllowApply;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct RefreshDevice;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct ApplyDeviceCfg;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct BootstrapDevice;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct UpdateRouteros;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct DowngradeRouteros;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct ClearLeftover;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct RebootDevice;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct PowerCycle;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct ChangeDeviceMode;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct RepartitionDevice;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct BootPartition;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
pub struct BootstrapCandidates;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq",
    skip_serializing_none
)]
pub struct NetboxPlan;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq",
    skip_serializing_none
)]
pub struct NetboxPlanPreview;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug",
    skip_serializing_none
)]
pub struct WriteNetboxPlan;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug"
)]
pub struct UndoNetboxWrite;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug"
)]
pub struct HelperJobs;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct LinkPorts;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct FlagInterfaces;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct CustomerNetOptions;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct PoeOptions;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct PortVlanOptions;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct NewDeviceOptions;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct LeaseTakeoverOptions;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct NewDeviceDevices;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct PoeObserved;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct RadioBridgeCandidates;
