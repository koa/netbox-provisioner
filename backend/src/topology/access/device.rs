use crate::{
    device::{AccessibleDevice, Credentials},
    topology::{
        CablePort, Device, DeviceId, Topology,
        access::{
            AccessTopology, AdhocCredentials, interface::InterfaceAccess, vlan::VlanAccess,
            vxlan::VxlanAccess, wlan_group::WlanGroupAccess,
        },
    },
};
use async_graphql::Object;
use log::error;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceAccess {
    topology: Arc<Topology>,
    id: DeviceId,
}

impl AccessTopology for DeviceAccess {
    type Id = DeviceId;
    type Data = Device;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.devices.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        DeviceAccess { topology, id }
    }
}

impl DeviceAccess {
    pub fn id(&self) -> DeviceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name()).unwrap_or_default()
    }
    pub fn serial(&self) -> Option<&str> {
        self.data().and_then(|d| d.serial.as_deref())
    }
    pub fn primary_ip(&self) -> Option<IpAddr> {
        self.data().and_then(Device::primary_ip)
    }
    pub fn primary_ip_v4(&self) -> Option<Ipv4Addr> {
        self.data().and_then(|d| d.primary_ip_v4)
    }
    pub fn primary_ip_v6(&self) -> Option<Ipv6Addr> {
        self.data().and_then(|d| d.primary_ip_v6)
    }

    pub fn loopback_ip(&self) -> Option<IpAddr> {
        self.data().and_then(|d| d.loopback_ip)
    }

    pub fn has_routeros(&self) -> bool {
        self.data().map(|d| d.has_routeros).unwrap_or(false)
    }
    pub fn credentials(&self) -> Option<&str> {
        self.data().and_then(|d| Device::credentials(d))
    }
    pub fn interfaces<'a>(&'a self) -> Box<[InterfaceAccess]> {
        self.topology
            .devices
            .get(&self.id)
            .map(|data| {
                data.ports
                    .iter()
                    .filter_map(|p| {
                        if let CablePort::Interface(p) = p {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .copied()
                    .map(self.create_access())
                    .collect::<Box<[_]>>()
            })
            .unwrap_or_default()
    }
    pub fn wlan_controller_of(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_controller_of)
            .map(self.create_access())
    }
    pub fn wlan_ap_of(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_ap_of)
            .map(self.create_access())
    }

    pub fn vlans(&self) -> impl Iterator<Item = VlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.vlans.iter().cloned())
            .map(self.create_access())
    }

    pub fn vxlan(&self) -> HashSet<VxlanAccess> {
        self.vlans().filter_map(|vl| vl.vxlan()).collect()
    }
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
