//! Scope: The query for the NetBox owners a NetBox user is a member of; not what an owner has
//! (`topology::ownership`), nor who logs in (`resident`).
use crate::netbox::NetboxError;
use crate::topology::OwnerId;
use cynic::{Id, QueryBuilder};

#[cynic::schema("netbox")]
mod schema {}

#[derive(cynic::QueryVariables, Debug)]
pub struct OwnersOfUserVariables {
    pub username: String,
}

/// The owners with the user among their `users`: NetBox doesn't give an owner's users out,
/// but filters by them.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    graphql_type = "Query",
    variables = "OwnersOfUserVariables",
    rename_all = "None"
)]
pub struct OwnersOfUser {
    #[arguments(filters: { users: { username: { exact: $username } } })]
    pub owner_list: Vec<OwnerOfUser>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "OwnerType", rename_all = "None")]
pub struct OwnerOfUser {
    pub id: Id,
}

/// The owners the NetBox user `username` is a member of.
pub async fn owners_of_user(username: &str) -> Result<Box<[OwnerId]>, NetboxError> {
    let data = super::query(OwnersOfUser::build(OwnersOfUserVariables {
        username: username.into(),
    }))
    .await?;
    Ok(data
        .owner_list
        .iter()
        .filter_map(|owner| owner.id.inner().parse().ok().map(OwnerId))
        .collect())
}
