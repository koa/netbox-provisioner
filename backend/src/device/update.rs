//! Scope: Bringing one device's RouterOS, packages and firmware to a version: download, upload,
//! reboot and check; not the way back (`update_fallback`), the order across devices (`rollout`)
//! nor rebooting and waiting itself (`reboot`).
//!
//! RouterOS updates pushed by the provisioner: it downloads the packages from MikroTik, copies
//! them onto the device over SCP and reboots it, so the device itself needs no route to the
//! internet. The target is the newest release of a channel (long-term, stable, testing,
//! development). RouterOS 6 goes to the last 6.49 first (the bundle package), from there to
//! the target RouterOS 7. RouterOS 7 gets `routeros` and the packages the device needs
//! (`device::ros::needed_radio_package`); other packages are uninstalled. After the system the
//! RouterBOARD firmware follows (`/system/routerboard/upgrade`, one more reboot). A device that
//! powers others over PoE reboots only while none of them updates ([`power`](super::power)).
//! Where the device has two partitions, the RouterOS from before stays on the other one as the
//! way back ([`update_fallback`]), and the update is done when the OSPF neighbors are back.
use crate::{
    Error,
    device::{
        access::act,
        dependency::{PowerChain, RebootLease},
        reboot::{
            REBOOT_TIMEOUT, RebootReason, reboot_and_wait, restart_and_wait, wait_until_back,
        },
        update_fallback,
    },
    device_state::UpdateState,
    graphql::scalars::ScalarU64,
    job::Log,
    lock::LockAnyway,
    saved_job::UpdateRun,
};
use async_graphql::{Enum, SimpleObject};
use log::warn;
use mikrotik_model::{
    action::{PackageDowngrade, PackageUninstall, RouterboardUpgrade},
    command::{Reads, Writes},
    model::{
        File, FileByName, IpSshCfg, IpSshPasswordAuthentication,
        SystemArchitecture as Architecture, SystemPackageState, SystemResourceState,
        SystemRouterboardState, UserSshKeysState,
    },
    resource::{ResourceMutation, SingleResource, Updatable, apply_mutation, collect_resource},
    value::RosValue,
};
use serde::Deserialize;
use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::time::{Instant, sleep, timeout};

const DOWNLOAD_URL: &str = "https://download.mikrotik.com/routeros";
/// The newest release of a channel, as `<version> <timestamp>`.
const NEWEST_URL: &str = "https://upgrade.mikrotik.com/routeros/NEWEST";
/// How long after `/system/reboot` a device may still answer with its old uptime.
const REBOOT_SENT_SETTLES: Duration = Duration::from_secs(30);

/// What the update needs of a device; implemented by
/// [`AccessibleDevice`](crate::device::AccessibleDevice) (through the management proxy) and by
/// test setups.
#[allow(async_fn_in_trait)]
pub trait DeviceLink {
    /// A fresh API connection (the device reboots in between).
    async fn api(&self) -> Result<crate::device::access::Session, Error>;
    /// Copies a file to the device's root directory.
    /// `progress` gets the bytes sent now and then ([`terminal::upload`](super::terminal::upload)).
    async fn upload(
        &self,
        name: &str,
        data: &[u8],
        progress: &(impl Fn(usize) + Sync),
    ) -> Result<(), Error>;
}

/// `bytes` of the package `file` (`total` bytes) are on their way to the device.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageUploading {
    pub file: Box<str>,
    pub bytes: ScalarU64,
    pub total: ScalarU64,
}

/// The version the update goes to: the newest of `channel`, or the one the device runs (only
/// its packages and firmware then).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateTarget {
    pub version: Box<str>,
    /// the channel's name (`long-term`); `None`: the version the device runs
    pub channel: Option<Box<str>>,
}

/// What the device runs before the update.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateStart {
    pub version: Box<str>,
    pub architecture: Box<str>,
    pub packages: Box<[Box<str>]>,
}

/// One update with a reboot ([`UpdateStep`]).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateStepPlanned {
    pub version: Box<str>,
    pub files: Box<[Box<str>]>,
    pub uninstall: Box<[Box<str>]>,
}

/// `/ip ssh` switched for a login with the password (the user has SSH keys, see
/// [`allow_password_login`]): for the upload, or the console and tunnel of an initial setup on
/// its neighbor; or back afterwards (`restored`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SshPasswordLogin {
    pub setting: Box<str>,
    pub value: Box<str>,
    pub restored: bool,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageDownloaded {
    pub file: Box<str>,
    pub bytes: ScalarU64,
}

/// The package is on the device, in full.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageUploaded {
    pub file: Box<str>,
}

/// The device is back with this RouterOS.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RouterOsRunning {
    pub version: Box<str>,
}

/// The RouterBOARD firmware is upgraded from `current` to `upgrade`, or runs `upgrade` after
/// the reboot (`running`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FirmwareUpgrade {
    pub current: Box<str>,
    pub upgrade: Box<str>,
    pub running: bool,
}

/// The device is up to date.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UpdateDone {
    pub from: Box<str>,
    pub to: Box<str>,
}

/// The device runs the older RouterOS it was downgraded to.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Downgraded {
    pub from: Box<str>,
    pub to: Box<str>,
}

