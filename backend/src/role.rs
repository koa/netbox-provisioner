//! Scope: Who a logged-in user is to the provisioner, an admin or a resident with its owners
//! (`docs/self-service.md`, „Rollen“); not checking the token (`binary`), nor
//! what an owner has (`topology::ownership`).
use crate::{config::CONFIG, context::UserInfo, lock::LockAnyway, netbox, topology::OwnerId};
use log::warn;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tokio::time::Instant;

#[cfg(test)]
mod test;

/// The group of the provider that makes an admin.
pub const ADMIN_GROUP: &str = "admin";
/// How long the owners of a user are taken as NetBox gave them: every request of the resident
/// UI asks, and a change in NetBox reaches it within that.
const OWNERS_AGE: Duration = Duration::from_secs(30);

/// A logged-in user's role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// in `auth_users` or the group [`ADMIN_GROUP`]: everything
    Admin,
    /// a member of these NetBox owners
    Resident { owners: Box<[OwnerId]> },
    /// neither: nothing
    Nobody,
}

/// Whether the token's `subject` and claims make an admin: `auth_users`, the emergency switch
/// until the groups are reliable, or the group [`ADMIN_GROUP`].
pub fn is_admin(subject: Option<&str>, user: Option<&UserInfo>) -> bool {
    admin_by(&CONFIG.auth_users, subject, user)
}

fn admin_by(auth_users: &[Box<str>], subject: Option<&str>, user: Option<&UserInfo>) -> bool {
    subject.is_some_and(|subject| auth_users.iter().any(|id| **id == *subject))
        || user.is_some_and(|user| user.groups.iter().any(|group| &**group == ADMIN_GROUP))
}

/// The role of a logged-in user: an admin, else a resident by the NetBox owners its
/// `preferred_username` is a member of. NetBox unreachable makes nobody, with a warning.
pub async fn of(subject: Option<&str>, user: Option<&UserInfo>) -> Role {
    if is_admin(subject, user) {
        return Role::Admin;
    }
    let Some(username) = user.and_then(|user| user.preferred_username.as_deref()) else {
        return Role::Nobody;
    };
    match owners_of(username).await {
        Ok(owners) if owners.is_empty() => Role::Nobody,
        Ok(owners) => Role::Resident { owners },
        Err(error) => {
            warn!("owners of {username} not read from NetBox: {error}");
            Role::Nobody
        }
    }
}

/// The owners NetBox has `username` in, at most [`OWNERS_AGE`] old.
async fn owners_of(username: &str) -> Result<Box<[OwnerId]>, netbox::NetboxError> {
    static KNOWN: LazyLock<Mutex<HashMap<Box<str>, Known>>> = LazyLock::new(Default::default);
    struct Known {
        at: Instant,
        owners: Box<[OwnerId]>,
    }
    if let Some(known) = KNOWN.lock_anyway().get(username)
        && known.at.elapsed() < OWNERS_AGE
    {
        return Ok(known.owners.clone());
    }
    let owners = netbox::owners::owners_of_user(username).await?;
    let mut known = KNOWN.lock_anyway();
    known.retain(|_, known| known.at.elapsed() < OWNERS_AGE);
    known.insert(
        username.into(),
        Known {
            at: Instant::now(),
            owners: owners.clone(),
        },
    );
    Ok(owners)
}
