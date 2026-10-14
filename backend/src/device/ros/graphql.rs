use crate::device::ros::l2::{EndpointNameGenerator, L2Setup, NameGenerator};
use crate::device::ros::{
    CapsManDeviceDataCurrent, CapsManDeviceDataTarget, WifiDeviceDataCurrent,
    WifiDeviceDataTarget,
};
use crate::{
    Error,
    config::CONFIG,
    device::{
        AccessibleDevice, GraphqlSystemRouterboard, PingResult,
        ros::{
            fallback,
            BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, W60GDeviceDataCurrent,
            W60GDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
            hw_facts::{RadioPackage, build_w60g_ports},
            needed_radio_package,
        },
        raw, update,
    },
    topology::access::device::DeviceAccess,
};
use crate::{graphql::scalars::ScalarDuration, job::Log};
use async_graphql::{Context, Object, SimpleObject};
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::{
    MikrotikDevice,
    ascii::AsciiString,
    generator::Generator,
    hwconfig::DeviceType,
    model::{
        CapsManManagerCfg, CertificateCfg, InterfaceWifiCapCfg, InterfaceWifiCapsmanCfg, InterfaceWifiState,
        InterfaceWifiCapsmanState,
        ReferenceType, SystemIdentityCfg, SystemPackageState, SystemRouterboardState, YesNo,
    },
    resource::{ResourceMutation, SingleResource, apply_mutations_via_script, collect_resource},
};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use surge_ping::SurgeError;

pub struct GraphqlDeviceType(DeviceType);
impl From<DeviceType> for GraphqlDeviceType {
    fn from(value: DeviceType) -> Self {
        GraphqlDeviceType(value)
    }
}
#[Object]
impl GraphqlDeviceType {
    async fn name(&self) -> &str {
        self.0.device_type_name()
    }
}

struct GraphqlSystemIdentity<'a>(&'a SystemIdentityCfg);
#[Object]
impl GraphqlSystemIdentity<'_> {
    async fn name(&self) -> String {
        self.0.name.to_string()
    }
}
#[derive(Clone, Debug)]
pub struct DeviceCfg {
    base_current: BaseDeviceDataCurrent,
    base_target: BaseDeviceDataTarget,
    wireless_current: Option<WirelessDeviceDataCurrent>,
    wireless_target: Option<WirelessDeviceDataTarget>,
    wifi_current: Option<WifiDeviceDataCurrent>,
    wifi_target: Option<WifiDeviceDataTarget>,
    /// the certificates CAPsMAN generated for itself (`/interface/wifi/capsman`)
    capsman_certificates: Box<[AsciiString]>,
    /// the package of the radios, if NetBox uses them (`needed_radio_package`)
    radio_package: Option<RadioPackage>,
    w60g_current: Option<W60GDeviceDataCurrent>,
    w60g_target: Option<W60GDeviceDataTarget>,
    /// the legacy CAPsMAN, on a controller with CAPs of it
    capsman_current: Option<CapsManDeviceDataCurrent>,
    capsman_target: Option<CapsManDeviceDataTarget>,
    /// the radios (default names) CAPsMAN has bound, which RouterOS flags dynamic and
    /// refuses to change
    bound_radios: Box<[AsciiString]>,
    /// what `generate_from` couldn't bring in line (a bound radio)
    wifi_warnings: Vec<String>,
}

