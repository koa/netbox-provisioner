use crate::device::ros::l2::{EndpointNameGenerator, KeptNames, L2Setup, NameGenerator};
use crate::device::ros::{
    CapsManDeviceDataCurrent, CapsManDeviceDataTarget, WifiDeviceDataCurrent,
    WifiDeviceDataTarget,
};
use crate::{
    Error,
    config::CONFIG,
    error_info::{ApiError, ErrorInfo},
    device::{
        AccessibleDevice, GraphqlSystemRouterboard, PingResult, neighbors,
        ros::{
            fallback,
            BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, W60GDeviceDataCurrent,
            W60GDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
            hw_facts::{RadioPackage, build_w60g_ports, radio_package},
            needed_radio_package,
            warning::{ApplyWarning, CapOffOutsideWlanGroup},
        },
        device_mode, partitions, raw, update,
        radio_hold::RadioHold,
        watchdog::Watching,
    },
    topology::{PhysicalPortId, access::device::DeviceAccess},
};
use crate::{graphql::scalars::ScalarDuration, job::Log};
use async_graphql::{Context, Enum, Object, SimpleObject};
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::{
    MikrotikDevice,
    ascii::AsciiString,
    generator::Generator,
    hwconfig::DeviceType,
    model::{
        CapsManManagerCfg, CertificateCfg, InterfaceWifiCapCfg, InterfaceWirelessCapCfg, InterfaceWifiCapsmanCfg, InterfaceWifiState,
        InterfaceWifiCapsmanState,
        ReferenceType, SystemIdentityCfg, SystemPackageState, SystemRouterboardState, YesNo,
    },
    resource::{ResourceMutation, SingleResource, apply_mutations_via_script, collect_resource},
};
use async_graphql::futures_util::future::join_all;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::IpAddr,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

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
    wifi_warnings: Vec<ApplyWarning>,
    /// the board's radios NetBox leaves alone, with the CAP off (`CapOffOutsideWlanGroup`)
    idle_radios: Option<RadioPackage>,
    /// the addresses a watchdog would watch that don't answer (`generate_live`)
    watchdog_warnings: Vec<ApplyWarning>,
}