/// A RouterOS version like `7.20.8` or a pre-release like `7.25rc1`, `7.26beta1`; a channel
/// suffix like ` (stable)` is ignored. A pre-release comes before its release, beta before rc.
#[derive(Debug, Clone)]
pub struct Version {
    numbers: Box<[u32]>,
    /// `("beta", 1)`, `("rc", 1)`; `None` for a release
    pre: Option<(Box<str>, u32)>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.split_whitespace().next()?;
        let (release, pre) = match text.find(|c: char| c.is_ascii_alphabetic()) {
            Some(at) => {
                let (release, tag) = text.split_at(at);
                let digits = tag.find(|c: char| c.is_ascii_digit()).unwrap_or(tag.len());
                let (kind, number) = tag.split_at(digits);
                (release, Some((kind.into(), number.parse().unwrap_or(0))))
            }
            None => (text, None),
        };
        let numbers = release
            .split('.')
            .map(|part| part.parse().ok())
            .collect::<Option<Box<[u32]>>>()?;
        Some(Version { numbers, pre })
    }
    pub fn major(&self) -> u32 {
        self.numbers[0]
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // 7.20 == 7.20.0
        let len = self.numbers.len().max(other.numbers.len());
        let at = |v: &Version, i: usize| v.numbers.get(i).copied().unwrap_or(0);
        // alpha < beta < rc < release
        let pre = |v: &Version| {
            v.pre.as_ref().map_or((u8::MAX, 0), |(kind, number)| {
                let rank = match kind.as_ref() {
                    "alpha" => 0,
                    "beta" => 1,
                    "rc" => 2,
                    _ => 3,
                };
                (rank, *number)
            })
        };
        (0..len)
            .map(|i| at(self, i).cmp(&at(other, i)))
            .find(|o| o.is_ne())
            .unwrap_or_else(|| pre(self).cmp(&pre(other)))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = self.numbers.iter().map(u32::to_string).collect();
        f.write_str(&parts.join("."))?;
        if let Some((kind, number)) = &self.pre {
            write!(f, "{kind}{number}")?;
        }
        Ok(())
    }
}

/// What the device runs.
#[derive(Debug, Clone)]
pub struct DeviceState {
    pub version: Version,
    /// `/system/resource architecture-name`
    pub architecture: Architecture,
    /// names of the installed packages that aren't disabled
    pub packages: Box<[Box<str>]>,
    /// `(current, upgrade)` RouterBOARD firmware; `None` without RouterBOARD (CHR)
    pub firmware: Option<(Box<str>, Box<str>)>,
    /// `/system/routerboard model`; `None` without RouterBOARD (CHR)
    pub model: Option<Box<str>>,
    pub uptime: Duration,
}

/// A RouterOS release channel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// the default: an update fleet-wide should take the releases that had longest to settle
    #[default]
    LongTerm,
    Stable,
    Testing,
    Development,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Channel::LongTerm => "long-term",
            Channel::Stable => "stable",
            Channel::Testing => "testing",
            Channel::Development => "development",
        }
    }
}

/// The newest release of RouterOS `major` in `channel`, from MikroTik. RouterOS 7 asks for
/// `NEWESTa7.<channel>`; `NEWEST7.<channel>` stopped at 7.12.1.
pub async fn newest(major: u32, channel: Channel) -> Result<Version, Error> {
    let cached = {
        let mut downloads = DOWNLOADS.lock_anyway();
        (downloads.holders > 0).then(|| {
            let key = (major, channel.name().into());
            downloads.newest.entry(key).or_default().clone()
        })
    };
    match cached {
        Some(cell) => cell
            .get_or_try_init(|| fetch_newest(major, channel))
            .await
            .cloned(),
        None => fetch_newest(major, channel).await,
    }
}

async fn fetch_newest(major: u32, channel: Channel) -> Result<Version, Error> {
    let prefix = if major >= 7 { "a" } else { "" };
    let url = format!("{NEWEST_URL}{prefix}{major}.{}", channel.name());
    let failed = |e: reqwest::Error| Error::Download(url.as_str().into(), e);
    let text = http()
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?
        .text()
        .await
        .map_err(failed)?;
    Version::parse(&text).ok_or_else(|| Error::UnknownRelease(url.into(), text.into()))
}

/// One update with a reboot: the version, the package files to install and the packages to
/// uninstall.
#[derive(Debug, PartialEq, Eq)]
pub struct UpdateStep {
    pub version: Version,
    pub files: Box<[Box<str>]>,
    pub uninstall: Box<[Box<str>]>,
}

/// The next step from `state` towards `target` with `routeros` and the `wanted` packages;
/// `None` when there. A device newer than `target` stays at its version, it only gets its
/// packages in line.
pub fn next_step(
    state: &DeviceState,
    target: &Version,
    newest_v6: &Version,
    wanted: &[Box<str>],
) -> Option<UpdateStep> {
    let arch = state.architecture;
    if state.version.major() < 7 {
        if state.version < *newest_v6 {
            // the bundle contains every RouterOS 6 package
            let arch = match arch {
                Architecture::X8664 => "x86",
                arch => architecture_name(arch),
            };
            return Some(UpdateStep {
                version: newest_v6.clone(),
                files: [format!("routeros-{arch}-{newest_v6}.npk").into()].into(),
                uninstall: [].into(),
            });
        }
        // RouterOS 7 replaces the whole RouterOS 6 package set
        return Some(v7_step(wanted, arch, target, [].into()));
    }
    let unwanted: Box<[Box<str>]> = state
        .packages
        .iter()
        .filter(|p| p.as_ref() != "routeros" && !wanted.contains(p))
        .cloned()
        .collect();
    if state.version < *target {
        return Some(v7_step(wanted, arch, target, unwanted));
    }
    let missing: Box<[Box<str>]> = wanted
        .iter()
        .filter(|p| !state.packages.contains(p))
        .cloned()
        .collect();
    if missing.is_empty() && unwanted.is_empty() {
        return None;
    }
    let step = v7_step(&missing, arch, &state.version, unwanted);
    Some(UpdateStep {
        files: step
            .files
            .into_iter()
            .filter(|file| !file.starts_with("routeros-"))
            .collect(),
        ..step
    })
}

