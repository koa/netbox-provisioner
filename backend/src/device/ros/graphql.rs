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
            BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, W60GDeviceDataCurrent,
            W60GDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
            hw_facts::{RadioPackage, build_w60g_ports},
            needed_radio_package,
        },
        update,
    },
    topology::access::device::DeviceAccess,
};
use async_graphql::{Context, Object, SimpleObject, futures_util::StreamExt};
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::{
    MikrotikDevice,
    ascii::AsciiString,
    generator::Generator,
    hwconfig::DeviceType,
    model::{
        CapsManManagerCfg, CertificateCfg, InterfaceWifiCapCfg, InterfaceWifiCapsmanCfg,
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
            wifi_target.generate_from(
                device,
                &l2,
                self.wifi_current.as_ref(),
                &self.capsman_certificates,
                self.radio_package,
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
        let mutations = self.base_target.generate_mutations(&self.base_current)?;
        // `/interface/wifi` before `/interface/wireless`: radios a removed `wifi-qcom-ac` left
        // give their names back to the `wlan<N>` taking them (the sort keeps this order)
        let mutations = if let (Some(wifi_target), Some(wifi_current)) =
            (&self.wifi_target, &self.wifi_current)
        {
            let wifi_mutations = wifi_target.generate_mutations(wifi_current)?;
            mutations.into_iter().chain(wifi_mutations).collect()
        } else {
            mutations
        };
        let mutations = if let (Some(wireless_target), Some(wireless_current)) =
            (&self.wireless_target, &self.wireless_current)
        {
            let wireless_mutations = wireless_target.generate_mutations(wireless_current)?;
            mutations.into_iter().chain(wireless_mutations).collect()
        } else {
            mutations
        };
        let mutations = if let (Some(w60g_target), Some(w60g_current)) =
            (&self.w60g_target, &self.w60g_current)
        {
            let w60g_mutations = w60g_target.generate_mutations(w60g_current)?;
            mutations.into_iter().chain(w60g_mutations).collect()
        } else {
            mutations
        };
        let mutations = if let (Some(capsman_target), Some(capsman_current)) =
            (&self.capsman_target, &self.capsman_current)
        {
            let capsman_mutations = capsman_target.generate_mutations(capsman_current)?;
            mutations.into_iter().chain(capsman_mutations).collect()
        } else {
            mutations
        };

        Ok(mutations)
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
            return Vec::new();
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
        self.base_target.poe_warnings(&self.base_current, &powered)
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
    let mut stream = raw
        .send_command(
            b"/interface/ethernet/poe/monitor".as_slice(),
            |cmd| {
                cmd.attribute(b"numbers".as_slice(), numbers.as_bytes())
                    .attribute(b"once".as_slice(), b"".as_slice())
            },
            (),
        )
        .await;
    let mut powered = HashSet::new();
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(row) => {
                if let (Some(name), Some("powered-on")) = (
                    update::field(&row, "name"),
                    update::field(&row, "poe-out-status"),
                ) {
                    powered.insert(AsciiString::from(name));
                }
            }
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { message, .. } => return Err(Error::Refused(message)),
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

/// What applying the config to a device did.
#[derive(SimpleObject)]
pub struct ApplyResult {
    /// the script that was run on the device; empty if there was nothing to do
    applied: String,
    /// the CAPs' radios a controller provisioned again, since the WLANs changed (German, for
    /// the UI)
    reprovisioned: Vec<String>,
    /// what the device still lacks afterwards (empty when everything was applied), or why it
    /// couldn't be checked
    remaining: String,
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
    let rows = update::rows(raw, format!("{menu}/print").as_bytes()).await?;
    let ids = rows
        .iter()
        .filter(|row| {
            // `/caps-man/radio` names the CAP in `remote-cap-identity`, `/interface/wifi/radio`
            // in `cap`
            update::field(row, "local") != Some("true")
                && ["remote-cap-identity", "cap"]
                    .iter()
                    .any(|name| update::field(row, name).is_some_and(|cap| !cap.is_empty()))
        })
        .filter_map(|row| update::field(row, ".id"))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(0);
    }
    let command = format!("{menu}/provision");
    let mut stream = raw
        .send_command(
            command.as_bytes(),
            |cmd| cmd.attribute(b"numbers".as_slice(), ids.join(",").as_bytes()),
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(_) => {}
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { message, .. } => {
                return Err(Error::Refused(message));
            }
        }
    }
    Ok(ids.len())
}

/// The name of the `/system/script` entry the config is applied with.
const APPLY_SCRIPT: &str = "netbox-provisioner-apply";
/// How long the device may take to run the script and be reachable again.
const APPLY_TIMEOUT: Duration = Duration::from_secs(90);

impl AccessibleDevice {
    /// Applies the generated config: the diff runs as a script on the device itself, so it
    /// completes even when it changes the path this connection takes. Then the diff is built
    /// again over a new connection; whatever is left wasn't applied (RouterOS stops a script
    /// at its first error).
    pub async fn apply(&self) -> Result<ApplyResult, Error> {
        if !CONFIG.allow_apply {
            return Err(Error::ApplyNotAllowed);
        }
        let mut cfg = DeviceCfg::fetch(&self.client, &self.device_config).await?;
        cfg.generate_for(&self.device_config)?;
        let mutations = cfg.sorted_mutations()?;
        let applied = render_script(&mutations)?;
        if applied.is_empty() {
            return Ok(ApplyResult {
                applied: String::new(),
                reprovisioned: Vec::new(),
                remaining: String::new(),
            });
        }
        let raw = self.connect_again::<SimpleResult>().await?;
        apply_mutations_via_script(&raw, &mutations, APPLY_SCRIPT).await?;
        drop(raw);

        let deadline = Instant::now() + APPLY_TIMEOUT;
        let remaining = loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let remaining = self.remaining().await;
            match &remaining {
                Ok(script) if script.is_empty() => break remaining,
                _ if Instant::now() >= deadline => break remaining,
                _ => info!("{}: not done yet", self.device_config.name()),
            }
        };
        let reprovisioned = if self.device_config.wlan_controller_of().is_some() {
            self.reprovision(reprovisioned_menus(&mutations)).await
        } else {
            Vec::new()
        };
        Ok(ApplyResult {
            applied: applied.into(),
            reprovisioned,
            remaining: match remaining {
                Ok(script) => script.into(),
                Err(error) => format!("cannot check the device: {error}"),
            },
        })
    }
    /// Provisions the radios of `menus` (`reprovisioned_menus`) again, which reconnects the
    /// CAPs' clients; what it did or why it couldn't, for the UI.
    async fn reprovision(&self, menus: Vec<&'static str>) -> Vec<String> {
        if menus.is_empty() {
            return Vec::new();
        }
        let raw = match self.connect_again::<SimpleResult>().await {
            Ok(raw) => raw,
            Err(error) => return vec![format!("Radios nicht neu provisioniert: {error}")],
        };
        let mut report = Vec::new();
        for menu in menus {
            match provision_radios(&raw, menu).await {
                Ok(0) => {}
                Ok(count) => report.push(format!("{count} Radios neu provisioniert ({menu})")),
                Err(error) => report.push(format!("{menu} nicht neu provisioniert: {error}")),
            }
        }
        report
    }
    /// The packages besides `routeros` the device needs (`needed_radio_package`); none on a
    /// CHR, which has no RouterBOARD.
    pub async fn wanted_packages(&self) -> Result<Vec<String>, Error> {
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
            .map(|package| package.name().to_string())
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
    /// Fetches a device's current config over an already-connected client.
    /// A free function rather than a method on `AccessibleDevice` (which has
    /// no public constructor outside this crate) since it never actually
    /// used `AccessibleDevice`'s own state - any reachable `MikrotikDevice`
    /// connection works, which is what the multi-CHR test harness
    /// (`netbox-provisioner/backend/tests/chr_network/`) needs: its nodes
    /// are reached via dynamic hostfwd ports, not `AccessibleDevice::
    /// create_client`'s hardcoded port 8728.
    pub async fn fetch(
        client: &MikrotikDevice,
        device: &DeviceAccess,
    ) -> Result<DeviceCfg, SetupError> {
        let routerboard = <SystemRouterboardState as SingleResource>::fetch(client)
            .await?
            .ok_or(SetupError::RouterboardNotDefined)?;
        Self::fetch_with_board_model(client, &routerboard.model.0, device).await
    }

    /// Like [`Self::fetch`], but takes the board model name directly instead
    /// of reading it from `/system/routerboard` - CHR has no routerboard at
    /// all (it isn't a physical RouterBOARD), so the multi-CHR test harness
    /// (`netbox-provisioner/backend/tests/chr_network/`) needs to supply the
    /// board it's emulating itself rather than have it detected live.
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
        let (wifi_current, wifi_target) = if needed.is_some_and(RadioPackage::is_wifi)
            || device.wlan_controller_of().is_some()
            || wifi_left_on().await
        {
                (
                    Some(WifiDeviceDataCurrent::fetch(client).await?),
                    Some(WifiDeviceDataTarget::detect_device(client).await?),
                )
            } else {
                (None, None)
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
                SystemRouterboardState::fetch(&client)
                    .await?
                    .expect("system/routerboard not found"),
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
                    newest: newest.to_string(),
                    upgrade: newest > state.version,
                    update: step.is_some(),
                });
            }
            // RouterOS 6 goes to 7 for its packages (`next_step`)
            let adjust_packages = state.version.major() >= 7
                && update::next_step(&state, &state.version, &state.version, &wanted).is_some();
            Ok::<_, Error>(RouterOsVersions {
                adjust_packages,
                version: state.version.to_string(),
                packages: state.packages,
                wanted: ["routeros".to_string()].into_iter().chain(wanted).collect(),
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
    version: String,
    /// installed packages
    packages: Vec<String>,
    /// the packages an update leaves installed
    wanted: Vec<String>,
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
    newest: String,
    /// whether the device runs an older version
    upgrade: bool,
    /// whether an update changes anything (version or packages)
    update: bool,
}