impl DeviceCfg {
    pub fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        name_generator: N,
    ) -> Result<(), SetupError> {
        self.base_target.set_name_and_loopback(device);

        // a legacy radio CAPsMAN manages keeps its name, NetBox's is in a warning
        let mut name_generator = name_generator;
        let kept = self
            .wireless_current
            .as_ref()
            .map(WirelessDeviceDataCurrent::managed_radios)
            .unwrap_or_default();
        let expected = device
            .interfaces()
            .iter()
            .filter_map(|interface| {
                let default_name = AsciiString::from(interface.external_port()?.to_string());
                kept.contains_key(&default_name).then(|| {
                    let name = name_generator.generate_interface_name(interface).into();
                    (default_name, name)
                })
            })
            .collect::<HashMap<_, Box<str>>>();
        let l2 = L2Setup::new(
            device,
            KeptNames {
                inner: name_generator,
                kept,
            },
        )?;
        let mut warnings = Vec::new();

        self.base_target.setup_addressing(device, &l2)?;
        if let Some(wireless_target) = self.wireless_target.as_mut() {
            wireless_target.generate_from(device, &l2, self.wireless_current.as_ref())?;
            if let Some(current) = &self.wireless_current {
                warnings.extend(wireless_target.keep_managed_radios(current, &expected));
            }
        }
        if let Some(w60g_target) = self.w60g_target.as_mut() {
            w60g_target.generate_from(device, &l2, self.w60g_current.as_ref())?;
        }
        if let Some(wifi_target) = self.wifi_target.as_mut() {
            warnings.extend(wifi_target.generate_from(
                device,
                &l2,
                self.wifi_current.as_ref(),
                &self.capsman_certificates,
                self.radio_package,
                &self.bound_radios,
            )?);
        }
        if let Some(capsman_target) = self.capsman_target.as_mut() {
            capsman_target.generate_from(device)?;
        }
        self.base_target
            .setup_ports(device, &l2, self.radio_package)?;
        if let Some(package) = self.idle_radios {
            warnings.push(ApplyWarning::CapOffOutsideWlanGroup(CapOffOutsideWlanGroup {
                package: package.name().into(),
            }));
        }
        self.wifi_warnings = warnings;
        Ok(())
    }
    /// [`Self::generate_from`] with the provisioner's own needs on top: the device the
    /// provisioner reaches the network through gets its SOCKS proxy, and the watchdogs find
    /// their way to it too.
    pub fn generate_for(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        self.generate_from(device, EndpointNameGenerator)?;
        self.base_target
            .setup_watchdog(device, &crate::graphql::management_tree(device))?;
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
    pub fn generate_mutations(&self) -> Result<Box<[ResourceMutation<'_>]>, Error> {
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
    /// [`Self::generate_for`] with the watchdogs only on addresses that answer from the device,
    /// asked over `raw` (`keep_silent_watchdogs`): the plan and every check after the apply
    /// see the same, since a silent address keeps what the device has.
    pub async fn generate_live(
        &mut self,
        device: &DeviceAccess,
        raw: &RawDevice<SimpleResult>,
    ) -> Result<(), Error> {
        self.generate_for(device)?;
        let addresses = self.base_target.watched_addresses();
        let answering = answering(raw, &addresses).await;
        let topology = device.topology().clone();
        self.watchdog_warnings = self.base_target.keep_silent_watchdogs(
            &self.base_current,
            &answering,
            |address| {
                topology
                    .list_devices()
                    .find(|device| crate::device::watchdog::addresses(device).contains(&address))
                    .map(|device| device.name().into())
                    .unwrap_or_default()
            },
        );
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
            return [self.wifi_warnings.clone(), self.watchdog_warnings.clone()].concat();
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
        warnings.extend(self.watchdog_warnings.iter().cloned());
        warnings
    }
}

/// What RouterOS has without anyone adding it, which mutations may refer to. Not the
/// `capsman` a bound radio reports without such an entry (cAP ax, 7.23.7): the target always
/// has that configuration, and taking it as there let router-stall-burri's radios get
/// `configuration=capsman` before it was added, which RouterOS refused.
pub(super) fn provided_by_routeros() -> [(ReferenceType, std::borrow::Cow<'static, [u8]>); 5] {
    [
        (ReferenceType::Interface, b"lo".into()),
        (ReferenceType::RoutingTable, b"main".into()),
        (ReferenceType::FirewallChain, b"input".into()),
        (ReferenceType::FirewallChain, b"output".into()),
        (ReferenceType::FirewallChain, b"forward".into()),
    ]
}

/// The ones of `addresses` that answer a ping from the device, all asked at once.
async fn answering(raw: &RawDevice<SimpleResult>, addresses: &[IpAddr]) -> BTreeSet<IpAddr> {
    let pings = addresses.iter().map(|address| async move {
        let text = address.to_string();
        let attributes = [("address", text.as_str()), ("count", "3"), ("interval", "0.3")];
        let ping = raw::command(raw, "/ping", &attributes);
        let answered = tokio::time::timeout(Duration::from_secs(10), ping)
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| raw::field(row, "received").is_some_and(|received| received != "0"))
            });
        answered.then_some(*address)
    });
    join_all(pings).await.into_iter().flatten().collect()
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
    /// what applying it would do that the operator should know before
    warnings: Vec<ApplyWarning>,
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
    /// the part of `delay` for a radio to come back ([`RADIO_RESTART`]), when the apply
    /// changes one
    pub radio_wait: Option<ScalarDuration>,
}

/// No fallback: the device mode `device_mode` has the scheduler off (Device-Mode on the device
/// page). The apply goes on; should it cut the device off, nothing brings the old config back.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FallbackUnavailable {
    pub device_mode: Box<str>,
}

/// The script runs on the device, or stopped at the line of `error`; the apply waits up to
/// `timeout` for the device to be in line.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyRunning {
    pub timeout: ScalarDuration,
    pub error: Option<ErrorInfo>,
}

/// The script is on the device, waiting for the other devices of the batch; once started, it
/// applies after `lead`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyWritten {
    pub lead: ScalarDuration,
}

