use graphql_client::GraphQLQuery;

type ScalarDuration = u64;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "target/authenticated_schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct ListDevices;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "target/authenticated_schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct PingDevice;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "target/authenticated_schema.graphql",
    query_path = "src/graphql/authenticated/list-devices.graphql",
    response_derives = "Debug"
)]
pub struct DetectDeviceType;
