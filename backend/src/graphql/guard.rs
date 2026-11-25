//! Scope: The guards of the GraphQL root fields: who may call one (the role the request
//! carries) and what the config allows; not finding the role (`role`) nor ownership, which the
//! residents' resolvers check at each object.
//!
//! Every root field has one: [`Admin`] on the admins' roots (`QueryAdmin`, `MutationAdmin`,
//! `SubscriptionAdmin`, as the object's guard), [`ResidentOrAdmin`] on the residents' (a resident or an
//! admin), none on `QueryOpen` only. A field with a guard of its own replaces the object's, so
//! it names the role too (`Admin.and(WritesAllowed)`). The test `graphql::test::rights` calls
//! every root field as a resident and without login.
use crate::{Error, config::CONFIG, error_info::ApiError, netbox, role::Role};
use async_graphql::{Context, Guard};

/// Lets only an admin through.
pub struct Admin;
impl Guard for Admin {
    async fn check(&self, ctx: &Context<'_>) -> async_graphql::Result<()> {
        match ctx.data_opt::<Role>() {
            Some(Role::Admin) => Ok(()),
            _ => Err(ApiError(Error::NotAllowed).into()),
        }
    }
}

/// Lets a resident or an admin through; what each may see or change the resolver decides by
/// the owners.
pub struct ResidentOrAdmin;
impl Guard for ResidentOrAdmin {
    async fn check(&self, ctx: &Context<'_>) -> async_graphql::Result<()> {
        match ctx.data_opt::<Role>() {
            Some(Role::Admin | Role::Resident { .. }) => Ok(()),
            _ => Err(ApiError(Error::NotAllowed).into()),
        }
    }
}

/// Lets a mutation that writes to devices through only with `allow_apply`; every such
/// mutation carries it, the work below doesn't ask again.
pub struct WritesAllowed;
impl Guard for WritesAllowed {
    async fn check(&self, _ctx: &Context<'_>) -> async_graphql::Result<()> {
        if CONFIG.allow_apply {
            Ok(())
        } else {
            Err(ApiError(Error::ApplyNotAllowed).into())
        }
    }
}

/// Lets a write to NetBox through only with `allow_netbox_write` and `netbox_write_token`.
pub struct NetboxWritesAllowed;
impl Guard for NetboxWritesAllowed {
    async fn check(&self, _ctx: &Context<'_>) -> async_graphql::Result<()> {
        if CONFIG.allow_netbox_write && CONFIG.netbox_write_token.is_some() {
            Ok(())
        } else {
            Err(ApiError(Error::Netbox(netbox::NetboxError::WriteNotAllowed)).into())
        }
    }
}