impl DeviceCfg {
    pub fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        name_generator: N,
    ) -> Result<(), SetupError> {
        self.base_target.set_name_and_loopback(device);

        let l2 = L2Setup::new(device, name_generator)?;

        self.base_target.setup_ip_addresses(&l2)?;
        self.base_target.setup_dns(device);
        self.base_target.setup_ospf(device, &l2);
        if let Some(wireless_target) = self.wireless_target.as_mut() {
            wireless_target.generate_from(device, &l2, self.wireless_current.as_ref())?;
        }
        if let Some(w60g_target) = self.w60g_target.as_mut() {
            w60g_target.generate_from(device, &l2, self.w60g_current.as_ref())?;
        }
        if let Some(wifi_target) = self.wifi_target.as_mut() {
            self.wifi_warnings = wifi_target.generate_from(
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
        self.base_target.setup_l2(device, &l2, self.radio_package)?;
        self.base_target.setup_mtu(device)?;
        self.base_target.setup_poe(device)?;
        Ok(())
    }
    /// [`Self::generate_from`] with the provisioner's own needs on top: the device the
    /// provisioner reaches the network through gets its SOCKS proxy.
    pub fn generate_for(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        self.generate_from(device, EndpointNameGenerator)?;
        if let Some(proxy) = CONFIG
            .management_proxy
            .as_ref()
            .filter(|p| *p.device == *device.name())
        {
            self.base_target
                .setup_management_proxy(&proxy.user, &proxy.password);
        }
        Ok(())
    }
    pub fn generate_mutations(&self) -> Result<Box<[ResourceMutation]>, Error> {
        let mut mutations = self.base_target.generate_mutations(&self.base_current)?.into_vec();
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
    ) -> Vec<String> {
        let ports = self.base_target.poe_switched_off(&self.base_current);
        if ports.is_empty() {
            return self.wifi_warnings.clone();
        }
        let names = ports.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>();
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
        warnings
    }
}

/// What RouterOS has without anyone adding it, which mutations may refer to: a radio that
/// CAPsMAN manages reports `configuration=capsman`, the configuration CAPsMAN gave it
/// (`/interface/wifi`, 7.23.7), which is no `/interface/wifi/configuration` entry.
pub(super) fn provided_by_routeros() -> [(ReferenceType, std::borrow::Cow<'static, [u8]>); 6] {
    [
        (ReferenceType::Interface, b"lo".into()),
        (ReferenceType::RoutingTable, b"main".into()),
        (ReferenceType::FirewallChain, b"input".into()),
        (ReferenceType::FirewallChain, b"output".into()),
        (ReferenceType::FirewallChain, b"forward".into()),
        (ReferenceType::WifiConfiguration, b"capsman".into()),
    ]
}

fn render_script(mutations: &[ResourceMutation]) -> Result<Box<str>, Error> {
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
pub struct GeneratedCfg {
    /// empty if the device is in line already
    script: Box<str>,
    /// what applying it would do that the operator should know before (German, for the UI)
    warnings: Vec<String>,
}

/// The script the apply runs on the device; empty if there is nothing to do.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyPlanned {
    pub script: Box<str>,
}

/// The device saved a backup it loads after `delay`, unless the provisioner reaches it again
/// (`fallback`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FallbackArmed {
    pub delay: ScalarDuration,
}

/// The script runs on the device, or stopped at the line of `error`; the apply waits up to
/// `timeout` for the device to be in line.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyRunning {
    pub timeout: ScalarDuration,
    pub error: Option<Box<str>>,
}

/// What the device still lacks after the apply (empty when everything was applied), or why it
/// couldn't be checked.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyChecked {
    pub remaining: Box<str>,
    pub error: Option<Box<str>>,
}

/// The fallback is taken away: the device is reachable with the new config.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FallbackStopped {
    pub after: ScalarDuration,
}

/// The fallback stays: the device loads the backup from before the apply in about `fires_in`
/// and reboots. Either it couldn't be reached (`error`), or the apply was cancelled.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FallbackLeft {
    pub fires_in: ScalarDuration,
    pub error: Option<Box<str>>,
}

/// A controller provisioned `count` CAP radios of `menu` again, since the WLANs changed (their
/// clients reconnect); or couldn't (`error`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RadiosReprovisioned {
    pub menu: Box<str>,
    pub count: u32,
    pub error: Option<Box<str>>,
}

/// The `radio` menus of the CAPsMANs whose WLANs `mutations` change: a CAP only takes a new
/// WLAN (a slave configuration) when its radios are provisioned again.
pub(super) fn reprovisioned_menus(mutations: &[ResourceMutation]) -> Vec<&'static str> {
    let touches = |menu: &str| {
        mutations.iter().any(|mutation| {
            let path = mutation.resource.strip_prefix(b"/").unwrap_or(mutation.resource);
            ["configuration", "provisioning", "datapath", "security"]
                .iter()
                .any(|entry| path == format!("{menu}/{entry}").as_bytes())
        })
    };
    [
        ("interface/wifi", "/interface/wifi/radio"),
        ("caps-man", "/caps-man/radio"),
    ]
    .into_iter()
    .filter(|(menu, _)| touches(menu))
    .map(|(_, radios)| radios)
    .collect()
}

