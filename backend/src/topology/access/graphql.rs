use crate::{
    device::{AccessibleDevice, Credentials},
    topology::access::{
        AccessTopology, DeviceAccess, InterfaceAccess, VlanAccess, VxlanAccess, WlanAccess,
        WlanGroupAccess,
    },
};
use async_graphql::{InputObject, Object};
use ipnet::IpNet;
use log::error;

#[derive(InputObject)]
struct AdhocCredentials {
    username: Option<Box<str>>,
    #[graphql(secret)]
    password: Option<Box<str>>,
}

#[Object]
impl DeviceAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name()
    }
    async fn management_address(&self) -> Option<String> {
        self.data()
            .and_then(|a| a.primary_ip)
            .map(|s| s.to_string())
    }
    #[graphql(name = "serial")]
    async fn api_serial(&self) -> Option<String> {
        self.serial().map(ToString::to_string)
    }
    async fn access(
        &self,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Option<AccessibleDevice> {
        let addr = target.and_then(|ip| ip.parse().ok()).or(self.primary_ip());
        let credentials = if let Some(credential_name) = credential_name {
            Some(Credentials::Named(credential_name))
        } else if let Some(AdhocCredentials { username, password }) = adhoc_credentials {
            Some(Credentials::Adhoc { username, password })
        } else {
            self.credentials()
                .map(|cred| Credentials::Named(cred.to_string().into()))
        };
        if let (Some(address), Some(credentials)) = (addr, credentials) {
            let client = AccessibleDevice::create_client(self.clone(), address, credentials).await;
            match client {
                Ok(c) => Some(c),
                Err(error) => {
                    error!("Cannot access device {}: {}", self.name(), error);
                    None
                }
            }
        } else {
            None
        }
    }
    #[graphql(name = "wlanControllerOf")]
    async fn api_is_wlan_controller_of(&self) -> Option<WlanGroupAccess> {
        self.wlan_controller_of()
    }
    #[graphql(name = "wlanApOf")]
    async fn api_is_wlan_ap_of(&self) -> Option<WlanGroupAccess> {
        self.wlan_ap_of()
    }
}

#[Object]
impl WlanGroupAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "wlanList")]
    async fn api_wlan_list(&self) -> Box<[WlanAccess]> {
        self.wlan().collect()
    }
    #[graphql(name = "controller")]
    async fn api_controller(&self) -> Option<DeviceAccess> {
        self.controller()
    }
    #[graphql(name = "aps")]
    async fn api_aps(&self) -> Box<[DeviceAccess]> {
        self.aps()
    }
}
#[Object]
impl WlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
}

#[Object]
impl VxlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "vni")]
    async fn api_vni(&self) -> Option<u32> {
        self.vni()
    }
    #[graphql(name = "interfaceTerminations")]
    async fn api_interface_terminations(&self) -> Box<[InterfaceAccess]> {
        self.interface_terminations()
    }
    #[graphql(name = "vlanTerminations")]
    async fn api_vlan_terminations(&self) -> Box<[VlanAccess]> {
        self.vlan_terminations()
    }
}

#[Object]
impl InterfaceAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name()
    }
    #[graphql(name = "ips")]
    async fn api_ips(&self) -> Box<[IpNetGraphql]> {
        self.ips().iter().copied().map(IpNetGraphql).collect()
    }
}
#[Object]
impl VlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name().unwrap_or_default()
    }
    async fn api_vlan_id(&self) -> u16 {
        self.vlan_id().expect("vlan_id not set")
    }
}
struct IpNetGraphql(IpNet);

#[Object]
impl IpNetGraphql {
    async fn ip(&self) -> String {
        self.0.addr().to_string()
    }
    async fn net(&self) -> String {
        self.0.network().to_string()
    }
    async fn mask(&self) -> u8 {
        self.0.prefix_len()
    }
    async fn display(&self) -> String {
        self.0.to_string()
    }
}
