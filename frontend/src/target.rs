//! Scope: Where and as whom a device is reached when the operator picks another address or
//! credentials; not the form for it (`components::device::adjust_target`).
//!
//! Where and as whom a device is reached, when the operator picks another address or other
//! credentials than NetBox's (`components::device::adjust_target`); every view asking the device
//! takes it.
use crate::graphql::authenticated::types::{AdhocCredentials, DeviceTarget};
use std::net::IpAddr;

#[derive(Debug, Clone, PartialOrd, PartialEq, Default)]
pub enum SelectedCredentials {
    #[default]
    Default,
    Named(Box<str>),
    Adhoc {
        username: Box<str>,
        password: Box<str>,
    },
}

#[derive(Debug, Clone, PartialOrd, PartialEq, Default)]
pub struct SelectedTarget {
    pub address: Option<IpAddr>,
    pub credentials: SelectedCredentials,
}

impl SelectedTarget {
    /// As the queries take it (`DeviceTarget`).
    pub fn input(&self) -> DeviceTarget {
        let (credential_name, adhoc_credentials) = match &self.credentials {
            SelectedCredentials::Default => (None, None),
            SelectedCredentials::Named(name) => (Some(name.to_string()), None),
            SelectedCredentials::Adhoc { username, password } => (
                None,
                Some(AdhocCredentials {
                    username: Some(username.to_string()),
                    password: Some(password.to_string()),
                }),
            ),
        };
        DeviceTarget {
            address: self.address.map(|address| address.to_string()),
            credential_name,
            adhoc_credentials,
        }
    }
}
