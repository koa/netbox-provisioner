//! Scope: The whole config of a device as it is and as NetBox has it, and the script between them;
//! not a single topic of it (its module).
//!
//! The config of a device as it is and as NetBox has it (`DeviceCfg`), and the script
//! between them (`GeneratedCfg`).
use crate::device::ros::l2::{EndpointNameGenerator, KeptNames, L2Setup, NameGenerator};
use crate::device::ros::{
    CapsManDeviceDataCurrent, CapsManDeviceDataTarget, WifiDeviceDataCurrent, WifiDeviceDataTarget,
};
use crate::{
    Error,
    config::CONFIG,
    device::{
        raw,
        ros::{
            BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, W60GDeviceDataCurrent,
            W60GDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
            graphql::GraphqlSystemRouterboard,
            hw_facts::{RadioPackage, SwitchChips, build_w60g_ports, radio_package},
            needed_radio_package,
            warning::{ApplyWarning, CapOffOutsideWlanGroup},
        },
    },
    topology::{PhysicalPortId, access::device::DeviceAccess},
};
use async_graphql::SimpleObject;
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::{
    MikrotikDevice,
    ascii::AsciiString,
    generator::Generator,
    model::{
        CertificateCfg, InterfaceWifiCapCfg, InterfaceWifiCapsmanCfg, InterfaceWifiCapsmanState,
        InterfaceWifiState, InterfaceWirelessCapCfg, ReferenceType, SystemPackageState,
        SystemRouterboardState, YesNo,
    },
    resource::{ResourceMutation, ResourceMutationOperation, SingleResource, collect_resource},
};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};

#[derive(Clone, Debug)]
pub struct DeviceCfg {
    pub(super) base_current: BaseDeviceDataCurrent,
    pub(super) base_target: BaseDeviceDataTarget,
    wireless_current: Option<WirelessDeviceDataCurrent>,
    wireless_target: Option<WirelessDeviceDataTarget>,
    wifi_current: Option<WifiDeviceDataCurrent>,
    wifi_target: Option<WifiDeviceDataTarget>,
    /// the certificates CAPsMAN generated for itself (`/interface/wifi/capsman`)
    capsman_certificates: Box<[AsciiString]>,
    /// the package of the radios, if NetBox uses them (`needed_radio_package`)
    radio_package: Option<RadioPackage>,
    /// the board's switch chips, for what they offload (`L2Setup::for_switch_chips`)
    switch_chips: SwitchChips,
    w60g_current: Option<W60GDeviceDataCurrent>,
    w60g_target: Option<W60GDeviceDataTarget>,
    /// the legacy CAPsMAN, on a controller with CAPs of it
    capsman_current: Option<CapsManDeviceDataCurrent>,
    capsman_target: Option<CapsManDeviceDataTarget>,
    /// the radios (default names) CAPsMAN has bound, which RouterOS flags dynamic and
    /// refuses to change
    bound_radios: Box<[AsciiString]>,
    /// what `generate_from` couldn't bring in line (a bound radio)
    wifi_warnings: Vec<ApplyWarning>,
    /// the board's radios NetBox leaves alone, with the CAP off (`CapOffOutsideWlanGroup`)
    idle_radios: Option<RadioPackage>,
    /// what `generate_live` found at the device: the addresses a watchdog would watch that
    /// don't answer, the API clients the restriction would lock out
    live_warnings: Vec<ApplyWarning>,
}

