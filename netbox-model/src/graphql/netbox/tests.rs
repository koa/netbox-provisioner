use super::*;

#[test]
fn fetch_topology_query_gql_output() {
    use cynic::QueryBuilder;

    let operation = FetchTopology::build(());

    insta::assert_snapshot!(operation.query);
}

#[test]
fn test_parse_response() {
    let result = serde_json::from_str::<FetchTopology>(include_str!("example.json")).unwrap();
    println!("{:#?}", result);
}
