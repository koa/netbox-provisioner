use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub name: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
}