/// The script is started and applies in `starts_in`; `error` if it couldn't be started or
/// wasn't seen running (the check after the batch tells whether it applied).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyStarted {
    pub starts_in: ScalarDuration,
    pub error: Option<ErrorInfo>,
}

/// What the device still lacks after the apply (empty when everything was applied), or why it
/// couldn't be checked.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ApplyChecked {
    pub remaining: Box<str>,
    pub error: Option<ErrorInfo>,
}

/// The fallback is taken away: the device is reachable with the new config.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FallbackStopped {
    pub after: ScalarDuration,
}

/// The fallback stays: the device loads the backup from before the apply in about `fires_in`
/// and reboots.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FallbackLeft {
    pub fires_in: ScalarDuration,
    pub reason: FallbackLeftReason,
    /// why taking it away failed (`NotStopped`)
    pub error: Option<ErrorInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum FallbackLeftReason {
    /// the apply was cancelled
    Cancelled,
    /// the check after the apply didn't reach the device (`ApplyChecked.error` says why)
    NotReached,
    /// reached, but taking the fallback away failed until shortly before it fires
    NotStopped,
    /// reached, but not in line after a batch's script: all of the batch go back
    NotApplied,
    /// another device of the batch wasn't reached or isn't in line (`BatchIncomplete`): all
    /// go back
    BatchIncomplete,
}

/// A controller provisioned `count` CAP radios of `menu` again, since the WLANs changed (their
/// clients reconnect); or couldn't (`error`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RadiosReprovisioned {
    pub menu: Box<str>,
    pub count: u32,
    pub error: Option<ErrorInfo>,
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
pub(crate) const APPLY_TIMEOUT: Duration = Duration::from_secs(90);
/// Before the fallback fires, the provisioner stops trying to reach the device: taking the
/// fallback away just as it loads the backup would leave it half done.
pub(crate) const FALLBACK_MARGIN: Duration = Duration::from_secs(30);

/// How long an apply waits for its script to answer before it goes on to check the device.
const SCRIPT_ANSWER: Duration = APPLY_TIMEOUT;

/// Leaves `planned` on the device as a script that waits `lead` before it applies
/// (`ApplyWritten`), until [`start_script`] starts it. The batch writes all its devices'
/// scripts first and then starts them at once, the wait is for starting all of them before
/// the first changes cut a connection.
pub(crate) async fn write_script(
    raw: &RawDevice<SimpleResult>,
    planned: &PlannedApply,
    lead: Duration,
    log: &Log,
) -> Result<(), Error> {
    let _ = raw::command(raw, "/system/script/remove", &[("numbers", APPLY_SCRIPT)]).await;
    let source = format!(":delay {}s\n{}", lead.as_secs().max(1), planned.script);
    raw::command(raw, "/system/script/add", &[("name", APPLY_SCRIPT), ("source", &source)]).await?;
    log.emit(ApplyWritten { lead: lead.into() });
    Ok(())
}

/// Saves the backup the device loads in `delay` unless the fallback is taken away
/// (`FallbackArmed`, with the part of it for a radio to come back), over `raw`; `false` where
/// the device mode doesn't allow it (`FallbackUnavailable`), a warning only.
pub(crate) async fn arm_fallback_on(
    raw: &RawDevice<SimpleResult>,
    delay: Duration,
    radio_wait: Option<Duration>,
    log: &Log,
) -> Result<bool, Error> {
    if let Some(device_mode) = fallback::arm(raw, delay).await? {
        log.emit(FallbackUnavailable { device_mode });
        return Ok(false);
    }
    log.emit(FallbackArmed {
        delay: delay.into(),
        radio_wait: radio_wait.map(Into::into),
    });
    Ok(true)
}

/// Starts the script [`write_script`] left, in the background (`:execute`,
/// nothing waits on the connection), and makes sure it runs; `false` when it doesn't (yet).
pub(crate) async fn start_script(raw: &RawDevice<SimpleResult>) -> Result<bool, Error> {
    let run = format!("/system script run \"{APPLY_SCRIPT}\"");
    raw::command(raw, "/execute", &[("script", &run)]).await?;
    script_running(raw).await
}

