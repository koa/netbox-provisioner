use graphql_client::GraphQLQuery;

type ScalarDuration = u64;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct ListDevices;

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
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct DetectDeviceType;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/show-device.graphql",
    response_derives = "Debug"
)]
pub struct DeviceOverview;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/authenticated/schema.graphql",
    query_path = "src/graphql/authenticated/adjust-target.graphql",
    response_derives = "Debug"
)]
pub struct AdjustTargetListCredentials;