/// Whether a downgrade from `running` to `target` is possible: RouterOS 7 to an older RouterOS 7.
pub fn downgradable(running: &Version, target: &Version) -> bool {
    running.major() >= 7 && target.major() >= 7 && target < running
}

/// The packages of `target` replacing those the device runs, for a downgrade (`downgradable`),
/// the same packages (a package `target` lacks fails its download, before anything is
/// uploaded).
pub fn downgrade_step(state: &DeviceState, target: &Version) -> Result<UpdateStep, Error> {
    if !downgradable(&state.version, target) {
        return Err(Error::NoDowngrade {
            running: state.version.to_string().into(),
            target: target.to_string().into(),
        });
    }
    let others: Box<[Box<str>]> = state
        .packages
        .iter()
        .filter(|p| p.as_ref() != "routeros")
        .cloned()
        .collect();
    Ok(v7_step(&others, state.architecture, target, [].into()))
}

/// The architecture as RouterOS and MikroTik's package files name it.
pub(crate) fn architecture_name(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::Arm => "arm",
        Architecture::Arm64 => "arm64",
        Architecture::Mipsbe => "mipsbe",
        Architecture::Tile => "tile",
        Architecture::X86 => "x86",
        Architecture::X8664 => "x86_64",
        Architecture::Mmips => "mmips",
    }
}

fn v7_step(
    wanted: &[Box<str>],
    arch: Architecture,
    version: &Version,
    uninstall: Box<[Box<str>]>,
) -> UpdateStep {
    // x86 packages carry no architecture
    let suffix = match arch {
        Architecture::X86 | Architecture::X8664 => String::new(),
        arch => format!("-{}", architecture_name(arch)),
    };
    UpdateStep {
        version: version.clone(),
        files: ["routeros"]
            .into_iter()
            .chain(wanted.iter().map(AsRef::as_ref))
            .map(|p| format!("{p}-{version}{suffix}.npk").into())
            .collect(),
        uninstall,
    }
}

/// SCP logs in with the password, which RouterOS refuses over SSH for a user with SSH keys
/// unless `always-allow-password-login` is on (up to 7.20) or `password-authentication` is
/// `yes` rather than `yes-if-no-key` (7.24): switched for the upload when keys are there.
/// The initial setup's console and tunnel on the neighbor log in with the password too
/// (sat-switch, 7.23.8: "ssh login refused" with a key for `admin`). Returns the setting and the
/// value to put back afterwards ([`restore_password_login`]); `log` hears of both.
pub(crate) async fn allow_password_login(
    device: &impl Writes,
    log: Option<&Log>,
) -> Result<Option<SshRelaxed>, Error> {
    let Some(ssh) = IpSshCfg::fetch(device).await? else {
        return Ok(None);
    };
    let relaxed = if ssh.always_allow_password_login == Some(false) {
        SshRelaxed::AlwaysAllowPasswordLogin
    } else if ssh.password_authentication == Some(IpSshPasswordAuthentication::YesIfNoKey) {
        SshRelaxed::PasswordAuthentication
    } else {
        return Ok(None);
    };
    if collect_resource::<UserSshKeysState>(device)
        .await?
        .is_empty()
    {
        return Ok(None);
    }
    relaxed.set(device, true, log).await?;
    Ok(Some(relaxed))
}

/// A setting of `/ip/ssh` [`allow_password_login`] switched.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SshRelaxed {
    /// `always-allow-password-login` (up to 7.20), off before
    AlwaysAllowPasswordLogin,
    /// `password-authentication` (7.24), `yes-if-no-key` before
    PasswordAuthentication,
}

impl SshRelaxed {
    fn setting(self) -> &'static str {
        match self {
            SshRelaxed::AlwaysAllowPasswordLogin => "always-allow-password-login",
            SshRelaxed::PasswordAuthentication => "password-authentication",
        }
    }
    /// Switches the setting on (password logins allowed) or back; `log` hears of it.
    async fn set(self, device: &impl Writes, on: bool, log: Option<&Log>) -> Result<(), Error> {
        let current = IpSshCfg::fetch(device)
            .await?
            .ok_or(Error::MissingDeviceState("/ip/ssh"))?;
        let mut target = current.clone();
        let value = match (self, on) {
            (SshRelaxed::AlwaysAllowPasswordLogin, on) => {
                target.always_allow_password_login = Some(on);
                if on { "yes" } else { "no" }
            }
            (SshRelaxed::PasswordAuthentication, true) => {
                target.password_authentication = Some(IpSshPasswordAuthentication::Yes);
                "yes"
            }
            (SshRelaxed::PasswordAuthentication, false) => {
                target.password_authentication = Some(IpSshPasswordAuthentication::YesIfNoKey);
                "yes-if-no-key"
            }
        };
        apply_mutation(device, &target.calculate_update(&current)).await?;
        if let Some(log) = log {
            log.emit(SshPasswordLogin {
                setting: self.setting().into(),
                value: value.into(),
                restored: !on,
            });
        }
        Ok(())
    }
}

/// Puts back what [`allow_password_login`] switched.
pub(crate) async fn restore_password_login(
    device: &impl Writes,
    relaxed: Option<SshRelaxed>,
    log: Option<&Log>,
) -> Result<(), Error> {
    match relaxed {
        Some(relaxed) => relaxed.set(device, false, log).await,
        None => Ok(()),
    }
}

/// Removes `files` from the device's root directory, where they are.
async fn remove_files(device: &impl Writes, files: &[Box<str>]) -> Result<(), Error> {
    for file in collect_resource::<File>(device).await? {
        let name = file.cfg.name;
        if files.iter().any(|f| f.as_bytes() == &*name.0) {
            apply_mutation(device, &ResourceMutation::remove::<FileByName>(&name)).await?;
        }
    }
    Ok(())
}

