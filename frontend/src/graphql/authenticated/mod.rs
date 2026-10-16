use graphql_client::GraphQLQuery;

type ScalarDuration = u64;
/// `D4:01:C3:29:67:64`; the backend checks it
type ScalarMac = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct ListSites;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct SiteDevices;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct PingDevice;

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
    query_path = "src/graphql/authenticated/adjust-target.graphql",
    response_derives = "Debug"
)]
pub struct AdjustTargetListCredentials;

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
pub struct RebootDevice;

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
    query_path = "src/graphql/authenticated/job.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct JobEvents;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/job.graphql",
    response_derives = "Debug"
)]
pub struct DeviceJobs;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/job.graphql",
    response_derives = "Debug"
)]
pub struct CancelJob;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/findings.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct Findings;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/neighbor-findings.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct NeighborFindings;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct NetboxPlan;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct NetboxPlanPreview;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/netbox-helper.graphql",
    response_derives = "Debug"
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
