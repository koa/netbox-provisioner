use crate::{
    device::AccessibleDevice,
    topology::access::{
        DeviceAccess, InterfaceAccess, VlanAccess, VxlanAccess, WlanAccess, WlanGroupAccess,
    },
};
use async_graphql::Object;
use ipnet::IpNet;

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
    async fn access(&self) -> Option<AccessibleDevice> {
        self.clone().into()
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
