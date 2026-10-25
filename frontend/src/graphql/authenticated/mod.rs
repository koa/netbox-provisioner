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
    response_derives = "Debug, Clone, PartialEq",
    skip_serializing_none
)]
pub struct Findings;

/// Serializes, to hand its findings on as the type of `Findings`.
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/interfaces.graphql",
    response_derives = "Debug, Clone, PartialEq, Serialize"
)]
pub struct InterfaceView;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/interface-live.graphql",
    response_derives = "Debug, Clone, PartialEq, Serialize",
    skip_serializing_none
)]
pub struct InterfaceLive;

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

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/batch-apply.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct BatchCandidates;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/batch-apply.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct BatchDevice;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/batch-apply.graphql",
    response_derives = "Debug"
)]
pub struct ApplyBatch;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/site-diff.graphql",
    response_derives = "Debug"
)]
pub struct DiffSite;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/site-diff.graphql",
    response_derives = "Debug, PartialEq"
)]
pub struct SiteDiffJobs;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/site-diff.graphql",
    response_derives = "Debug, Clone, PartialEq"
)]
pub struct SiteDiffEvents;
