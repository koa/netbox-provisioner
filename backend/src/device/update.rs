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
    device_state::UpdateState,
    device::{
        power::{PowerChain, RebootLease},
        raw::{command, field, rows},
        update_fallback,
    },
    graphql::scalars::ScalarDuration,
    job::Log,
};
use async_graphql::{Enum, SimpleObject};
use log::warn;
use serde::Deserialize;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::value::{ParseRosValueResult, RosValue};
use std::{cmp::Ordering, time::Duration};
use tokio::time::{Instant, sleep, timeout};
use async_graphql::futures_util::StreamExt;

const DOWNLOAD_URL: &str = "https://download.mikrotik.com/routeros";
/// The newest release of a channel, as `<version> <timestamp>`.
const NEWEST_URL: &str = "https://upgrade.mikrotik.com/routeros/NEWEST";
/// Booting a package update takes a few minutes on small devices.
pub const REBOOT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// What the update needs of a device; implemented by
/// [`AccessibleDevice`](crate::device::AccessibleDevice) (through the management proxy) and by
/// test setups.
#[allow(async_fn_in_trait)]
pub trait DeviceLink {
    /// A fresh API connection (the device reboots in between).
    async fn api(&self) -> Result<RawDevice<SimpleResult>, Error>;
    /// Copies a file to the device's root directory.
    /// `progress` gets the bytes sent now and then ([`terminal::upload`](super::terminal::upload)).
    async fn upload(
        &self,
        name: &str,
        data: &[u8],
        progress: &(dyn Fn(usize) + Sync),
    ) -> Result<(), Error>;
}

/// `bytes` of the package `file` (`total` bytes) are on their way to the device.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageUploading {
    pub file: Box<str>,
    pub bytes: u64,
    pub total: u64,
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

/// `/ip ssh` switched for the upload (the user has SSH keys, see [`allow_password_login`]), or
/// back afterwards (`restored`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SshPasswordLogin {
    pub setting: Box<str>,
    pub value: Box<str>,
    pub restored: bool,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageDownloaded {
    pub file: Box<str>,
    pub bytes: u64,
}

/// The package is on the device, in full.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PackageUploaded {
    pub file: Box<str>,
}