/// Provisions the CAPs' radios of `menu` again (not the controller's own); how many there are.
async fn provision_radios(
    raw: &RawDevice<SimpleResult>,
    menu: &str,
) -> Result<usize, Error> {
    let rows = raw::rows(raw, &format!("{menu}/print")).await?;
    let ids = rows
        .iter()
        .filter(|row| {
            // `/caps-man/radio` names the CAP in `remote-cap-identity`, `/interface/wifi/radio`
            // in `cap`
            raw::field(row, "local") != Some("true")
                && ["remote-cap-identity", "cap"]
                    .iter()
                    .any(|name| raw::field(row, name).is_some_and(|cap| !cap.is_empty()))
        })
        .filter_map(|row| raw::field(row, ".id"))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(0);
    }
    let numbers = ids.join(",");
    raw::command(raw, &format!("{menu}/provision"), &[("numbers", &numbers)]).await?;
    Ok(ids.len())
}

/// The name of the `/system/script` entry the config is applied with.
const APPLY_SCRIPT: &str = "netbox-provisioner-apply";
/// How long the device may take to run the script; reachable but not in line after that, the
/// script stopped at an error.
const APPLY_TIMEOUT: Duration = Duration::from_secs(90);
/// Before the fallback fires, the provisioner stops trying to reach the device: taking the
/// fallback away just as it loads the backup would leave it half done.
const FALLBACK_MARGIN: Duration = Duration::from_secs(30);

impl AccessibleDevice {
    /// Applies the generated config: the diff runs as a script on the device itself, so it
    /// completes even when it changes the path this connection takes. Then the diff is built
    /// again over a new connection; whatever is left wasn't applied (RouterOS stops a script
    /// at its first error). Before, the device saves a backup it loads unless the provisioner
    /// reaches it again (`fallback`): reachable, even with a part left, it stays as it is.
    /// Cancelled once the script runs, the apply leaves the fallback, which brings back the
    /// config from before. Writes without asking for `allow_apply`, that is up to the caller
    /// (`WritesAllowed`).
    pub async fn apply(&self, log: &Log) -> Result<(), Error> {
        let mut cfg = DeviceCfg::fetch(&self.client, &self.device_config).await?;
        cfg.generate_for(&self.device_config)?;
        let mutations = cfg.sorted_mutations()?;
        let applied = render_script(&mutations)?;
        log.emit(ApplyPlanned {
            script: applied.clone(),
        });
        if applied.is_empty() {
            return Ok(());
        }
        log.check()?;
        let raw = self.connect_again::<SimpleResult>().await?;
        let delay = CONFIG.apply_fallback();
        fallback::arm(&raw, delay).await?;
        let fires = Instant::now() + delay;
        log.emit(FallbackArmed {
            delay: delay.into(),
        });
        // an error is mostly the script's own: it stopped at a line, the ones before are
        // applied, so the fallback stays until the device is reached again, as without
        let error = apply_mutations_via_script(&raw, &mutations, APPLY_SCRIPT)
            .await
            .err();
        drop(raw);
        log.emit(ApplyRunning {
            timeout: APPLY_TIMEOUT.into(),
            error: error.map(|error| error.to_string().into()),
        });
        // cancelled from here on, the fallback stays
        let wait = async |time| {
            let waited = log.unless_cancelled(tokio::time::sleep(time)).await;
            if waited.is_err() {
                log.emit(FallbackLeft {
                    fires_in: fires.saturating_duration_since(Instant::now()).into(),
                    error: None,
                });
            }
            waited
        };

        let settled = Instant::now() + APPLY_TIMEOUT;
        let give_up = fires.checked_sub(FALLBACK_MARGIN).unwrap_or(fires);
        let remaining = loop {
            wait(Duration::from_secs(3)).await?;
            let remaining = self.remaining().await;
            match &remaining {
                Ok(script) if script.is_empty() => break remaining,
                Ok(_) if Instant::now() >= settled => break remaining,
                _ if Instant::now() >= give_up => break remaining,
                _ => info!("{}: not done yet", self.device_config.name()),
            }
        };
        log.emit(match &remaining {
            Ok(script) => ApplyChecked {
                remaining: script.clone(),
                error: None,
            },
            Err(error) => ApplyChecked {
                remaining: "".into(),
                error: Some(error.to_string().into()),
            },
        });
        let reachable = remaining.is_ok();
        // taken away, trying again until `give_up`
        let disarmed = loop {
            if !reachable {
                break Err(None);
            }
            let disarmed = match self.connect_again::<SimpleResult>().await {
                Ok(raw) => fallback::disarm(&raw).await,
                Err(error) => Err(error),
            };
            match disarmed {
                Ok(()) => break Ok(()),
                Err(error) if Instant::now() >= give_up => break Err(Some(error)),
                Err(error) => info!("{}: fallback not stopped yet: {error}", self.device_config.name()),
            }
            wait(Duration::from_secs(3)).await?;
        };
        match disarmed {
            Ok(()) => log.emit(FallbackStopped {
                after: delay
                    .saturating_sub(fires.saturating_duration_since(Instant::now()))
                    .into(),
            }),
            Err(error) => log.emit(FallbackLeft {
                fires_in: fires.saturating_duration_since(Instant::now()).into(),
                error: Some(
                    error
                        .map(|error| error.to_string())
                        // `ApplyChecked` above has the reason
                        .unwrap_or_else(|| "not checked".into())
                        .into(),
                ),
            }),
        }
        if self.device_config.wlan_controller_of().is_some() {
            log.check()?;
            self.reprovision(reprovisioned_menus(&mutations), log).await;
        }
        Ok(())
    }
    /// Provisions the radios of `menus` (`reprovisioned_menus`) again, which reconnects the
    /// CAPs' clients.
    async fn reprovision(&self, menus: Vec<&'static str>, log: &Log) {
        if menus.is_empty() {
            return;
        }
        let failed = |menu: &str, error: Error| RadiosReprovisioned {
            menu: menu.into(),
            count: 0,
            error: Some(error.to_string().into()),
        };
        let raw = match self.connect_again::<SimpleResult>().await {
            Ok(raw) => raw,
            Err(error) => return log.emit(failed(menus[0], error)),
        };
        for menu in menus {
            match provision_radios(&raw, menu).await {
                Ok(0) => {}
                Ok(count) => log.emit(RadiosReprovisioned {
                    menu: menu.into(),
                    count: count as u32,
                    error: None,
                }),
                Err(error) => log.emit(failed(menu, error)),
            }
        }
    }
    /// The packages besides `routeros` the device needs (`needed_radio_package`); none on a
    /// CHR, which has no RouterBOARD.
    pub async fn wanted_packages(&self) -> Result<Box<[Box<str>]>, Error> {
        let model = <SystemRouterboardState as SingleResource>::fetch(&self.client)
            .await
            .ok()
            .flatten()
            .map(|routerboard| routerboard.model.0);
        let needed = model.and_then(|model| needed_radio_package(&self.device_config, &model));
        // a legacy CAPsMAN NetBox doesn't know of (its CAPs' radios aren't in NetBox) stays
        // as it is (`DeviceCfg::fetch`), so does the package it runs in
        let needed = match needed {
            None if self.runs_legacy_capsman().await => Some(RadioPackage::Wireless),
            needed => needed,
        };
        Ok(needed
            .map(|package| package.name().into())
            .into_iter()
            .collect())
    }
    /// Whether `/caps-man/manager` is on (it fails without `wireless`).
    async fn runs_legacy_capsman(&self) -> bool {
        <CapsManManagerCfg as SingleResource>::fetch(&self.client)
            .await
            .ok()
            .flatten()
            .is_some_and(|manager| manager.enabled)
    }
    /// The diff of a fresh look at the device.
    async fn remaining(&self) -> Result<Box<str>, Error> {
        let client = self.connect_again().await?;
        let mut cfg = DeviceCfg::fetch(&client, &self.device_config).await?;
        cfg.generate_for(&self.device_config)?;
        cfg.script()
    }
}