/// Whether the apply script runs on the device.
pub(crate) async fn script_running(raw: &RawDevice<SimpleResult>) -> Result<bool, Error> {
    Ok(raw::rows(raw, "/system/script/job/print")
        .await?
        .iter()
        .any(|job| raw::field(job, "script") == Some(APPLY_SCRIPT)))
}

/// Takes the apply script away, before it is started; nothing to do for one never written.
pub(crate) async fn remove_script(raw: &RawDevice<SimpleResult>) -> Result<(), Error> {
    for script in raw::rows(raw, "/system/script/print").await? {
        if raw::field(&script, "name") == Some(APPLY_SCRIPT)
            && let Some(id) = raw::field(&script, ".id")
        {
            raw::command(raw, "/system/script/remove", &[("numbers", id)]).await?;
        }
    }
    Ok(())
}

/// When to stop trying to reach a device whose fallback `fires` then.
pub(crate) fn give_up(fires: Instant) -> Instant {
    fires.checked_sub(FALLBACK_MARGIN).unwrap_or(fires)
}

pub(crate) fn fallback_left(
    log: &Log,
    fires: Instant,
    reason: FallbackLeftReason,
    error: Option<&Error>,
) {
    log.emit(FallbackLeft {
        fires_in: fires.saturating_duration_since(Instant::now()).into(),
        reason,
        error: error.map(ErrorInfo::from),
    });
}

pub(crate) fn emit_checked(log: &Log, remaining: &Result<Box<str>, Error>) {
    log.emit(match remaining {
        Ok(script) => ApplyChecked {
            remaining: script.clone(),
            error: None,
        },
        Err(error) => ApplyChecked {
            remaining: "".into(),
            error: Some(ErrorInfo::from(error)),
        },
    });
}

/// A device's diff, read and generated for an apply.
pub(crate) struct PlannedApply {
    cfg: DeviceCfg,
    script: Box<str>,
    /// it changes a radio (`/interface/wireless`, `/interface/wifi`)
    restarts_radio: bool,
}

impl PlannedApply {
    /// Leaves the watch address as the device has it (`radio_hold`), planned again
    /// (`ApplyPlanned`) where that changes the script.
    pub(crate) fn hold_watchdog(&mut self, log: &Log) -> Result<(), Error> {
        self.cfg.base_target.watchdog.watch_address = self.cfg.base_current.watchdog.watch_address.clone();
        let script = render_script(&self.cfg.sorted_mutations()?)?;
        if script != self.script {
            log.emit(ApplyPlanned {
                script: script.clone(),
            });
            self.script = script;
        }
        Ok(())
    }
    /// The device is in line already.
    pub(crate) fn is_empty(&self) -> bool {
        self.script.trim().is_empty()
    }
    /// The longer fallback for a radio it changes, if it does ([`RADIO_RESTART`]).
    pub(crate) fn radio_wait(&self) -> Option<Duration> {
        self.restarts_radio.then_some(RADIO_RESTART)
    }
}

/// How long a radio may stay off after a change: a 5 GHz AP with `frequency=auto` picks its
/// channel again, and on a DFS channel listens for radar before it sends, 10 min where the
/// channel touches the weather radar band (5600–5650 MHz; lab-sxt-a took 10 min on 5680 MHz
/// at 80 MHz after a new SSID and key). Radar while it runs moves it to another channel that
/// listens again (berghof-haus-stall: radar on 5680 MHz 5 min after the apply, then 5560 MHz),
/// so the time covers two of them, plus the scans and the station finding it. Switzerland's
/// outdoor channels are all DFS, and the high ones are kept for their higher power.
pub(crate) const RADIO_RESTART: Duration = Duration::from_secs(24 * 60);

/// How long one check of a device after its apply may take.
const CHECK_TIMEOUT: Duration = Duration::from_secs(25);