/// The device reboots; the job waits until it is back (up to [`REBOOT_TIMEOUT`]).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Reboot {
    pub reason: RebootReason,
    /// how long the job waits for it
    pub timeout: ScalarDuration,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RebootReason {
    /// to install the uploaded packages
    Packages,
    /// to load the upgraded RouterBOARD firmware
    Firmware,
    /// asked for (`reboot`)
    Requested,
    /// a cold boot (power cut or button) confirming a device-mode change
    DeviceMode,
    /// the flash is reformatted in partitions
    Repartition,
    /// to boot from another partition (`bootPartition`)
    Partition,
    /// `/system/package/downgrade` installs the uploaded older packages
    Downgrade,
    /// the apply fallback left on the device loads the config from before, asked for
    ApplyFallback,
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
    /// `/system/resource architecture-name`, e.g. `arm`, `mipsbe`, `x86_64`
    pub architecture: Box<str>,
    /// names of the installed packages that aren't disabled
    pub packages: Box<[Box<str>]>,
    /// `(current, upgrade)` RouterBOARD firmware; `None` without RouterBOARD (CHR)
    pub firmware: Option<(Box<str>, Box<str>)>,
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
    let arch = state.architecture.as_ref();
    if state.version.major() < 7 {
        if state.version < *newest_v6 {
            // the bundle contains every RouterOS 6 package
            let arch = if arch.starts_with("x86") { "x86" } else { arch };
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

/// The packages of `target` replacing those the device runs, for a downgrade: RouterOS 7 to an
/// older RouterOS 7, the same packages (a package `target` lacks fails its download, before
/// anything is uploaded).
pub fn downgrade_step(state: &DeviceState, target: &Version) -> Result<UpdateStep, Error> {
    if state.version.major() < 7 || target.major() < 7 || *target >= state.version {
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
    Ok(v7_step(&others, &state.architecture, target, [].into()))
}

fn v7_step(wanted: &[Box<str>], arch: &str, version: &Version, uninstall: Box<[Box<str>]>) -> UpdateStep {
    // x86 packages carry no architecture
    let suffix = if arch.starts_with("x86") {
        String::new()
    } else {
        format!("-{arch}")
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
/// Returns the setting and the value to put back afterwards.
async fn allow_password_login(
    device: &RawDevice<SimpleResult>,
    log: &Log,
) -> Result<Option<(&'static str, &'static str)>, Error> {
    let ssh = rows(device, "/ip/ssh/print").await?;
    let Some(ssh) = ssh.first() else {
        return Ok(None);
    };
    let (setting, on, off) = if field(ssh, "always-allow-password-login") == Some("false") {
        ("always-allow-password-login", "yes", "no")
    } else if field(ssh, "password-authentication") == Some("yes-if-no-key") {
        ("password-authentication", "yes", "yes-if-no-key")
    } else {
        return Ok(None);
    };
    if rows(device, "/user/ssh-keys/print").await?.is_empty() {
        return Ok(None);
    }
    set_ssh(device, setting, on).await?;
    log.emit(SshPasswordLogin {
        setting: setting.into(),
        value: on.into(),
        restored: false,
    });
    Ok(Some((setting, off)))
}

async fn set_ssh(device: &RawDevice<SimpleResult>, setting: &str, value: &str) -> Result<(), Error> {
    command(device, "/ip/ssh/set", &[(setting, value)]).await?;
    Ok(())
}

/// Removes `files` from the device's root directory, where they are.
async fn remove_files(device: &RawDevice<SimpleResult>, files: &[Box<str>]) -> Result<(), Error> {
    for row in rows(device, "/file/print").await? {
        let (Some(name), Some(id)) = (field(&row, "name"), field(&row, ".id")) else {
            continue;
        };
        if !files.iter().any(|file| file.as_ref() == name) {
            continue;
        }
        command(device, "/file/remove", &[(".id", id)]).await?;
    }
    Ok(())
}

/// Marks the packages for removal at the next reboot.
async fn uninstall(device: &RawDevice<SimpleResult>, packages: &[Box<str>]) -> Result<(), Error> {
    for row in rows(device, "/system/package/print").await? {
        let (Some(name), Some(id)) = (field(&row, "name"), field(&row, ".id")) else {
            continue;
        };
        if !packages.iter().any(|p| p.as_ref() == name) {
            continue;
        }
        command(device, "/system/package/uninstall", &[("numbers", id)]).await?;
    }
    Ok(())
}

pub async fn read_state(device: &RawDevice<SimpleResult>) -> Result<DeviceState, Error> {
    let resource = rows(device, "/system/resource/print").await?;
    let resource = resource
        .first()
        .ok_or(Error::MissingDeviceState("/system/resource"))?;
    let version = field(resource, "version")
        .and_then(Version::parse)
        .ok_or(Error::MissingDeviceState("version"))?;
    let architecture = field(resource, "architecture-name").unwrap_or_default().into();
    let uptime = field(resource, "uptime")
        .and_then(|uptime| match Duration::parse_ros(uptime.as_bytes()) {
            ParseRosValueResult::Value(uptime) => Some(uptime),
            _ => None,
        })
        .ok_or(Error::MissingDeviceState("uptime"))?;
    let packages = rows(device, "/system/package/print")
        .await?
        .iter()
        .filter(|row| field(row, "disabled") != Some("true"))
        .filter_map(|row| field(row, "name").map(Box::from))
        .collect();
    // CHR has no RouterBOARD; the command fails there
    let firmware = rows(device, "/system/routerboard/print")
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| {
            Some((
                field(&row, "current-firmware")?.into(),
                field(&row, "upgrade-firmware")?.into(),
            ))
        });
    Ok(DeviceState {
        version,
        architecture,
        packages,
        firmware,
        uptime,
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

async fn download(version: &Version, file: &str) -> Result<Vec<u8>, Error> {
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
        .to_vec())
}

/// Reboots and waits (up to `wait`) until the device answers with `expect` (a version, a
/// firmware); the lease is held until the device is back.
async fn reboot_and_wait<T, F>(
    link: &impl DeviceLink,
    rebooting: RebootLease,
    log: &Log,
    reason: RebootReason,
    wait: Duration,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    restart_and_wait(link, rebooting, log, reason, wait, "/system/reboot", check).await
}

/// [`reboot_and_wait`] with `path` as the command that reboots (`/system/package/downgrade`).
async fn restart_and_wait<T, F>(
    link: &impl DeviceLink,
    _rebooting: RebootLease,
    log: &Log,
    reason: RebootReason,
    wait: Duration,
    path: &str,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    let device = link.api().await?;
    let before = read_state(&device).await?.uptime;
    let sent = Instant::now();
    let mut answer = device.send_simple_command(path.as_bytes(), ()).await;
    // the answer, or the connection breaking with the reboot
    if let Ok(Some(SimpleResult::Trap { message, .. })) = timeout(Duration::from_secs(10), answer.next()).await {
        return Err(Error::Refused(format!("{path}: {message}").into()));
    }
    drop(answer);
    drop(device);
    log.emit(Reboot {
        reason,
        timeout: wait.into(),
    });
    wait_until_back(link, log, before, sent, wait, check).await
}

/// Waits (up to `wait`, usually [`REBOOT_TIMEOUT`]) until the device answers again after a
/// boot that came after `sent`, when it had been up for `before`, and checks it with `check`.
pub(crate) async fn wait_until_back<T, F>(
    link: &impl DeviceLink,
    log: &Log,
    before: Duration,
    sent: Instant,
    wait: Duration,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    // A connection while the device boots may just wait for it (it does through QEMU's port
    // forwarding), so the reboot shows in the uptime, not in a failing connection.
    let deadline = Instant::now() + wait;
    loop {
        log.unless_cancelled(sleep(Duration::from_secs(5))).await?;
        let state = match timeout(Duration::from_secs(30), link.api()).await {
            Ok(Ok(device)) => read_state(&device).await.ok(),
            _ => None,
        };
        if let Some(state) = state
            && state.uptime < before + sent.elapsed().saturating_sub(Duration::from_secs(5))
        {
            if let Some(result) = check(&state) {
                return Ok(result);
            }
            return Err(Error::UnexpectedAfterReboot {
                version: state.version.to_string().into(),
                firmware: state.firmware.map(|(running, _)| running),
            });
        }
        if Instant::now() > deadline {
            return Err(Error::NotBackAfterReboot);
        }
    }
}

/// Reboots the device and waits until it is back, as an update does (`power` as for
/// [`update`]: not while a device it powers updates).
pub async fn reboot(link: &impl DeviceLink, power: &PowerChain, log: &Log) -> Result<(), Error> {
    let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
    let state = reboot_and_wait(link, rebooting, log, RebootReason::Requested, REBOOT_TIMEOUT, |state| {
        Some(state.clone())
    })
    .await?;
    log.emit(RouterOsRunning {
        version: state.version.to_string().into(),
    });
    Ok(())
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
    let mut state = read_state(&link.api().await?).await?;
    let from: Box<str> = state.version.to_string().into();
    log.emit(UpdateStart {
        version: from.clone(),
        architecture: state.architecture.clone(),
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
        let stepped = steps(link, power, state, target, newest_v6, wanted, armed.as_deref(), wait, log).await;
        state = match (stepped, armed) {
            (Ok(state), Some(partition)) => {
                log.enter(UpdateState::StoppingFallback);
                update_fallback::disarm(&link.api().await?, &partition, log).await?;
                state
            }
            (Ok(state), None) => state,
            (Err(error), Some(partition)) => {
                log.enter(UpdateState::NotUpdated);
                return Err(not_updated(link, &partition, error, log).await);
            }
            (Err(error), None) => return Err(error),
        };
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
    }
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
        rows(&link.api().await?, "/system/routerboard/upgrade").await?;
        log.enter(UpdateState::WaitingToRebootFirmware);
        let rebooting = log.unless_cancelled(power.rebooting(log)).await?;
        log.enter(UpdateState::RebootingFirmware);
        reboot_and_wait(link, rebooting, log, RebootReason::Firmware, REBOOT_TIMEOUT, |state| {
            state
                .firmware
                .as_ref()
                .is_some_and(|(running, _)| running == upgrade)
                .then_some(())
        })
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
        architecture: state.architecture.clone(),
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
    let path = "/system/package/downgrade";
    let restarted = restart_and_wait(link, rebooting, log, RebootReason::Downgrade, REBOOT_TIMEOUT, path, |state| {
        (state.version == *target).then(|| state.clone())
    })
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
/// until the device would be back from it).
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
        log.enter(UpdateState::Rebooting);
        uninstall(&link.api().await?, &step.uninstall).await?;
        let expected = step.version.clone();
        state = reboot_and_wait(link, rebooting, log, RebootReason::Packages, wait, |state| {
            (state.version == expected).then(|| state.clone())
        })
        .await?;
        // back from the fallback, with the version a package-only step expects too
        if let Some(partition) = armed
            && update_fallback::running(&link.api().await?).await.as_deref() == Some(partition)
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
        let data = log.unless_cancelled(download(&step.version, file)).await??;
        log.emit(PackageDownloaded {
            file: file.clone(),
            bytes: data.len() as u64,
        });
        packages.push((file, data));
    }
    log.check()?;
    log.enter(UpdateState::Uploading);
    let relaxed = allow_password_login(&link.api().await?, log).await?;
    let uploaded = async {
        for &(file, ref data) in &packages {
            let total = data.len() as u64;
            let progress = |sent: usize| {
                log.emit(PackageUploading {
                    file: file.clone(),
                    bytes: sent as u64,
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
                let size = rows(&device, "/file/print")
                    .await?
                    .iter()
                    .find(|row| field(row, "name") == Some(file))
                    .and_then(|row| field(row, "size").map(String::from));
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
    if let Some((setting, value)) = relaxed {
        set_ssh(&link.api().await?, setting, value).await?;
        log.emit(SshPasswordLogin {
            setting: setting.into(),
            value: value.into(),
            restored: true,
        });
    }
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
    if matches!(error, Error::NotBackAfterReboot | Error::UpdateRolledBack { .. }) {
        return error;
    }
    let device = match timeout(Duration::from_secs(30), link.api()).await {
        Ok(Ok(device)) => device,
        _ => return error,
    };
    if update_fallback::running(&device).await.as_deref() == Some(partition) {
        let version = read_state(&device).await.map(|state| state.version.to_string());
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
    use super::{DeviceState, UpdateStep, Version, downgrade_step, next_step};
    use crate::Error;
    use std::time::Duration;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }
    fn state(version: &str, arch: &str, packages: &[&str]) -> DeviceState {
        DeviceState {
            version: v(version),
            architecture: arch.into(),
            packages: names(packages),
            firmware: None,
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
            next_step(&state("7.15.3", "arm", &["routeros", "wireless"]), &target, &v6, &wireless),
            Some(UpdateStep {
                version: target.clone(),
                files: names(&["routeros-7.20.8-arm.npk", "wireless-7.20.8-arm.npk"]),
                uninstall: names(&[]),
            })
        );
        assert_eq!(
            next_step(&state("7.24.5", "x86_64", &["routeros"]), &v("7.25"), &v6, &[])
                .unwrap()
                .files,
            names(&["routeros-7.25.npk"])
        );
        assert_eq!(next_step(&state("7.20.8", "arm", &["routeros"]), &target, &v6, &[]), None);
        // newer than the channel: no downgrade
        assert_eq!(next_step(&state("7.21", "arm", &["routeros"]), &target, &v6, &[]), None);
        let sxt = ["system", "wireless", "routing", "dhcp", "security"];
        assert_eq!(
            next_step(&state("6.45.1", "mipsbe", &sxt), &target, &v6, &wireless)
                .unwrap()
                .files,
            names(&["routeros-mipsbe-6.49.19.npk"])
        );
        assert_eq!(
            next_step(&state("6.49.19", "mipsbe", &sxt), &target, &v6, &wireless)
                .unwrap()
                .files,
            names(&[
                "routeros-7.20.8-mipsbe.npk",
                "wireless-7.20.8-mipsbe.npk"
            ])
        );
    }

    #[test]
    fn brings_the_packages_in_line() {
        let target = v("7.20.8");
        let v6 = v("6.49.19");
        let wifi = names(&["wifi-qcom-ac"]);
        // an unused radio package goes with the update
        assert_eq!(
            next_step(&state("7.18.2", "arm", &["routeros", "wifi-qcom-ac"]), &target, &v6, &[]),
            Some(UpdateStep {
                version: target.clone(),
                files: names(&["routeros-7.20.8-arm.npk"]),
                uninstall: names(&["wifi-qcom-ac"]),
            })
        );
        // at the version already: only the missing package, and what isn't needed goes
        assert_eq!(
            next_step(&state("7.20.8", "arm", &["routeros", "wireless"]), &target, &v6, &wifi),
            Some(UpdateStep {
                version: target.clone(),
                files: names(&["wifi-qcom-ac-7.20.8-arm.npk"]),
                uninstall: names(&["wireless"]),
            })
        );
        assert_eq!(
            next_step(&state("7.20.8", "arm", &["routeros", "wifi-qcom-ac"]), &target, &v6, &wifi),
            None
        );
    }

    #[test]
    fn a_downgrade_takes_the_packages_the_device_runs() {
        assert_eq!(
            downgrade_step(&state("7.25", "arm", &["routeros", "wifi-qcom-ac"]), &v("7.23.8")).unwrap(),
            UpdateStep {
                version: v("7.23.8"),
                files: names(&["routeros-7.23.8-arm.npk", "wifi-qcom-ac-7.23.8-arm.npk"]),
                uninstall: names(&[]),
            }
        );
        assert_eq!(
            downgrade_step(&state("7.24.5", "x86_64", &["routeros"]), &v("7.23.8")).unwrap().files,
            names(&["routeros-7.23.8.npk"])
        );
        // only to an older RouterOS 7
        for (running, target) in [("7.23.8", "7.23.8"), ("7.23.8", "7.25"), ("7.20.8", "6.49.19"), ("6.49.18", "6.48")] {
            assert!(
                matches!(downgrade_step(&state(running, "arm", &["routeros"]), &v(target)), Err(Error::NoDowngrade { .. })),
                "{running} to {target}"
            );
        }
    }
}
