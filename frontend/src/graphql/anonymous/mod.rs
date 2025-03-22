use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "target/anonymous_schema.graphql",
    query_path = "src/graphql/anonymous/settings.graphql",
    response_derives = "Debug"
)]
pub struct Settings;
