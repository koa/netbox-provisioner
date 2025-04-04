use crate::device::AccessibleDevice;
use crate::topology::access::{DeviceAccess, InterfaceAccess, VxlanAccess, WlanGroupAccess};
use crate::topology::WlanData;
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
    async fn api_wlan_list(&self) -> &[WlanData] {
        self.wlan()
    }
    #[graphql(name = "controller")]
    async fn api_controller(&self) -> Option<DeviceAccess> {
        self.controller()
    }
    #[graphql(name = "aps")]
    async fn api_aps(&self) -> Box<[DeviceAccess]> {
        self.aps()
    }
    #[graphql(name = "transportVxlan")]
    async fn api_transport_vxlan(&self) -> Option<VxlanAccess> {
        self.transport_vxlan()
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
    #[graphql(name = "terminations")]
    async fn api_terminations(&self) -> Box<[InterfaceAccess]> {
        self.terminations()
    }
    #[graphql(name = "wlanGroup")]
    async fn api_wlan_group(&self) -> Option<WlanGroupAccess> {
        self.wlan_group()
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