impl DeviceCfg {
    pub fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        name_generator: N,
    ) -> Result<(), SetupError> {
        self.base_target.set_name_and_loopback(device);

        // a legacy radio CAPsMAN manages keeps its name and settings, as the CAP has them
        let kept = self
            .wireless_current
            .as_ref()
            .map(WirelessDeviceDataCurrent::managed_radios)
            .unwrap_or_default();
        let managed_in_netbox = device
            .interfaces()
            .iter()
            .filter_map(|interface| {
                let default_name = AsciiString::from(interface.external_port()?.to_string());
                kept.contains_key(&default_name).then_some(default_name)
            })
            .collect::<HashSet<_>>();
        let l2 = L2Setup::new(
            device,
            KeptNames {
                inner: name_generator,
                kept,
            },
        )?
        .for_switch_chips(self.switch_chips);
        let mut warnings = Vec::new();

        self.base_target.setup_addressing(device, &l2)?;
        if let Some(wireless_target) = self.wireless_target.as_mut() {
            wireless_target.generate_from(device, &l2, self.wireless_current.as_ref())?;
            if let Some(current) = &self.wireless_current {
                wireless_target.keep_managed_radios(current, &managed_in_netbox);
            }
        }
        if let Some(w60g_target) = self.w60g_target.as_mut() {
            w60g_target.generate_from(device, &l2, self.w60g_current.as_ref())?;
        }
        if let Some(wifi_target) = self.wifi_target.as_mut() {
            wifi_target.generate_from(
                device,
                &l2,
                self.wifi_current.as_ref(),
                &self.capsman_certificates,
                self.radio_package,
                &self.bound_radios,
            )?;
        }
        if let Some(capsman_target) = self.capsman_target.as_mut() {
            capsman_target.generate_from(device)?;
        }
        self.base_target
            .setup_ports(device, &l2, self.radio_package)?;
        // radios are interfaces the provisioner manages as well
        let radios = self
            .wireless_current
            .iter()
            .flat_map(|current| current.wireless.iter().map(|radio| radio.data.name.clone()))
            .chain(
                self.wifi_current
                    .iter()
                    .flat_map(|current| current.wifi.iter().map(|radio| radio.data.name.clone())),
            )
            .chain(
                self.w60g_current
                    .iter()
                    .flat_map(|current| current.radio.iter().map(|radio| radio.0.name.clone())),
            )
            .collect::<Vec<_>>();
        self.base_target
            .keep_hand_made_addresses(&self.base_current, &radios);
        let renamed =
            following_renames(&self.base_target, &self.base_current, &self.radio_renames());
        self.base_target.keep_dhcp_v6_clients(&renamed);
        if let Some(package) = self.idle_radios {
            warnings.push(ApplyWarning::CapOffOutsideWlanGroup(
                CapOffOutsideWlanGroup {
                    package: package.name().into(),
                },
            ));
        }
        self.wifi_warnings = warnings;
        Ok(())
    }
    /// [`Self::generate_from`] with the provisioner's own needs on top: the device the
    /// provisioner reaches the network through gets its SOCKS proxy, and the watchdogs find
    /// their way to it too.
    pub fn generate_for(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        self.generate_from(device, EndpointNameGenerator)?;
        self.base_target.setup_watchdog(
            device,
            &crate::topology::management::ManagementTree::of_device(device),
        )?;
        if let Some(proxy) = CONFIG
            .management_proxy
            .as_ref()
            .filter(|p| *p.device == *device.name())
        {
            self.base_target.setup_management_proxy(
                &proxy.user,
                &proxy.password,
                CONFIG.api_networks.as_deref().unwrap_or_default(),
            );
        }
        if let Some(syslog) = &CONFIG.device_syslog {
            self.base_target.setup_syslog(syslog);
        }
        self.setup_services(CONFIG.api_networks.as_deref());
        self.allow_provisioner(CONFIG.api_networks.as_deref().unwrap_or_default());
        Ok(())
    }
    /// `/ip/service` from the device's, the API and www only from `api_networks`
    /// (`BaseDeviceDataTarget::setup_services`).
    pub fn setup_services(&mut self, api_networks: Option<&[ipnet::IpNet]>) {
        self.base_target
            .setup_services(&self.base_current.ip_service, api_networks);
    }
    /// Lets the provisioner in from `networks` (`api_networks`) through the perimeter
    /// firewall (`BaseDeviceDataTarget::allow_provisioner`).
    pub fn allow_provisioner(&mut self, networks: &[ipnet::IpNet]) {
        self.base_target.allow_provisioner(networks);
    }
    pub fn generate_mutations(&self) -> Result<Box<[ResourceMutation<'_>]>, Error> {
        // owned: the device as it is after the renames lives only here
        let mut current =
            following_renames(&self.base_target, &self.base_current, &self.radio_renames());
        super::syslog::after_switch_to_cef(&mut current);
        let mut mutations = self
            .base_target
            .generate_mutations(&current)?
            .into_iter()
            .map(ResourceMutation::into_owned)
            .collect::<Vec<_>>();
        // `/interface/wifi` before `/interface/wireless`: radios a removed `wifi-qcom-ac` left
        // give their names back to the `wlan<N>` taking them (the sort keeps this order)
        if let (Some(target), Some(current)) = (&self.wifi_target, &self.wifi_current) {
            mutations.extend(target.generate_mutations(current)?);
        }
        if let (Some(target), Some(current)) = (&self.wireless_target, &self.wireless_current) {
            mutations.extend(target.generate_mutations(current)?);
        }
        if let (Some(target), Some(current)) = (&self.w60g_target, &self.w60g_current) {
            mutations.extend(target.generate_mutations(current)?);
        }
        if let (Some(target), Some(current)) = (&self.capsman_target, &self.capsman_current) {
            mutations.extend(target.generate_mutations(current)?);
        }
        Ok(mutations.into())
    }
    /// The radios the plan renames, old name to new (`following_renames`).
    fn radio_renames(&self) -> Vec<(AsciiString, AsciiString)> {
        let wifi = self
            .wifi_target
            .as_ref()
            .zip(self.wifi_current.as_ref())
            .into_iter()
            .flat_map(|(target, current)| {
                current.wifi.iter().filter_map(|radio| {
                    let new = &target.wifi.get(&radio.default_name)?.name;
                    (new != &radio.data.name).then(|| (radio.data.name.clone(), new.clone()))
                })
            });
        let wireless = self
            .wireless_target
            .as_ref()
            .zip(self.wireless_current.as_ref())
            .into_iter()
            .flat_map(|(target, current)| {
                current.wireless.iter().filter_map(|radio| {
                    let new = &target.wireless.get(&radio.default_name.clone())?.name;
                    (new != &radio.data.name).then(|| (radio.data.name.clone(), new.clone()))
                })
            });
        wifi.chain(wireless).collect()
    }
    /// [`Self::generate_for`] with the watchdogs only on addresses that answer from the device,
    /// asked over `raw` (`keep_silent_watchdogs`): the plan and every check after the apply
    /// see the same, since a silent address keeps what the device has. Likewise the API is
    /// restricted only where its open connections stay inside `api_networks`
    /// (`keep_api_open_for`).
    pub async fn generate_live(
        &mut self,
        device: &DeviceAccess,
        raw: &RawDevice<SimpleResult>,
    ) -> Result<(), Error> {
        self.generate_for(device)?;
        let addresses = self.base_target.watched_addresses();
        let answering = raw::answering(raw, &addresses).await;
        let topology = device.topology().clone();
        self.live_warnings =
            self.base_target
                .keep_silent_watchdogs(&self.base_current, &answering, |address| {
                    topology
                        .list_devices()
                        .find(|device| {
                            crate::device::watchdog::addresses(device).contains(&address)
                        })
                        .map(|device| device.name().into())
                        .unwrap_or_default()
                });
        if let Some(networks) = CONFIG
            .api_networks
            .as_deref()
            .filter(|networks| !networks.is_empty())
        {
            let clients = raw::rows(raw, "/ip/service/print")
                .await?
                .iter()
                .filter(|row| raw::field(row, "name") == Some("api"))
                .filter_map(|row| raw::field(row, "remote").and_then(remote_address))
                .collect::<Vec<_>>();
            self.live_warnings
                .extend(self.base_target.keep_api_open_for(
                    &self.base_current.ip_service,
                    &clients,
                    networks,
                ));
        }
        Ok(())
    }
    /// The diff from [`Self::generate_mutations`] in an order where everything a command
    /// refers to exists before it.
    pub fn sorted_mutations(&self) -> Result<Box<[ResourceMutation<'_>]>, Error> {
        let mutations = self.generate_mutations()?;
        for m in &mutations {
            info!("Mutation generated: {:?}", m);
        }
        let sorted = ResourceMutation::sort_mutations_with_provided_dependencies(
            mutations.as_ref(),
            provided_by_routeros(),
        )?;
        Ok(sorted.iter().map(|m| (*m).clone()).collect())
    }
    /// The diff as a RouterOS script.
    pub fn script(&self) -> Result<Box<str>, Error> {
        render_script(&self.sorted_mutations()?)
    }
    /// What the operator should know before applying (`BaseDeviceDataTarget::poe_warnings`).
    /// Whether a port losing PoE out powers something is asked at the device, over `raw`,
    /// only where a port loses it.
    pub async fn warnings(
        &self,
        raw: impl AsyncFnOnce() -> Result<RawDevice<SimpleResult>, Error>,
    ) -> Vec<ApplyWarning> {
        let ports = self.base_target.poe_switched_off(&self.base_current);
        if ports.is_empty() {
            return [self.wifi_warnings.clone(), self.live_warnings.clone()].concat();
        }
        let names = ports
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let powered = match raw().await {
            Ok(raw) => powered_ports(&raw, &names).await,
            Err(error) => Err(error),
        };
        let powered = powered.unwrap_or_else(|error| {
            // better a warning too many than power cut unasked
            info!("cannot read the PoE status: {error}");
            names.into_iter().collect()
        });
        let mut warnings = self.wifi_warnings.clone();
        warnings.extend(self.base_target.poe_warnings(&self.base_current, &powered));
        warnings.extend(self.live_warnings.iter().cloned());
        warnings
    }
}

