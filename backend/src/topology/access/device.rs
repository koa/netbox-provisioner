use crate::error_info::ErrorInfo;
use crate::{
    Error,
    device::{
        AccessibleDevice, Credentials, Path,
        bootstrap::{self, BootstrapPlan},
    },
    topology::{
        CablePort, Device, DeviceId, DeviceRole, Topology,
        findings::Finding,
        access::{
            AccessTopology, AdhocCredentials, interface::InterfaceAccess,
            ip_addresses::IpAddressAccess, site::SiteAccess, wlan_group::WlanGroupAccess,
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
    pub(crate) fn topology(&self) -> &Arc<Topology> {
        &self.topology
    }
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
    /// its [initial setup](bootstrap) gives it, through a tunnel to a neighbor on the cable
    /// (every one of [`bootstrap::plans`] in turn): before its first apply that is all it has,
    /// and no route back, and a device whose OSPF is cut off is still reached like that.
    pub async fn connect(
        &self,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Option<AccessibleDevice>, Error> {
        let fallbacks = if target.is_none() {
            bootstrap::plans(self)
                .unwrap_or_default()
                .into_iter()
                .filter(|plan| Some(plan.port_address()) != self.primary_ip())
                .collect()
        } else {
            Vec::new()
        };
        let addr = target
            .and_then(|ip| ip.parse().ok())
            .or(bootstrap::reachable_address(self));
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
        let mut error = match first {
            Ok(device) => return Ok(Some(device)),
            Err(Error::NoAddress(_)) if fallbacks.is_empty() => return Ok(None),
            Err(error) => error,
        };
        for plan in fallbacks {
            let address = plan.port_address();
            info!(
                "{} not reachable ({error}), trying {address} through {}",
                self.name(),
                plan.neighbor
            );
            // directly first: the port's address may well be routed to (berghof-stall-haus'
            // 10.198.1.4 was, from the provisioner's own network), only the device has no
            // route back to the provisioner on all of its sites' networks
            match timeout(
                CONNECT_TIMEOUT,
                AccessibleDevice::create_client(self.clone(), address, Path::Routed, credentials.clone()),
            )
            .await
            .unwrap_or(Err(Error::Timeout(address)))
            {
                Ok(device) => return Ok(Some(device)),
                Err(e) => info!("{}: {address} not reachable directly ({e})", self.name()),
            }
            let credentials = credentials.clone();
            match timeout(CONNECT_TIMEOUT, async {
                let path = Path::Neighbor(plan.into());
                AccessibleDevice::create_client(self.clone(), address, path, credentials).await
            })
            .await
            .unwrap_or(Err(Error::Timeout(address)))
            {
                Ok(device) => return Ok(Some(device)),
                Err(e) => error = e,
            }
        }
        Err(error)
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
    pub fn interfaces(&self) -> Box<[InterfaceAccess]> {
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
    pub fn site(&self) -> Option<SiteAccess> {
        self.data().and_then(|d| d.site).map(self.create_access())
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
    /// Whether its OSPF announces its default route (custom field `propagate_default_route`).
    pub fn propagates_default_route(&self) -> bool {
        self.data().is_some_and(|d| d.propagate_default_route)
    }
    /// The DNS servers of the device's client networks (config context `dns_servers`).
    pub fn dns_servers(&self) -> &[IpAddr] {
        self.data().map(|d| &*d.dns_servers).unwrap_or_default()
    }
    /// How long a new SSH connection from the internet keeps its source in a stage of the
    /// bruteforce protection (config context `ssh_stage_time`, else a minute, as MikroTik's
    /// guide has it).
    pub fn ssh_stage_time(&self) -> Duration {
        self.data().and_then(|d| d.ssh_stage_time).unwrap_or(Duration::from_secs(60))
    }
    /// How long a source the bruteforce protection caught stays blocked (config context
    /// `ssh_blacklist_time`, else 10 days, as MikroTik's guide has it).
    pub fn ssh_blacklist_time(&self) -> Duration {
        self.data()
            .and_then(|d| d.ssh_blacklist_time)
            .unwrap_or(Duration::from_secs(10 * 24 * 3600))
    }
}

/// A device to apply together with another (`batchCandidates`).
#[derive(async_graphql::SimpleObject)]
pub struct BatchCandidate {
    device: DeviceAccess,
    /// at the other end of one of its wireless links
    partner: bool,
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
    #[graphql(name = "site")]
    async fn api_site(&self) -> Option<SiteAccess> {
        self.site()
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
        ctx: &async_graphql::Context<'_>,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Option<AccessibleDevice> {
        // the page shows why next to "unreachable" (an error at this path, which a returned
        // error would put on the whole device); `None` also without credentials or an address
        let result = self
            .connect(target, credential_name, adhoc_credentials)
            .await
            .inspect_err(|error| error!("Cannot access device {}: {}", self.name(), error));
        crate::device::ros::graphql::nullable(ctx, result).flatten()
    }
    /// The devices to apply together with this one (`applyBatch`): the other ends of its
    /// wireless links (`partner`), then the RouterOS devices of its site and theirs.
    async fn batch_candidates(&self) -> Vec<BatchCandidate> {
        let partners = crate::device::batch::wireless_partners(self);
        let mut sites: Vec<SiteAccess> = self.site().into_iter().collect();
        sites.extend(partners.iter().filter_map(DeviceAccess::site));
        let mut seen = std::collections::HashSet::from([self.id()]);
        let mut candidates: Vec<BatchCandidate> = Vec::new();
        let mut partners = partners;
        partners.sort_by(|a, b| a.name().cmp(b.name()));
        for device in partners {
            if seen.insert(device.id()) {
                candidates.push(BatchCandidate { device, partner: true });
            }
        }
        let mut others: Vec<DeviceAccess> = sites
            .iter()
            .flat_map(|site| site.devices())
            .filter(DeviceAccess::has_routeros)
            .collect();
        others.sort_by(|a, b| a.name().cmp(b.name()));
        for device in others {
            if seen.insert(device.id()) {
                candidates.push(BatchCandidate { device, partner: false });
            }
        }
        candidates
    }
    /// How the device would be set up over mac-telnet, see [`MutationAuthenticated::bootstrap`].
    #[graphql(name = "bootstrapPlan")]
    async fn api_bootstrap_plan(&self) -> Option<BootstrapPlan> {
        bootstrap::plan(self).ok()
    }
    /// Why there is no `bootstrapPlan`.
    async fn bootstrap_problem(&self) -> Option<ErrorInfo> {
        bootstrap::plan(self).err().map(|error| ErrorInfo::from(&error))
    }
    /// The provisioner's entries in the device's journal in NetBox, newest first: what its
    /// jobs did (`crate::journal`, the device page's „Verlauf“).
    async fn journal(&self) -> Result<Vec<crate::journal::JournalEntry>, crate::error_info::ApiError> {
        Ok(crate::journal::entries(self.id).await.map_err(Error::Netbox)?)
    }
    /// The RouterOS device feeding this one over PoE and its port: a cold boot
    /// (`Mutation.powerCycle`) is possible.
    async fn power_supply(&self) -> Option<crate::device::power_cycle::PowerSupply> {
        crate::device::power_cycle::supply(self).map(|(_, supply, _)| supply)
    }
    /// The neighbors this device stays reachable through when an update leaves it without
    /// OSPF (`device::update_fallback::neighbors`): its way back where it has no two
    /// partitions; without either, an update needs `updateRouteros(withoutFallback:)`.
    async fn update_neighbors(&self) -> Vec<Box<str>> {
        crate::device::update_fallback::neighbors(self).into()
    }
    /// The watchdogs that may fire while this device updates (`device::watchdog::watchers`):
    /// of the devices powering it, which cut its power mid-install, and of the ones reaching
    /// the network through it (`updateRouteros(suspendWatchdogs:)`). Only those watching
    /// something or that can't be read.
    async fn watchdogs(&self) -> Vec<crate::device::watchdog::Watchdog> {
        use crate::device::watchdog;
        let powered = watchdog::addresses(self);
        let watchers = watchdog::watchers(self, &crate::graphql::management_tree(self));
        let read = watchers.iter().map(|watchdog::Watcher { device, supplier }| {
            let powered = if *supplier { powered.as_slice() } else { &[] };
            async move {
                let read = async {
                    let link = device
                        .connect(None, None, None)
                        .await?
                        .ok_or(Error::MissingCredentials)?;
                    watchdog::read(&link.connect_again().await?, powered).await
                };
                let (watching, problem) = match read.await {
                    Ok(watching) => (watching, None),
                    Err(error) => (Default::default(), Some(ErrorInfo::from(&error))),
                };
                watchdog::Watchdog {
                    device: device.name().into(),
                    supplier: *supplier,
                    watching,
                    problem,
                }
            }
        });
        async_graphql::futures_util::future::join_all(read)
            .await
            .into_iter()
            .filter(|watchdog| !watchdog.watching.is_empty() || watchdog.problem.is_some())
            .collect()
    }
    /// The findings about it, the worst first.
    async fn findings(&self) -> Vec<Finding> {
        self.topology
            .findings()
            .iter()
            .filter(|finding| finding.devices.contains(&self.id))
            .cloned()
            .collect()
    }
    /// Its interfaces by task, with the findings about each (NetBox only).
    #[graphql(name = "interfaceView")]
    async fn api_interface_view(&self) -> crate::topology::interfaces::InterfaceView {
        self.interface_view()
    }
    #[graphql(name = "wlanControllerOf")]
    async fn api_is_wlan_controller_of(&self) -> Option<WlanGroupAccess> {
        self.wlan_controller_of()
    }
    #[graphql(name = "wlanApOf")]
    async fn api_is_wlan_ap_of(&self) -> Option<WlanGroupAccess> {
        self.wlan_ap_of()
    }
    /// Its Ethernet ports, for choosing the ends of a link (helper „Verbindung“).
    async fn link_ports(&self) -> Vec<crate::helper::connection::LinkPort> {
        crate::helper::connection::link_ports(self)
    }
    /// What the helper „Kunden-Netz“ can choose from.
    async fn customer_net_options(&self) -> crate::helper::customer_net::CustomerNetOptions {
        crate::helper::customer_net::options(self)
    }
    /// What „VLAN am Port“ can choose from: the ports that can take a customer's VLAN and the
    /// VLANs they can have.
    async fn port_vlan_options(&self) -> crate::helper::port_vlan::PortVlanOptions {
        crate::helper::port_vlan::options(self)
    }
    /// What the helper „PoE“ can choose from.
    async fn poe_options(&self) -> crate::helper::poe::PoeOptions {
        crate::helper::poe::options(self)
    }
    /// The radio bridge devices of the network, for the helper „Funkbrücke“.
    async fn radio_bridge_candidates(&self) -> Vec<crate::helper::radio_bridge::RadioBridgeCandidate> {
        crate::helper::radio_bridge::candidates(self.topology())
    }
    /// Its interfaces whose tags count, for the helper „Flags“.
    async fn flag_interfaces(&self) -> Vec<crate::helper::flags::FlagInterface> {
        crate::helper::flags::flag_interfaces(self)
    }
}