/// Marks the packages for removal at the next reboot.
async fn uninstall(device: &impl Writes, packages: &[Box<str>]) -> Result<(), Error> {
    for package in collect_resource::<SystemPackageState>(device).await? {
        if !packages.iter().any(|p| p.as_bytes() == &*package.name.0) {
            continue;
        }
        let number = package.id.encode_ros();
        let number = String::from_utf8_lossy(&number);
        act(device, &PackageUninstall { number: &number }).await?;
    }
    Ok(())
}

pub async fn read_state(device: &impl Reads) -> Result<DeviceState, Error> {
    let resource = SystemResourceState::fetch(device).await?;
    let packages = collect_resource::<SystemPackageState>(device).await?;
    // CHR has no RouterBOARD; the command fails there
    let routerboard = SystemRouterboardState::fetch(device).await.ok().flatten();
    state_from(resource.as_ref(), &packages, routerboard.as_ref())
}

/// [`DeviceState`] from `/system/resource`, `/system/package` and `/system/routerboard`, for who
/// reads them for more anyway.
pub(crate) fn state_from(
    resource: Option<&SystemResourceState>,
    packages: &[SystemPackageState],
    routerboard: Option<&SystemRouterboardState>,
) -> Result<DeviceState, Error> {
    let resource = resource.ok_or(Error::MissingDeviceState("/system/resource"))?;
    let version = Version::parse(&resource.version.to_string())
        .ok_or(Error::MissingDeviceState("version"))?;
    let packages = packages
        .iter()
        .filter(|package| !package.disabled)
        .map(|package| package.name.to_string().into())
        .collect();
    let firmware = routerboard.map(|routerboard| {
        (
            routerboard.current_firmware.to_string().into(),
            routerboard.upgrade_firmware.to_string().into(),
        )
    });
    Ok(DeviceState {
        version,
        architecture: resource.architecture_name,
        packages,
        firmware,
        model: routerboard.map(|routerboard| routerboard.model.to_string().into()),
        uptime: resource.uptime,
    })
}

/// The client for MikroTik's servers: a connection that stops delivering fails, rather than
/// holding the update forever (a package takes long on a slow line, but keeps coming).
fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("a client with timeouts")
    })
}

/// The size of a package file on MikroTik's server, without downloading it.
pub async fn package_size(version: &Version, file: &str) -> Result<Option<u64>, Error> {
    let url = format!("{DOWNLOAD_URL}/{version}/{file}");
    let failed = |e: reqwest::Error| Error::Download(url.as_str().into(), e);
    Ok(http()
        .head(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok()?.parse().ok()))
}

/// The packages downloaded while a rollout runs ([`DownloadCache`]), by version and file: its
/// devices of an architecture share them instead of each downloading its own. The newest
/// release of each channel too: an update may cut the way to the internet (its device the
/// uplink router), the others then go on with what was fetched (wng-926-sw1, Berg 926: its
/// update failed asking MikroTik for the newest release).
static DOWNLOADS: LazyLock<Mutex<Downloads>> = LazyLock::new(Default::default);

type Download = Arc<tokio::sync::OnceCell<Arc<[u8]>>>;

#[derive(Default)]
struct Downloads {
    holders: usize,
    files: HashMap<(Box<str>, Box<str>), Download>,
    /// by major version and channel name
    newest: HashMap<(u32, Box<str>), Arc<tokio::sync::OnceCell<Version>>>,
}

/// Keeps what updates download in memory while it lives (a rollout); dropped by the last
/// holder, the packages go.
pub struct DownloadCache(());

impl DownloadCache {
    pub fn hold() -> Self {
        DOWNLOADS.lock_anyway().holders += 1;
        DownloadCache(())
    }
}

impl Drop for DownloadCache {
    fn drop(&mut self) {
        let mut downloads = DOWNLOADS.lock_anyway();
        downloads.holders -= 1;
        if downloads.holders == 0 {
            downloads.files.clear();
            downloads.newest.clear();
        }
    }
}

/// `file` of `version`, from the [`DownloadCache`] while one is held.
pub(crate) async fn download(version: &Version, file: &str) -> Result<Arc<[u8]>, Error> {
    let cached = {
        let mut downloads = DOWNLOADS.lock_anyway();
        (downloads.holders > 0).then(|| {
            let key = (version.to_string().into(), file.into());
            downloads.files.entry(key).or_default().clone()
        })
    };
    match cached {
        Some(cell) => cell.get_or_try_init(|| fetch(version, file)).await.cloned(),
        None => fetch(version, file).await,
    }
}

async fn fetch(version: &Version, file: &str) -> Result<Arc<[u8]>, Error> {
    let url = format!("{DOWNLOAD_URL}/{version}/{file}");
    let failed = |e: reqwest::Error| Error::Download(url.as_str().into(), e);
    Ok(http()
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?
        .bytes()
        .await
        .map_err(failed)?
        .to_vec()
        .into())
}

/// Updates the device to `target` with `routeros` and the `wanted` packages, step by step (see
/// [`next_step`]), telling `log` how far it got. Writes to the device without asking for
/// `allow_apply`, that is up to the caller (`WritesAllowed`). `power` says which devices' updates
/// and reboots it has to wait for. On a device with two partitions the RouterOS from before
/// stays on the other one, and the device switches back to it `fallback` after a boot unless
/// the provisioner reaches it ([`update_fallback`]); a device without has to have a neighbor
/// in `way_back`, or updating it without one confirmed.
pub async fn update(
    link: &impl DeviceLink,
    power: &PowerChain,
    target: &Version,
    wanted: &[Box<str>],
    fallback: Duration,
    way_back: &update_fallback::WayBack,
    log: &Log,
) -> Result<(), Error> {
    update_keeping(
        link,
        power,
        target,
        wanted,
        fallback,
        way_back,
        &|_| {},
        log,
    )
    .await
}

