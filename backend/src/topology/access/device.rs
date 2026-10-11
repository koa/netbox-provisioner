use crate::{
    Error,
    device::{
        AccessibleDevice, Credentials, Path,
        bootstrap::{self, BootstrapPlan},
    },
    topology::{
        CablePort, Device, DeviceId, DeviceRole, Topology,
        access::{
            AccessTopology, AdhocCredentials, interface::InterfaceAccess,
            ip_addresses::IpAddressAccess, wlan_group::WlanGroupAccess,
        },
    },
};
use async_graphql::Object;
use log::{error, info};
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::time::timeout;

/// Includes the login; through the management proxy a missing route fails at once, a host that
/// is down only after the TCP timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

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
        self.data()
            .and_then(|d| d.primary_ip)
            .map(self.create_access())
            .and_then(|a: IpAddressAccess| a.addr())
    }
    pub fn primary_ip_v4(&self) -> Option<IpAddressAccess> {
        self.data()
            .and_then(|d| d.primary_ip_v4)
            .map(self.create_access())
    }
    pub fn primary_ip_v6(&self) -> Option<IpAddressAccess> {
        self.data()
            .and_then(|d| d.primary_ip_v6)
            .map(self.create_access())
    }
    /// Logs in to the device: at `target` or else its primary IP, with the named or the ad-hoc
    /// credentials or else those of its tenant. `None` without address or credentials.
    ///
    /// Without `target`, a device that doesn't answer at its primary IP is tried at the address
    /// its [initial setup](bootstrap) gives it, through a tunnel to the neighbor it was set up
    /// from: before its first apply that is all it has, and no route back.
    pub async fn connect(
        &self,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Option<AccessibleDevice>, Error> {
        let fallback = if target.is_none() {
            bootstrap::plan(self)
                .ok()
                .filter(|plan| Some(plan.port_address()) != self.primary_ip())
        } else {
            None
        };
        let addr = target.and_then(|ip| ip.parse().ok()).or(self.primary_ip());
        let credentials = if let Some(credential_name) = credential_name {
            Some(Credentials::Named(credential_name))
        } else if let Some(AdhocCredentials { username, password }) = adhoc_credentials {
            Some(Credentials::Adhoc { username, password })
        } else {
            self.credentials()
                .map(|cred| Credentials::Named(cred.to_string().into()))
        };
        let Some(credentials) = credentials else {
            return Ok(None);
        };
        let first = match addr {
            Some(address) => timeout(
                CONNECT_TIMEOUT,
                AccessibleDevice::create_client(
                    self.clone(),
                    address,
                    Path::Routed,
                    credentials.clone(),
                ),
            )
            .await
            .unwrap_or(Err(Error::Timeout(address))),
            None => Err(Error::NoAddress(self.name().into())),
        };
        Ok(Some(match (first, fallback) {
            (Ok(device), _) => device,
            (Err(error), Some(plan)) => {
                let address = plan.port_address();
                info!(
                    "{} not reachable ({error}), trying {address} through {}",
                    self.name(),
                    plan.neighbor
                );
                timeout(CONNECT_TIMEOUT, async {
                    let path = Path::Neighbor(bootstrap::tunnel(&plan).await?.into());
                    AccessibleDevice::create_client(self.clone(), address, path, credentials).await
                })
                .await
                .unwrap_or(Err(Error::Timeout(address)))?
            }
            (Err(Error::NoAddress(_)), None) => return Ok(None),
            (Err(error), None) => return Err(error),
        }))
    }

    pub fn loopback_ip(&self) -> Option<IpAddressAccess> {
        self.data()
            .and_then(|d| d.loopback_ip)
            .map(self.create_access())
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
    /// The group this device is a CAP of with `/interface/wifi` (`Topology::is_legacy_cap`).
    pub fn wifi_cap_of(&self) -> Option<WlanGroupAccess> {
        self.wlan_ap_of().filter(|_| !self.is_legacy_cap())
    }
    /// The group this device is a CAP of with `/interface/wireless`
    /// (`Topology::is_legacy_cap`).
    pub fn legacy_cap_of(&self) -> Option<WlanGroupAccess> {
        self.wlan_ap_of().filter(|_| self.is_legacy_cap())
    }
    fn is_legacy_cap(&self) -> bool {
        self.topology.is_legacy_cap(self.id)
    }

    pub fn role(&self) -> Option<DeviceRole> {
        self.data().and_then(|d| d.role)
    }
    /// ISO 3166 code of the country the device's site is in (its topmost region).
    pub fn country(&self) -> Option<&str> {
        self.data().and_then(|d| d.country.as_deref())
    }
    /// The DNS servers of the device's client networks (config context `dns_servers`).
    pub fn dns_servers(&self) -> &[IpAddr] {
        self.data().map(|d| &*d.dns_servers).unwrap_or_default()
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
    async fn management_address(&self) -> Option<IpAddressAccess> {
        self.data()
            .and_then(|a| a.primary_ip)
            .map(self.create_access())
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
        match self
            .connect(target, credential_name, adhoc_credentials)
            .await
        {
            Ok(c) => c,
            Err(error) => {
                error!("Cannot access device {}: {}", self.name(), error);
                None
            }
        }
    }
    /// How the device would be set up over mac-telnet, see [`MutationAuthenticated::bootstrap`].
    #[graphql(name = "bootstrapPlan")]
    async fn api_bootstrap_plan(&self) -> Option<BootstrapPlan> {
        bootstrap::plan(self).ok()
    }
    /// Why there is no `bootstrapPlan`.
    async fn bootstrap_problem(&self) -> Option<String> {
        bootstrap::plan(self).err().map(|e| e.to_string())
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
