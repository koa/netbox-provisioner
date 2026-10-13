use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// In the ID token, which the frontend sends; missing when it falls back to Pocket-ID's
    /// access token (only `sub`, `client_id` and `scope`)
    pub name: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub hd: Option<String>,
}

#[cfg(test)]
mod test {
    use super::UserInfo;
    use serde_json::json;

    #[test]
    fn decode_pocket_id_access_token() {
        let claims = json!({
            "client_id": "ce24efe9-5a7e-4883-932f-c05510db54a3",
            "scope": "openid profile email",
            "scp": ["openid", "profile", "email"],
        });
        let user: UserInfo = serde_json::from_value(claims).unwrap();
        assert_eq!(user.name, None);
    }
}