/// [`update`], handing what going on after a restart needs (`saved_job`) to `keep` before
/// each state that goes on, where the fallback is set.
#[allow(clippy::too_many_arguments)]
pub async fn update_keeping(
    link: &impl DeviceLink,
    power: &PowerChain,
    target: &Version,
    wanted: &[Box<str>],
    fallback: Duration,
    way_back: &update_fallback::WayBack,
    keep: &(impl Fn(&UpdateRun) + Sync),
    log: &Log,
) -> Result<(), Error> {
    let mut state = read_state(&link.api().await?).await?;
    let from: Box<str> = state.version.to_string().into();
    log.emit(UpdateStart {
        version: from.clone(),
        architecture: architecture_name(state.architecture).into(),
        packages: state.packages.clone(),
    });
    let newest_v6 = if state.version.major() < 7 {
        Some(newest(6, Channel::LongTerm).await?)
    } else {
        None
    };
    let newest_v6 = newest_v6.as_ref().unwrap_or(target);
    // from the upload on a power loss can brick the device: it installs at the next boot
    log.enter(UpdateState::WaitingForChain);
    let _updating = log.unless_cancelled(power.updating(log)).await?;
    if next_step(&state, target, newest_v6, wanted).is_some() {
        log.check()?;
        log.enter(UpdateState::RememberingNeighbors);
        let device = link.api().await?;
        let ospf = update_fallback::ospf_up(&device).await?;
        log.enter(UpdateState::ArmingFallback);
        // RouterOS 6 has no partitions to copy to with this menu
        let armed = match state.version.major() >= 7 {
            true => update_fallback::arm(&device, fallback, way_back, log).await?,
            false => {
                update_fallback::arm_v6(way_back, log)?;
                None
            }
        };
        drop(device);
        let wait = match armed {
            Some(_) => fallback + REBOOT_TIMEOUT,
            None => REBOOT_TIMEOUT,
        };
        let run = armed.as_ref().map(|partition| UpdateRun {
            from: from.clone(),
            target: target.to_string().into(),
            newest_v6: newest_v6.to_string().into(),
            wanted: wanted.into(),
            partition: partition.clone(),
            wait,
            ospf: ospf.clone(),
            expected: None,
            failed: None,
        });
        let stepped = steps(
            link,
            power,
            state,
            target,
            newest_v6,
            wanted,
            armed.as_deref(),
            wait,
            run.as_ref(),
            keep,
            log,
        )
        .await;
        state = after_steps(link, stepped, run.as_ref(), keep, &ospf, log).await?;
    }
    firmware_and_done(link, power, state, from, log).await
}

/// Goes on with an update saved before a restart of the provisioner (`saved_job`), once
/// `link` reaches the device, in `state` (the job entered it with `Log::resume_in`): waits
/// for the reboot entered at `rebooting_since` and goes on with the steps, or decides about
/// the fallback, then as [`update`]. A reboot that was never sent before the restart (the
/// device answers, not booted since) ends as such at once, the fallback and the packages left
/// for the device page.
pub async fn resume(
    link: &impl DeviceLink,
    power: &PowerChain,
    state: UpdateState,
    run: &UpdateRun,
    rebooting_since: Instant,
    keep: &(impl Fn(&UpdateRun) + Sync),
    log: &Log,
) -> Result<(), Error> {
    let version = |text: &str| Version::parse(text).ok_or_else(|| Error::NoVersion(text.into()));
    let target = version(&run.target)?;
    let newest_v6 = version(&run.newest_v6)?;
    let _updating = log.unless_cancelled(power.updating(log)).await?;
    let stepped = match state {
        UpdateState::Rebooting => {
            async {
                let expected = version(run.expected.as_deref().unwrap_or_default())?;
                // a reboot sent just before the restart may still be shutting the device down
                let settled = (rebooting_since + REBOOT_SENT_SETTLES)
                    .saturating_duration_since(Instant::now());
                log.unless_cancelled(sleep(settled)).await?;
                if let Ok(Ok(device)) = timeout(Duration::from_secs(30), link.api()).await
                    && let Ok(now) = read_state(&device).await
                    && now.uptime + Duration::from_secs(5) >= rebooting_since.elapsed()
                {
                    return Err(Error::RebootNotSent);
                }
                let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
                let wait = (rebooting_since + run.wait).saturating_duration_since(Instant::now());
                // up for less than since entering `Rebooting`: booted since
                let state =
                    wait_until_back(link, log, Duration::ZERO, rebooting_since, wait, |state| {
                        (state.version == expected).then(|| state.clone())
                    })
                    .await?;
                drop(rebooting);
                if update_fallback::running(&link.api().await?)
                    .await
                    .as_deref()
                    == Some(&*run.partition)
                {
                    return Err(Error::UpdateRolledBack {
                        partition: run.partition.clone(),
                        version: state.version.to_string().into(),
                    });
                }
                log.emit(RouterOsRunning {
                    version: state.version.to_string().into(),
                });
                let wanted = &run.wanted;
                let partition = Some(&*run.partition);
                steps(
                    link,
                    power,
                    state,
                    &target,
                    &newest_v6,
                    wanted,
                    partition,
                    run.wait,
                    Some(run),
                    keep,
                    log,
                )
                .await
            }
            .await
        }
        UpdateState::NotUpdated => Err(Error::UpdateFailedBefore(
            run.failed.clone().unwrap_or_default(),
        )),
        _ => read_state(&link.api().await?).await,
    };
    let state = after_steps(link, stepped, Some(run), keep, &run.ospf, log).await?;
    firmware_and_done(link, power, state, run.from.clone(), log).await
}

