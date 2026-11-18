//! Scope: Tests of `role`; nothing else.
use super::admin_by;
use crate::context::UserInfo;
use serde_json::json;

fn user(claims: serde_json::Value) -> UserInfo {
    serde_json::from_value(claims).expect("claims")
}

/// `auth_users` or the group `admin` make an admin; another group or none doesn't.
#[test]
fn an_admin_is_in_auth_users_or_the_admin_group() {
    let auth_users: Box<[Box<str>]> = Box::new(["lab-admin".into()]);
    let in_group = user(json!({"groups": ["users", "admin"]}));
    let resident = user(json!({"groups": ["users"], "preferred_username": "meier"}));
    let without = user(json!({}));
    assert!(admin_by(&auth_users, Some("lab-admin"), Some(&without)));
    assert!(admin_by(&auth_users, Some("someone"), Some(&in_group)));
    assert!(!admin_by(&auth_users, Some("someone"), Some(&resident)));
    assert!(!admin_by(&auth_users, Some("someone"), Some(&without)));
    assert!(!admin_by(&auth_users, None, None));
}

/// A token without `groups` (Pocket-ID's access token) still decodes, with none.
#[test]
fn a_token_without_groups_has_none() {
    let claims = user(json!({"preferred_username": "meier"}));
    assert!(claims.groups.is_empty());
    assert_eq!(claims.preferred_username.as_deref(), Some("meier"));
}