#[Object]
impl DeviceCfg {
    async fn current(&self) -> &BaseDeviceDataCurrent {
        &self.base_current
    }
    async fn target(&self) -> &BaseDeviceDataTarget {
        &self.base_target
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
        let (wireless_current, wireless_target) = if needed == Some(RadioPackage::Wireless)
            && build_w60g_ports(model).is_empty()
        {
            (
                Some(WirelessDeviceDataCurrent::fetch(client).await?),
                Some(WirelessDeviceDataTarget::detect_device(client).await?),
            )
        } else {
            (None, None)
        };
        // a controller has no radios: CAP and CAPsMAN of `/interface/wifi` are in the base
        // package since RouterOS 7.13 (they fail before, then there is nothing to clean up)
        let capsman_certificates: Box<[AsciiString]> =
            InterfaceWifiCapsmanState::fetch(client)
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
            let certificates = collect_resource::<CertificateCfg>(client).await.unwrap_or_default();
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
                .filter(|radio| radio.dynamic == Some(true))
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
        // the 60 GHz radios have their own menu (in `wireless` too)
        let (w60g_current, w60g_target) = if needed == Some(RadioPackage::Wireless)
            && !build_w60g_ports(model).is_empty()
        {
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
        Ok(DeviceCfg {
            base_current: current,
            base_target: target,
            wireless_current,
            wireless_target,
            wifi_current,
            wifi_target,
            capsman_certificates,
            radio_package: needed,
            w60g_current,
            w60g_target,
            capsman_current,
            capsman_target,
            bound_radios,
            wifi_warnings: Vec::new(),
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

#[Object]
impl BaseDeviceDataCurrent {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl BaseDeviceDataTarget {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl AccessibleDevice {
    async fn ping(&self, count: Option<u8>) -> Result<Box<[PingResult]>, SurgeError> {
        self.simple_ping(count.unwrap_or(1)).await
    }

    async fn device_stats(&self) -> Result<DeviceStats, Error> {
        DeviceStats::fetch(&self.client).await
    }

    async fn config(&self) -> Result<DeviceCfg, Error> {
        Ok(DeviceCfg::fetch(&self.client, &self.device_config).await?)
    }
    /// The script that would bring the device in line with NetBox, and what to know before
    /// applying it; `null` with the error in the response's errors when that fails, so a
    /// device the generator fails on still shows the rest (an update may be what it needs).
    async fn generate_cfg(&self, ctx: &Context<'_>) -> Option<GeneratedCfg> {
        let generated = async {
            let mut device_cfg = DeviceCfg::fetch(&self.client, &self.device_config).await?;
            device_cfg.generate_for(&self.device_config)?;
            Ok(GeneratedCfg {
                script: device_cfg.script()?,
                warnings: device_cfg
                    .warnings(async || self.connect_again::<SimpleResult>().await)
                    .await,
            })
        };
        nullable(ctx, generated.await)
    }
    /// The running RouterOS and its packages, and what an update in each channel would do.
    async fn routeros(&self, ctx: &Context<'_>) -> Option<RouterOsVersions> {
        let versions = async {
            let state = update::read_state(&self.connect_again().await?).await?;
            let wanted = self.wanted_packages().await?;
            let newest_v6 = if state.version.major() < 7 {
                Some(update::newest(6, update::Channel::LongTerm).await?)
            } else {
                None
            };
            let mut channels = Vec::new();
            for channel in [
                update::Channel::LongTerm,
                update::Channel::Stable,
                update::Channel::Testing,
                update::Channel::Development,
            ] {
                let newest = update::newest(7, channel).await?;
                let step =
                    update::next_step(&state, &newest, newest_v6.as_ref().unwrap_or(&newest), &wanted);
                channels.push(ChannelUpdate {
                    channel,
                    newest: newest.to_string().into(),
                    upgrade: newest > state.version,
                    update: step.is_some(),
                });
            }
            // RouterOS 6 goes to 7 for its packages (`next_step`)
            let adjust_packages = state.version.major() >= 7
                && update::next_step(&state, &state.version, &state.version, &wanted).is_some();
            Ok::<_, Error>(RouterOsVersions {
                adjust_packages,
                version: state.version.to_string().into(),
                packages: state.packages,
                wanted: ["routeros".into()].into_iter().chain(wanted).collect(),
                default_channel: CONFIG.routeros_channel.unwrap_or_default(),
                channels,
            })
        };
        nullable(ctx, versions.await)
    }
}

/// A field error that leaves the field `null` instead of its parent: async-graphql passes an
/// `Err` of a resolver on to the parent even for a nullable field.
fn nullable<T>(ctx: &Context<'_>, result: Result<T, Error>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            ctx.add_error(
                ctx.set_error_path(async_graphql::Error::from(error).into_server_error(ctx.item.pos)),
            );
            None
        }
    }
}

#[derive(SimpleObject)]
pub struct RouterOsVersions {
    version: Box<str>,
    /// installed packages
    packages: Box<[Box<str>]>,
    /// the packages an update leaves installed
    wanted: Box<[Box<str>]>,
    /// the channel preselected for updates (`routeros_channel` of the config)
    default_channel: update::Channel,
    channels: Vec<ChannelUpdate>,
    /// whether the packages differ from `wanted`, which an update without channel fixes at
    /// the running version
    adjust_packages: bool,
}

#[derive(SimpleObject)]
pub struct ChannelUpdate {
    channel: update::Channel,
    /// the newest RouterOS 7 of the channel
    newest: Box<str>,
    /// whether the device runs an older version
    upgrade: bool,
    /// whether an update changes anything (version or packages)
    update: bool,
}