/// After the steps (`stepped`): with the fallback set (`run`) its entry goes, or the update
/// failed and the fallback decides ([`not_updated`]); then the OSPF neighbors from before
/// (`ospf`) have to be back.
async fn after_steps(
    link: &impl DeviceLink,
    stepped: Result<DeviceState, Error>,
    run: Option<&UpdateRun>,
    keep: &(impl Fn(&UpdateRun) + Sync),
    ospf: &BTreeSet<Box<str>>,
    log: &Log,
) -> Result<DeviceState, Error> {
    let state = match (stepped, run) {
        (Ok(state), Some(run)) => {
            keep(&UpdateRun {
                expected: None,
                ..run.clone()
            });
            log.enter(UpdateState::StoppingFallback);
            update_fallback::disarm(&link.api().await?, &run.partition, log).await?;
            state
        }
        (Ok(state), None) => state,
        (Err(error), Some(run)) => {
            keep(&UpdateRun {
                expected: None,
                failed: Some(error.to_string().into()),
                ..run.clone()
            });
            log.enter(UpdateState::NotUpdated);
            return Err(not_updated(link, &run.partition, error, log).await);
        }
        (Err(error), None) => return Err(error),
    };
    log.enter(UpdateState::WaitingForNeighbors);
    let missing = update_fallback::wait_for_ospf(&link.api().await?, ospf, log).await?;
    if !missing.is_empty() {
        return Err(Error::OspfNeighborsMissing(missing.join(", ").into()));
    }
    if !ospf.is_empty() {
        log.emit(update_fallback::OspfNeighborsBack {
            neighbors: ospf.iter().cloned().collect(),
        });
    }
    Ok(state)
}

/// The RouterBOARD firmware where an upgrade is there, then `UpdateDone` from `from`.
async fn firmware_and_done(
    link: &impl DeviceLink,
    power: &PowerChain,
    state: DeviceState,
    from: Box<str>,
    log: &Log,
) -> Result<(), Error> {
    if let Some((current, upgrade)) = &state.firmware
        && current != upgrade
    {
        let upgrade_event = |running| FirmwareUpgrade {
            current: current.clone(),
            upgrade: upgrade.clone(),
            running,
        };
        log.check()?;
        log.enter(UpdateState::Firmware);
        log.emit(upgrade_event(false));
        // the firmware loads at the next boot, whenever that is
        act(&link.api().await?, &RouterboardUpgrade).await?;
        log.enter(UpdateState::WaitingToRebootFirmware);
        let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
        log.enter(UpdateState::RebootingFirmware);
        reboot_and_wait(
            link,
            rebooting,
            log,
            RebootReason::Firmware,
            REBOOT_TIMEOUT,
            |state| {
                state
                    .firmware
                    .as_ref()
                    .is_some_and(|(running, _)| running == upgrade)
                    .then_some(())
            },
        )
        .await?;
        log.emit(upgrade_event(true));
    }
    log.emit(UpdateDone {
        from,
        to: state.version.to_string().into(),
    });
    Ok(())
}