/// What RouterOS has without anyone adding it, which mutations may refer to. Not the
/// `capsman` a bound radio reports without such an entry (cAP ax, 7.23.7): the target always
/// has that configuration, and taking it as there let router-stall-burri's radios get
/// `configuration=capsman` before it was added, which RouterOS refused.
pub(crate) fn provided_by_routeros() -> [(ReferenceType, std::borrow::Cow<'static, [u8]>); 6] {
    [
        (ReferenceType::Interface, b"lo".into()),
        // `/ipv6/nd`'s own entry: every interface
        (ReferenceType::Interface, b"all".into()),
        (ReferenceType::RoutingTable, b"main".into()),
        (ReferenceType::FirewallChain, b"input".into()),
        (ReferenceType::FirewallChain, b"output".into()),
        (ReferenceType::FirewallChain, b"forward".into()),
    ]
}

/// Whether the script has `mutation`: an update without fields it leaves out (the diff has one
/// for every entry, most of them without a change), a removal or a move it has without.
pub(crate) fn in_script(mutation: &ResourceMutation) -> bool {
    matches!(
        mutation.operation,
        ResourceMutationOperation::RemoveByKey(_) | ResourceMutationOperation::Move { .. }
    ) || !mutation.fields.is_empty()
}

pub(super) fn render_script(mutations: &[ResourceMutation]) -> Result<Box<str>, Error> {
    let mut cfg = String::new();
    let mut generator = Generator::new(&mut cfg);
    for mutation in mutations {
        generator.append_mutation(mutation)?;
    }
    Ok(cfg.into_boxed_str())
}

/// The ports of `names` whose PoE out powers something (`poe-out-status=powered-on`), not
/// only waits for a load.
async fn powered_ports(
    raw: &RawDevice<SimpleResult>,
    names: &[AsciiString],
) -> Result<HashSet<AsciiString>, Error> {
    let numbers = names
        .iter()
        .map(|name| name.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut powered = HashSet::new();
    let rows = raw::command(
        raw,
        "/interface/ethernet/poe/monitor",
        &[("numbers", &numbers), ("once", "")],
    )
    .await?;
    for row in rows {
        if let (Some(name), Some("powered-on")) =
            (raw::field(&row, "name"), raw::field(&row, "poe-out-status"))
        {
            powered.insert(AsciiString::from(name));
        }
    }
    Ok(powered)
}

/// The script that would bring a device in line with NetBox.
#[derive(SimpleObject)]
#[graphql(complex)]
pub struct GeneratedCfg {
    /// empty if the device is in line already
    pub(super) script: Box<str>,
    /// what the script changes, by menu (`site_diff`)
    pub(super) menus: Vec<crate::device::site_diff::MenuChanges>,
    /// what applying it would do that the operator should know before
    pub(super) warnings: Vec<ApplyWarning>,
    /// when the device was read for it
    #[graphql(skip)]
    pub(super) as_of: tokio::time::Instant,
}

impl GeneratedCfg {
    /// A device found in line, read at `as_of`: what an apply's check saw (no warnings, they
    /// are about changes).
    pub(crate) fn in_line(as_of: tokio::time::Instant) -> GeneratedCfg {
        GeneratedCfg {
            script: "".into(),
            menus: Vec::new(),
            warnings: Vec::new(),
            as_of,
        }
    }
}

#[async_graphql::ComplexObject]
impl GeneratedCfg {
    /// seconds since the device was read for it (`facts`)
    async fn age(&self) -> u64 {
        self.as_of.elapsed().as_secs()
    }
}

impl GeneratedCfg {
    pub fn menu_changes(&self) -> &[crate::device::site_diff::MenuChanges] {
        &self.menus
    }
}

impl DeviceCfg {
    /// Reads a device's current config over `client`, any connection to it (the CHR harness
    /// reaches its nodes over their own forwarded ports).
    pub async fn fetch(
        client: &MikrotikDevice,
        device: &DeviceAccess,
    ) -> Result<DeviceCfg, SetupError> {
        let routerboard = <SystemRouterboardState as SingleResource>::fetch(client)
            .await?
            .ok_or(SetupError::RouterboardNotDefined)?;
        Self::fetch_with_board_model(client, &routerboard.model.0, device).await
    }

    /// [`Self::fetch`] for the board `model`, which a CHR (no `/system/routerboard`) can't
    /// report itself.
    pub async fn fetch_with_board_model(
        client: &MikrotikDevice,
        model: &[u8],
        device: &DeviceAccess,
    ) -> Result<DeviceCfg, SetupError> {
        let installed_packages = collect_resource::<SystemPackageState>(client)
            .await?
            .into_iter()
            .filter(|p| !p.disabled)
            .map(|p| p.name.0)
            .collect::<HashSet<_>>();
        // a radio stack is read only where NetBox needs it, and then its package must be there
        let needed = needed_radio_package(device, model);
        if let Some(package) = needed
            && !installed_packages.contains(package.name().as_bytes())
        {
            return Err(SetupError::MissingPackage(package.name()));
        }

        let current = BaseDeviceDataCurrent::fetch(client).await?;
        let target = BaseDeviceDataTarget::new(model)?;
        let (wireless_current, wireless_target) =
            if needed == Some(RadioPackage::Wireless) && build_w60g_ports(model).is_empty() {
                (
                    Some(WirelessDeviceDataCurrent::fetch(client).await?),
                    Some(WirelessDeviceDataTarget::detect_device(client).await?),
                )
            } else {
                (None, None)
            };
        // a controller has no radios: CAP and CAPsMAN of `/interface/wifi` are in the base
        // package since RouterOS 7.13 (they fail before, then there is nothing to clean up)
        let capsman_certificates: Box<[AsciiString]> = InterfaceWifiCapsmanState::fetch(client)
            .await
            .ok()
            .flatten()
            .map(|state| {
                [state.generated_ca_certificate, state.generated_certificate]
                    .into_iter()
                    .flatten()
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        // where CAP or CAPsMAN are still on without NetBox asking for them, or CAPsMAN's
        // certificates are left (a former controller or CAP), they are read to be cleaned up
        let wifi_left_on = || async {
            let capsman = InterfaceWifiCapsmanCfg::fetch(client).await.ok().flatten();
            let cap = InterfaceWifiCapCfg::fetch(client).await.ok().flatten();
            let certificates = collect_resource::<CertificateCfg>(client)
                .await
                .unwrap_or_default();
            capsman.is_some_and(|capsman| capsman.enabled == YesNo::Yes)
                || cap.is_some_and(|cap| cap.enabled == YesNo::Yes)
                || certificates
                    .iter()
                    .any(|certificate| capsman_certificates.contains(&certificate.name))
        };
        let (wifi_current, wifi_target, bound_radios) = if needed.is_some_and(RadioPackage::is_wifi)
            || device.wlan_controller_of().is_some()
            || wifi_left_on().await
        {
            let bound_radios = collect_resource::<InterfaceWifiState>(client)
                .await?
                .into_iter()
                // dynamic, or only bound with the same board and version (hAP ax², 7.23.8,
                // router-stall-burri): RouterOS takes a change then but doesn't use it
                .filter(|radio| radio.dynamic == Some(true) || radio.bound)
                .filter_map(|radio| radio.default_name)
                .collect();
            (
                Some(WifiDeviceDataCurrent::fetch(client).await?),
                Some(WifiDeviceDataTarget::detect_device(client).await?),
                bound_radios,
            )
        } else {
            (None, None, Box::default())
        };
        // the 60 GHz radios have their own menu (in `wireless` too); such a board has no other
        // radio, a `wlan<N>` or `wifi<N>` in NetBox is its `wlan60-<N>` misnamed
        if !build_w60g_ports(model).is_empty()
            && let Some(port) = device.interfaces().iter().find_map(|interface| {
                interface.external_port().filter(|port| {
                    matches!(port, PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_))
                })
            })
        {
            return Err(SetupError::PortNotFound(port));
        }
        let (w60g_current, w60g_target) =
            if needed == Some(RadioPackage::Wireless) && !build_w60g_ports(model).is_empty() {
                (
                    Some(W60GDeviceDataCurrent::fetch(client).await?),
                    Some(W60GDeviceDataTarget::detect_device(client).await?),
                )
            } else {
                (None, None)
            };
        // the legacy CAPsMAN only where NetBox has CAPs of it: a controller may run one that
        // NetBox doesn't know of (CAPs without their radios in NetBox), which stays as it is
        let (capsman_current, capsman_target) = if device
            .wlan_controller_of()
            .is_some_and(|group| group.has_legacy_caps())
        {
            if !installed_packages.contains(RadioPackage::Wireless.name().as_bytes()) {
                return Err(SetupError::MissingPackage(RadioPackage::Wireless.name()));
            }
            (
                Some(CapsManDeviceDataCurrent::fetch(client).await?),
                Some(CapsManDeviceDataTarget::detect_device(client).await?),
            )
        } else {
            (None, None)
        };
        // A device in no Wireless LAN Group serves no WLAN where NetBox uses none of its radios:
        // the target switches a CAP of `/interface/wifi` off, one of `/interface/wireless`
        // stays as it is, off after the initial setup. On an AP missing from its group in
        // NetBox the operator should see that. The installed package tells which one the
        // radios run (a wAP ac runs either).
        let idle_radios = match radio_package(model) {
            Some(package)
                if needed.is_none()
                    && device.wlan_ap_of().is_none()
                    && device.wlan_controller_of().is_none() =>
            {
                let wireless = RadioPackage::Wireless;
                if installed_packages.contains(wireless.name().as_bytes()) {
                    let cap_on = InterfaceWirelessCapCfg::fetch(client)
                        .await
                        .ok()
                        .flatten()
                        .is_some_and(|cap| cap.enabled);
                    (!cap_on).then_some(wireless)
                } else {
                    Some(package)
                }
            }
            _ => None,
        };
        Ok(DeviceCfg {
            base_current: current,
            base_target: target,
            wireless_current,
            wireless_target,
            wifi_current,
            wifi_target,
            capsman_certificates,
            radio_package: needed,
            switch_chips: SwitchChips::of(model),
            w60g_current,
            w60g_target,
            capsman_current,
            capsman_target,
            bound_radios,
            wifi_warnings: Vec::new(),
            idle_radios,
            live_warnings: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct DeviceStats {
    routerboard: GraphqlSystemRouterboard,
}
impl DeviceStats {
    pub async fn fetch(client: &MikrotikDevice) -> Result<DeviceStats, Error> {
        Ok(Self {
            routerboard: GraphqlSystemRouterboard(
                SystemRouterboardState::fetch(client)
                    .await?
                    // a CHR has none
                    .ok_or(SetupError::RouterboardNotDefined)?,
            ),
        })
    }
}

/// without brackets (`10.0.2.2:40374`, CHR 7.24.5; `2a02:610:7501:1000::201:0`, lab-wap 7.23.8),
/// so the part before the last colon, whatever the whole would parse as.
pub(super) fn remote_address(remote: &str) -> Option<IpAddr> {
    let (address, _port) = remote.rsplit_once(':')?;
    address
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

/// `current` as it is once the interfaces `target` renames have their new names: what refers
/// to them follows a rename on RouterOS, so a bridge port, a list member or a bridge VLAN on a
/// renamed interface stays as it is instead of looking like an entry on another interface.
/// Paired by the names from before, every port of a renamed set looked like another, and one
/// was set onto an interface another port still held ("device already added as bridge port").
/// `more` are renames besides the ethernet ports' (the radios'), old name to new.
pub(super) fn following_renames(
    target: &BaseDeviceDataTarget,
    current: &BaseDeviceDataCurrent,
    more: &[(AsciiString, AsciiString)],
) -> BaseDeviceDataCurrent {
    let renames: HashMap<&AsciiString, &AsciiString> = current
        .ethernet
        .iter()
        .filter_map(|port| {
            let new = &target.ethernet.get(&port.default_name)?.name;
            (new != &port.data.name).then_some((&port.data.name, new))
        })
        .chain(more.iter().map(|(old, new)| (old, new)))
        .collect();
    let mut current = current.clone();
    if renames.is_empty() {
        return current;
    }
    let follow = |name: &mut AsciiString| {
        if let Some(new) = renames.get(name) {
            *name = (*new).clone();
        }
    };
    for port in current.bridge_port.iter_mut() {
        follow(&mut port.data.interface);
    }
    for member in current.interface_list_member.iter_mut() {
        follow(&mut member.data.interface);
    }
    for client in current.dhcp_v_6_client.iter_mut() {
        follow(&mut client.0.interface);
    }
    for vlan in current.bridge_vlan.iter_mut() {
        vlan.data.tagged = vlan
            .data
            .tagged
            .iter()
            .cloned()
            .map(|mut n| {
                follow(&mut n);
                n
            })
            .collect();
        vlan.data.untagged = vlan
            .data
            .untagged
            .iter()
            .cloned()
            .map(|mut n| {
                follow(&mut n);
                n
            })
            .collect();
    }
    current
}