/// Whether `mutations` change a radio, which then may stay off for [`RADIO_RESTART`].
fn restarts_radio(mutations: &[ResourceMutation<'_>]) -> bool {
    mutations.iter().any(|m| {
        m.resource.starts_with(b"interface/wireless") || m.resource.starts_with(b"interface/wifi")
    })
}

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
        let mut planned = self.plan_apply(log).await?;
        if planned.is_empty() {
            return Ok(());
        }
        log.check()?;
        let hold = match planned.radio_wait() {
            Some(_) => {
                let applied = BTreeSet::from([self.device_config.id()]);
                Some(RadioHold::take(self, &mut planned, &applied, &Default::default(), log).await?)
            }
            None => None,
        };
        let (applied, back) = self.apply_planned(&planned, log).await;
        match hold {
            Some(hold) if back => hold.release(self, log).await,
            Some(hold) => hold.leave(log),
            None => {}
        }
        applied
    }
    /// [`Self::apply`] once planned; `true` along with it where the fallback won't fire:
    /// nothing applied, or the device reached again and the fallback taken away.
    async fn apply_planned(&self, planned: &PlannedApply, log: &Log) -> (Result<(), Error>, bool) {
        let radio_wait = planned.radio_wait();
        let delay = CONFIG.apply_fallback() + radio_wait.unwrap_or_default();
        let fires = Instant::now() + delay;
        let armed = match self.arm_fallback(delay, radio_wait, log).await {
            Ok(armed) => armed,
            Err(error) => return (Err(error), true),
        };
        self.run_script(planned, SCRIPT_ANSWER, log).await;
        // cancelled from here on, the fallback stays
        let wait = async |time| {
            let waited = log.unless_cancelled(tokio::time::sleep(time)).await;
            if waited.is_err() && armed {
                fallback_left(log, fires, FallbackLeftReason::Cancelled, None);
            }
            waited
        };

        let settled = Instant::now() + APPLY_TIMEOUT;
        let give_up = give_up(fires);
        let remaining = loop {
            if let Err(error) = wait(Duration::from_secs(3)).await {
                return (Err(error), false);
            }
            match self.check_applied(settled).await {
                Some(remaining) => break remaining,
                None if Instant::now() >= give_up => break self.remaining().await,
                None => info!("{}: not done yet", self.device_config.name()),
            }
        };
        emit_checked(log, &remaining);
        let back = remaining.is_ok();
        match (armed, back) {
            (false, _) => {}
            (true, true) => {
                if let Err(error) = self.stop_fallback(fires, delay, log).await {
                    return (Err(error), false);
                }
            }
            (true, false) => fallback_left(log, fires, FallbackLeftReason::NotReached, None),
        }
        (self.after_apply(planned, log).await, back)
    }
    /// The diff to apply, planned (`ApplyPlanned`); an empty one is nothing to do.
    pub(crate) async fn plan_apply(&self, log: &Log) -> Result<PlannedApply, Error> {
        let mut cfg = DeviceCfg::fetch(&self.client, &self.device_config).await?;
        cfg.generate_live(&self.device_config, &self.connect_again().await?).await?;
        let mutations = cfg.sorted_mutations()?;
        let script = render_script(&mutations)?;
        let restarts_radio = restarts_radio(&mutations);
        drop(mutations);
        log.emit(ApplyPlanned {
            script: script.clone(),
        });
        Ok(PlannedApply { cfg, script, restarts_radio })
    }
    /// Saves the backup the device loads in `delay` unless the fallback is taken away
    /// (`FallbackArmed`); `false` where the device mode doesn't allow it.
    pub(crate) async fn arm_fallback(
        &self,
        delay: Duration,
        radio_wait: Option<Duration>,
        log: &Log,
    ) -> Result<bool, Error> {
        arm_fallback_on(&self.connect_again::<SimpleResult>().await?, delay, radio_wait, log).await
    }
    /// Runs `planned` as a script on the device, waiting up to `answer` for it to answer
    /// (`ApplyRunning`): the script goes on without the provisioner, also when it cuts the
    /// connection it was started over, so the apply goes on to check the device. An error is
    /// mostly the script's own: it stopped at a line, the ones before are applied.
    pub(crate) async fn run_script(&self, planned: &PlannedApply, answer: Duration, log: &Log) {
        let error = async {
            let raw = self.connect_again::<SimpleResult>().await?;
            let mutations = planned.cfg.sorted_mutations()?;
            match tokio::time::timeout(answer, apply_mutations_via_script(&raw, &mutations, APPLY_SCRIPT)).await {
                Ok(result) => result.map_err(Error::from),
                Err(_) => Err(Error::ScriptNoAnswer(answer)),
            }
        }
        .await
        .err();
        log.emit(ApplyRunning {
            timeout: APPLY_TIMEOUT.into(),
            error: error.map(|error| ErrorInfo::from(&error)),
        });
    }
    /// What is left of the diff once the device is in line, or `settled` has passed: `None`
    /// while it isn't, or can't be reached.
    pub(crate) async fn check_applied(&self, settled: Instant) -> Option<Result<Box<str>, Error>> {
        match self.remaining().await {
            Ok(script) if script.is_empty() || Instant::now() >= settled => Some(Ok(script)),
            _ => None,
        }
    }
    /// Takes the fallback away (`FallbackStopped`), trying until shortly before it `fires`
    /// (`FallbackLeft`). Cancelled between the tries, the fallback stays.
    pub(crate) async fn stop_fallback(
        &self,
        fires: Instant,
        delay: Duration,
        log: &Log,
    ) -> Result<(), Error> {
        let give_up = give_up(fires);
        loop {
            let disarmed = match self.connect_again::<SimpleResult>().await {
                Ok(raw) => fallback::disarm(&raw).await,
                Err(error) => Err(error),
            };
            match disarmed {
                Ok(()) => {
                    log.emit(FallbackStopped {
                        after: delay
                            .saturating_sub(fires.saturating_duration_since(Instant::now()))
                            .into(),
                    });
                    return Ok(());
                }
                Err(error) if Instant::now() >= give_up => {
                    fallback_left(log, fires, FallbackLeftReason::NotStopped, Some(&error));
                    return Ok(());
                }
                Err(error) => info!("{}: fallback not stopped yet: {error}", self.device_config.name()),
            }
            if log.unless_cancelled(tokio::time::sleep(Duration::from_secs(3))).await.is_err() {
                fallback_left(log, fires, FallbackLeftReason::Cancelled, None);
                return Err(Error::Cancelled);
            }
        }
    }
    /// What follows an apply: a controller provisions its CAPs' radios again.
    pub(crate) async fn after_apply(&self, planned: &PlannedApply, log: &Log) -> Result<(), Error> {
        if self.device_config.wlan_controller_of().is_some() {
            log.check()?;
            let mutations = planned.cfg.sorted_mutations()?;
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
            error: Some(ErrorInfo::from(&error)),
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
    /// Whether the checks keep the watch address as the device has it (`radio_hold`).
    pub(crate) fn hold_watchdog(&self, held: bool) {
        self.watchdog_held.store(held, Ordering::Relaxed);
    }
    /// What the watch address is to be after `radio_hold`: as NetBox has it where that answers
    /// (none, where it has none), else `old`, as the device had it.
    pub(crate) async fn watch_after_hold(
        &self,
        raw: &RawDevice<SimpleResult>,
        old: &Watching,
    ) -> Result<Watching, Error> {
        let mut cfg = DeviceCfg::fetch(&self.connect_again().await?, &self.device_config).await?;
        cfg.generate_for(&self.device_config)?;
        Ok(match cfg.base_target.watchdog.watch_address.value().copied() {
            None => Watching::default(),
            Some(address) if answering(raw, &[address]).await.contains(&address) => Watching {
                watch_address: Some(address.to_string().into()),
                poe_pings: Vec::new(),
            },
            Some(_) => old.clone(),
        })
    }
    /// The diff of a fresh look at the device.
    pub(crate) async fn remaining_diff(&self) -> Result<Box<str>, Error> {
        self.remaining().await
    }
    /// The diff left, over a new connection, within [`CHECK_TIMEOUT`]: the apply checks
    /// every few seconds, and a device reached again only for a short while must be caught.
    async fn remaining(&self) -> Result<Box<str>, Error> {
        tokio::time::timeout(CHECK_TIMEOUT, async {
            let client = self.connect_again().await?;
            let mut cfg = DeviceCfg::fetch(&client, &self.device_config).await?;
            cfg.generate_live(&self.device_config, &self.connect_again().await?).await?;
            if self.watchdog_held.load(Ordering::Relaxed) {
                cfg.base_target.watchdog.watch_address = cfg.base_current.watchdog.watch_address.clone();
            }
            cfg.script()
        })
        .await
        .unwrap_or(Err(Error::Timeout(self.address)))
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
        // the 60 GHz radios have their own menu (in `wireless` too); such a board has no other
        // radio, a `wlan<N>` or `wifi<N>` in NetBox is its `wlan60-<N>` misnamed
        if !build_w60g_ports(model).is_empty()
            && let Some(port) = device.interfaces().iter().find_map(|interface| {
                interface
                    .external_port()
                    .filter(|port| matches!(port, PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_)))
            })
        {
            return Err(SetupError::PortNotFound(port));
        }
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
            w60g_current,
            w60g_target,
            capsman_current,
            capsman_target,
            bound_radios,
            wifi_warnings: Vec::new(),
            idle_radios,
            watchdog_warnings: Vec::new(),
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
    async fn identity(&self) -> GraphqlSystemIdentity<'_> {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl BaseDeviceDataTarget {
    async fn identity(&self) -> GraphqlSystemIdentity<'_> {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl AccessibleDevice {
    async fn ping(&self, count: Option<u8>) -> Result<Box<[PingResult]>, ApiError> {
        Ok(self.simple_ping(count.unwrap_or(1)).await.map_err(Error::from)?)
    }

    /// The device the connection actually reached, to tell it from another one answering at
    /// the same address.
    async fn connected(&self) -> crate::device::Connected {
        self.connected_device()
    }

    async fn device_stats(&self) -> Result<DeviceStats, ApiError> {
        Ok(DeviceStats::fetch(&self.client).await?)
    }

    async fn config(&self) -> Result<DeviceCfg, ApiError> {
        Ok(DeviceCfg::fetch(&self.client, &self.device_config).await?)
    }
    /// The script that would bring the device in line with NetBox, and what to know before
    /// applying it; `null` with the error in the response's errors when that fails, so a
    /// device the generator fails on still shows the rest (an update may be what it needs).
    async fn generate_cfg(&self, ctx: &Context<'_>) -> Option<GeneratedCfg> {
        let generated = async {
            let mut device_cfg = DeviceCfg::fetch(&self.client, &self.device_config).await?;
            device_cfg
                .generate_live(&self.device_config, &self.connect_again().await?)
                .await?;
            Ok(GeneratedCfg {
                script: device_cfg.script()?,
                warnings: device_cfg
                    .warnings(async || self.connect_again::<SimpleResult>().await)
                    .await,
            })
        };
        nullable(ctx, generated.await)
    }
    /// The device mode and the features the provisioner needs that are off in it; `null`
    /// before RouterOS 7.17, which has no device mode.
    async fn device_mode(&self, ctx: &Context<'_>) -> Option<device_mode::DeviceMode> {
        let mode = async { device_mode::read(&self.connect_again().await?, &self.device_config).await };
        nullable(ctx, mode.await).flatten()
    }
    /// What the device sees at its ports against NetBox's cables there, the worst first.
    async fn neighbor_findings(&self) -> Result<Vec<neighbors::NeighborFinding>, ApiError> {
        Ok(neighbors::check(&self.connect_again().await?, &self.device_config).await?)
    }
    /// The ports the device reports feeding something over PoE, and what is at their other
    /// end (helper „PoE“).
    async fn poe_observed(&self) -> Result<Vec<crate::helper::poe::PoeObserved>, ApiError> {
        Ok(crate::helper::poe::observed(&self.connect_again().await?, &self.device_config).await?)
    }
    /// The partitions of the flash; `null` on a device without (16 MiB of flash, a CHR).
    async fn partitions(&self, ctx: &Context<'_>) -> Option<partitions::Partitions> {
        let read = async { partitions::read(&self.connect_again().await?).await };
        nullable(ctx, read.await).flatten()
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
pub(crate) fn nullable<T>(ctx: &Context<'_>, result: Result<T, Error>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            ctx.add_error(
                ctx.set_error_path(async_graphql::Error::from(ApiError(error)).into_server_error(ctx.item.pos)),
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