/// Downgrades RouterOS 7 to the older `target` (the device page's rescue after an update):
/// the packages the device runs, of `target`, uploaded like for an update, then
/// `/system/package/downgrade`, which reboots. Nothing is copied to another partition: that
/// one may hold the RouterOS to go back to. A device without two partitions needs a way back
/// as for an update ([`update_fallback::check_downgrade`]). The firmware stays as it is.
pub async fn downgrade(
    link: &impl DeviceLink,
    power: &PowerChain,
    target: &Version,
    way_back: &update_fallback::WayBack,
    log: &Log,
) -> Result<(), Error> {
    let state = read_state(&link.api().await?).await?;
    let from: Box<str> = state.version.to_string().into();
    log.emit(UpdateStart {
        version: from.clone(),
        architecture: architecture_name(state.architecture).into(),
        packages: state.packages.clone(),
    });
    let step = downgrade_step(&state, target)?;
    log.enter(UpdateState::WaitingForChain);
    let _updating = log.unless_cancelled(power.updating(log)).await?;
    log.enter(UpdateState::RememberingNeighbors);
    let device = link.api().await?;
    let ospf = update_fallback::ospf_up(&device).await?;
    update_fallback::check_downgrade(&device, way_back, log).await?;
    drop(device);
    log.emit(UpdateStepPlanned {
        version: step.version.to_string().into(),
        files: step.files.clone(),
        uninstall: step.uninstall.clone(),
    });
    let rebooting = put_packages(link, power, &step, log).await?;
    log.enter(UpdateState::Rebooting);
    let restarted = restart_and_wait(
        link,
        rebooting,
        log,
        RebootReason::Downgrade,
        REBOOT_TIMEOUT,
        &PackageDowngrade,
        |state| (state.version == *target).then(|| state.clone()),
    )
    .await;
    let state = match restarted {
        Ok(state) => state,
        Err(error @ Error::Refused(_)) => {
            // refused before the reboot: the packages must not install at some later boot
            if let Err(remove) = remove_files(&link.api().await?, &step.files).await {
                warn!("uploaded packages not removed: {remove}");
            }
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    log.emit(RouterOsRunning {
        version: state.version.to_string().into(),
    });
    log.enter(UpdateState::WaitingForNeighbors);
    let missing = update_fallback::wait_for_ospf(&link.api().await?, &ospf, log).await?;
    if !missing.is_empty() {
        return Err(Error::OspfNeighborsMissing(missing.join(", ").into()));
    }
    if !ospf.is_empty() {
        log.emit(update_fallback::OspfNeighborsBack {
            neighbors: ospf.into_iter().collect(),
        });
    }
    log.emit(Downgraded {
        from,
        to: state.version.to_string().into(),
    });
    Ok(())
}

/// The update steps from `state`; `armed`: the partition the device switches back to, where it
/// has one ([`update_fallback`]); `wait`: how long a reboot is waited for (with the fallback,
/// until the device would be back from it); `run` handed to `keep` with each step's version
/// before its reboot.
#[allow(clippy::too_many_arguments)]
async fn steps(
    link: &impl DeviceLink,
    power: &PowerChain,
    mut state: DeviceState,
    target: &Version,
    newest_v6: &Version,
    wanted: &[Box<str>],
    armed: Option<&str>,
    wait: Duration,
    run: Option<&UpdateRun>,
    keep: &(impl Fn(&UpdateRun) + Sync),
    log: &Log,
) -> Result<DeviceState, Error> {
    while let Some(step) = next_step(&state, target, newest_v6, wanted) {
        log.check()?;
        log.emit(UpdateStepPlanned {
            version: step.version.to_string().into(),
            files: step.files.clone(),
            uninstall: step.uninstall.clone(),
        });
        let rebooting = put_packages(link, power, &step, log).await?;
        if let Some(run) = run {
            keep(&UpdateRun {
                expected: Some(step.version.to_string().into()),
                ..run.clone()
            });
        }
        log.enter(UpdateState::Rebooting);
        uninstall(&link.api().await?, &step.uninstall).await?;
        let expected = step.version.clone();
        state = reboot_and_wait(
            link,
            rebooting,
            log,
            RebootReason::Packages,
            wait,
            |state| (state.version == expected).then(|| state.clone()),
        )
        .await?;
        // back from the fallback, with the version a package-only step expects too
        if let Some(partition) = armed
            && update_fallback::running(&link.api().await?)
                .await
                .as_deref()
                == Some(partition)
        {
            return Err(Error::UpdateRolledBack {
                partition: partition.into(),
                version: state.version.to_string().into(),
            });
        }
        log.emit(RouterOsRunning {
            version: state.version.to_string().into(),
        });
    }

    Ok(state)
}

/// Downloads the packages of `step` and uploads them; once all are on the device, waits for
/// the lease to reboot. Where it stops before, the uploaded packages are removed again: what is
/// uploaded installs at the next boot.
async fn put_packages(
    link: &impl DeviceLink,
    power: &PowerChain,
    step: &UpdateStep,
    log: &Log,
) -> Result<RebootLease, Error> {
    // all before the first upload: a part of the packages must not install
    log.enter(UpdateState::Downloading);
    let mut packages = Vec::new();
    for file in &step.files {
        let data = log
            .unless_cancelled(download(&step.version, file))
            .await??;
        log.emit(PackageDownloaded {
            file: file.clone(),
            bytes: (data.len() as u64).into(),
        });
        packages.push((file, data));
    }
    log.check()?;
    log.enter(UpdateState::Uploading);
    let relaxed = allow_password_login(&link.api().await?, Some(log)).await?;
    let uploaded = async {
        for &(file, ref data) in &packages {
            let total = (data.len() as u64).into();
            let progress = |sent: usize| {
                log.emit(PackageUploading {
                    file: file.clone(),
                    bytes: (sent as u64).into(),
                    total,
                })
            };
            link.upload(file, data, &progress).await?;
            // the size shows up while the file is still being written out (on 16 MB devices the
            // root directory is in RAM, there it settles within seconds)
            let device = link.api().await?;
            let expected = data.len().to_string();
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let size = collect_resource::<File>(&device)
                    .await?
                    .iter()
                    .find(|entry| *entry.cfg.name.0 == *file.as_bytes())
                    .and_then(|entry| entry.status.size.map(|size| size.to_string()));
                if size.as_deref() == Some(&expected) {
                    break;
                }
                if Instant::now() > deadline {
                    return Err(match size {
                        Some(size) => Error::UploadIncomplete {
                            file: file.clone(),
                            size: size.into(),
                            expected: data.len(),
                        },
                        None => Error::UploadMissing(file.clone()),
                    });
                }
                sleep(Duration::from_secs(2)).await;
            }
            log.emit(PackageUploaded { file: file.clone() });
        }
        Ok::<_, Error>(())
    }
    .await;
    restore_password_login(&link.api().await?, relaxed, Some(log)).await?;
    // what is uploaded installs at the next boot, also when the update stops before it
    let rebooting = match uploaded {
        Ok(()) => {
            log.enter(UpdateState::WaitingToReboot);
            log.unless_cancelled(power.rebooting(log)).await
        }
        Err(error) => Err(error),
    };
    match rebooting {
        Ok(rebooting) => Ok(rebooting),
        Err(error) => {
            if let Err(remove) = remove_files(&link.api().await?, &step.files).await {
                warn!("uploaded packages not removed: {remove}");
            }
            Err(error)
        }
    }
}

/// The update failed with `error` while the device can switch back to `partition`: back on it
/// is [`Error::UpdateRolledBack`]; where the device is reached on the new one, the scheduler goes,
/// so the device doesn't switch back by itself (the device page offers that). Unreached, the
/// scheduler stays and does its job.
async fn not_updated(link: &impl DeviceLink, partition: &str, error: Error, log: &Log) -> Error {
    // the fallback stays: not reached, back on it, or the packages installing at the next boot
    if matches!(
        error,
        Error::NotBackAfterReboot | Error::RebootNotSent | Error::UpdateRolledBack { .. }
    ) {
        return error;
    }
    let device = match timeout(Duration::from_secs(30), link.api()).await {
        Ok(Ok(device)) => device,
        _ => return error,
    };
    if update_fallback::running(&device).await.as_deref() == Some(partition) {
        let version = read_state(&device)
            .await
            .map(|state| state.version.to_string());
        return Error::UpdateRolledBack {
            partition: partition.into(),
            version: version.unwrap_or_default().into(),
        };
    }
    if let Err(disarm) = update_fallback::disarm(&device, partition, log).await {
        warn!("update fallback not removed: {disarm}");
    }
    error
}

#[cfg(test)]
mod tests {
    use super::{Architecture, DeviceState, UpdateStep, Version, downgrade_step, next_step};
    use crate::Error;
    use std::time::Duration;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }
    fn state(version: &str, arch: Architecture, packages: &[&str]) -> DeviceState {
        DeviceState {
            version: v(version),
            architecture: arch,
            packages: names(packages),
            firmware: None,
            model: None,
            uptime: Duration::ZERO,
        }
    }

    fn names(names: &[&str]) -> Box<[Box<str>]> {
        names.iter().map(|&name| name.into()).collect()
    }

    #[test]
    fn parses_and_orders_versions() {
        assert_eq!(v("7.15.3 (stable)").to_string(), "7.15.3");
        assert_eq!(v("7.25rc1").to_string(), "7.25rc1");
        assert_eq!(v("7.26beta1 1790861387").to_string(), "7.26beta1");
        assert!(v("7.24.5") < v("7.25beta2"));
        assert!(v("7.25beta2") < v("7.25rc1"));
        assert!(v("7.25rc1") < v("7.25"));
        assert!(Version::parse("7.x").is_none());
        assert!(v("7.9") < v("7.15.3"));
        assert_eq!(v("7.20"), v("7.20.0"));
        assert!(Version::parse("").is_none());
    }

    #[test]
    fn plans_the_steps() {
        let target = v("7.20.8");
        let v6 = v("6.49.19");
        let wireless = names(&["wireless"]);
        assert_eq!(
            next_step(
                &state("7.15.3", Architecture::Arm, &["routeros", "wireless"]),
                &target,
                &v6,
                &wireless
            ),
            Some(UpdateStep {
                version: target.clone(),
                files: names(&["routeros-7.20.8-arm.npk", "wireless-7.20.8-arm.npk"]),
                uninstall: names(&[]),
            })
        );
        assert_eq!(
            next_step(
                &state("7.24.5", Architecture::X8664, &["routeros"]),
                &v("7.25"),
                &v6,
                &[]
            )
            .unwrap()
            .files,
            names(&["routeros-7.25.npk"])
        );
        assert_eq!(
            next_step(
                &state("7.20.8", Architecture::Arm, &["routeros"]),
                &target,
                &v6,
                &[]
            ),
            None
        );
        // newer than the channel: no downgrade
        assert_eq!(
            next_step(
                &state("7.21", Architecture::Arm, &["routeros"]),
                &target,
                &v6,
                &[]
            ),
            None
        );
        let sxt = ["system", "wireless", "routing", "dhcp", "security"];
        assert_eq!(
            next_step(
                &state("6.45.1", Architecture::Mipsbe, &sxt),
                &target,
                &v6,
                &wireless
            )
            .unwrap()
            .files,
            names(&["routeros-mipsbe-6.49.19.npk"])
        );
        assert_eq!(
            next_step(
                &state("6.49.19", Architecture::Mipsbe, &sxt),
                &target,
                &v6,
                &wireless
            )
            .unwrap()
            .files,
            names(&["routeros-7.20.8-mipsbe.npk", "wireless-7.20.8-mipsbe.npk"])
        );
    }

    #[test]
    fn brings_the_packages_in_line() {
        let target = v("7.20.8");
        let v6 = v("6.49.19");
        let wifi = names(&["wifi-qcom-ac"]);
        // an unused radio package goes with the update
        assert_eq!(
            next_step(
                &state("7.18.2", Architecture::Arm, &["routeros", "wifi-qcom-ac"]),
                &target,
                &v6,
                &[]
            ),
            Some(UpdateStep {
                version: target.clone(),
                files: names(&["routeros-7.20.8-arm.npk"]),
                uninstall: names(&["wifi-qcom-ac"]),
            })
        );
        // at the version already: only the missing package, and what isn't needed goes
        assert_eq!(
            next_step(
                &state("7.20.8", Architecture::Arm, &["routeros", "wireless"]),
                &target,
                &v6,
                &wifi
            ),
            Some(UpdateStep {
                version: target.clone(),
                files: names(&["wifi-qcom-ac-7.20.8-arm.npk"]),
                uninstall: names(&["wireless"]),
            })
        );
        assert_eq!(
            next_step(
                &state("7.20.8", Architecture::Arm, &["routeros", "wifi-qcom-ac"]),
                &target,
                &v6,
                &wifi
            ),
            None
        );
    }

    #[test]
    fn a_downgrade_takes_the_packages_the_device_runs() {
        assert_eq!(
            downgrade_step(
                &state("7.25", Architecture::Arm, &["routeros", "wifi-qcom-ac"]),
                &v("7.23.8")
            )
            .unwrap(),
            UpdateStep {
                version: v("7.23.8"),
                files: names(&["routeros-7.23.8-arm.npk", "wifi-qcom-ac-7.23.8-arm.npk"]),
                uninstall: names(&[]),
            }
        );
        assert_eq!(
            downgrade_step(
                &state("7.24.5", Architecture::X8664, &["routeros"]),
                &v("7.23.8")
            )
            .unwrap()
            .files,
            names(&["routeros-7.23.8.npk"])
        );
        // only to an older RouterOS 7
        for (running, target) in [
            ("7.23.8", "7.23.8"),
            ("7.23.8", "7.25"),
            ("7.20.8", "6.49.19"),
            ("6.49.18", "6.48"),
        ] {
            assert!(
                matches!(
                    downgrade_step(
                        &state(running, Architecture::Arm, &["routeros"]),
                        &v(target)
                    ),
                    Err(Error::NoDowngrade { .. })
                ),
                "{running} to {target}"
            );
        }
    }
}
