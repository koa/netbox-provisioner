//! RouterOS updates pushed by the provisioner: it downloads the packages from MikroTik, copies
//! them onto the device over SCP and reboots it, so the device itself needs no route to the
//! internet. The target is the newest release of a channel (long-term, stable, testing,
//! development). RouterOS 6 goes to the last 6.49 first (the bundle package), from there to
//! the target RouterOS 7. RouterOS 7 gets `routeros` and the packages the device needs
//! (`device::ros::needed_radio_package`); other packages are uninstalled. After the system the
//! RouterBOARD firmware follows (`/system/routerboard/upgrade`, one more reboot). A device that
//! powers others over PoE reboots only while none of them updates ([`power`](super::power)).
use crate::{Error, device::power::PowerChain};
use async_graphql::{Enum, SimpleObject};
use serde::Deserialize;
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{cmp::Ordering, collections::HashMap, fmt::Write, time::Duration};
use tokio::time::{Instant, sleep, timeout};
use async_graphql::futures_util::StreamExt;

const DOWNLOAD_URL: &str = "https://download.mikrotik.com/routeros";
/// The newest release of a channel, as `<version> <timestamp>`.
const NEWEST_URL: &str = "https://upgrade.mikrotik.com/routeros/NEWEST";
/// Booting a package update takes a few minutes on small devices.
const REBOOT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// What the update needs of a device; implemented by
/// [`AccessibleDevice`](crate::device::AccessibleDevice) (through the management proxy) and by
/// test setups.
#[allow(async_fn_in_trait)]
pub trait DeviceLink {
    /// A fresh API connection (the device reboots in between).
    async fn api(&self) -> Result<RawDevice<SimpleResult>, Error>;
    /// Copies a file to the device's root directory.
    async fn upload(&self, name: &str, data: &[u8]) -> Result<(), Error>;
}

#[derive(Debug, SimpleObject)]
pub struct UpdateResult {
    /// RouterOS version before
    pub from: String,
    /// RouterOS version after, as far as it got
    pub to: String,
    pub error: Option<String>,
    pub log: String,
}

/// A RouterOS version like `7.20.8` or a pre-release like `7.25rc1`, `7.26beta1`; a channel
/// suffix like ` (stable)` is ignored. A pre-release comes before its release, beta before rc.
#[derive(Debug, Clone)]
pub struct Version {
    numbers: Vec<u32>,
    /// `("beta", 1)`, `("rc", 1)`; `None` for a release
    pre: Option<(String, u32)>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.split_whitespace().next()?;
        let (release, pre) = match text.find(|c: char| c.is_ascii_alphabetic()) {
            Some(at) => {
                let (release, tag) = text.split_at(at);
                let digits = tag.find(|c: char| c.is_ascii_digit()).unwrap_or(tag.len());
                let (kind, number) = tag.split_at(digits);
                (release, Some((kind.to_string(), number.parse().unwrap_or(0))))
            }
            None => (text, None),
        };
        let numbers = release
            .split('.')
            .map(|part| part.parse().ok())
            .collect::<Option<Vec<u32>>>()?;
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
                let rank = match kind.as_str() {
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
    pub architecture: String,
    /// names of the installed packages that aren't disabled
    pub packages: Vec<String>,
    /// `(current, upgrade)` RouterBOARD firmware; `None` without RouterBOARD (CHR)
    pub firmware: Option<(String, String)>,
    pub uptime: Duration,
}

/// A RouterOS release channel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    LongTerm,
    #[default]
    Stable,
    Testing,
    Development,
}

impl Channel {
    fn name(self) -> &'static str {
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
    let failed = |e: reqwest::Error| Error::Update(format!("{url}: {e}").into());
    let text = reqwest::get(&url)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?
        .text()
        .await
        .map_err(failed)?;
    Version::parse(&text).ok_or_else(|| Error::Update(format!("{url}: {text}").into()))
}

/// One update with a reboot: the version, the package files to install and the packages to
/// uninstall.
#[derive(Debug, PartialEq, Eq)]
pub struct UpdateStep {
    pub version: Version,
    pub files: Vec<String>,
    pub uninstall: Vec<String>,
}

/// The next step from `state` towards `target` with `routeros` and the `wanted` packages;
/// `None` when there. A device newer than `target` stays at its version, it only gets its
/// packages in line.
pub fn next_step(
    state: &DeviceState,
    target: &Version,
    newest_v6: &Version,
    wanted: &[String],
) -> Option<UpdateStep> {
    let arch = state.architecture.as_str();
    if state.version.major() < 7 {
        if state.version < *newest_v6 {
            // the bundle contains every RouterOS 6 package
            let arch = if arch.starts_with("x86") { "x86" } else { arch };
            return Some(UpdateStep {
                version: newest_v6.clone(),
                files: vec![format!("routeros-{arch}-{newest_v6}.npk")],
                uninstall: Vec::new(),
            });
        }
        // RouterOS 7 replaces the whole RouterOS 6 package set
        return Some(v7_step(wanted, arch, target, Vec::new()));
    }
    let unwanted: Vec<String> = state
        .packages
        .iter()
        .filter(|p| *p != "routeros" && !wanted.contains(p))
        .cloned()
        .collect();
    if state.version < *target {
        return Some(v7_step(wanted, arch, target, unwanted));
    }
    let missing: Vec<String> = wanted
        .iter()
        .filter(|p| !state.packages.contains(p))
        .cloned()
        .collect();
    if missing.is_empty() && unwanted.is_empty() {
        return None;
    }
    let mut step = v7_step(&missing, arch, &state.version, unwanted);
    step.files.retain(|file| !file.starts_with("routeros-"));
    Some(step)
}

fn v7_step(wanted: &[String], arch: &str, version: &Version, uninstall: Vec<String>) -> UpdateStep {
    // x86 packages carry no architecture
    let suffix = if arch.starts_with("x86") {
        String::new()
    } else {
        format!("-{arch}")
    };
    UpdateStep {
        version: version.clone(),
        files: ["routeros".to_string()]
            .iter()
            .chain(wanted)
            .map(|p| format!("{p}-{version}{suffix}.npk"))
            .collect(),
        uninstall,
    }
}

async fn rows(device: &RawDevice<SimpleResult>, command: &[u8]) -> Result<Vec<HashMap<Box<str>, Option<Box<str>>>>, Error> {
    let mut stream = device.send_simple_command(command, ()).await;
    let mut rows = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(row) => rows.push(row),
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { message, .. } => {
                return Err(Error::Update(
                    format!("{}: {message}", String::from_utf8_lossy(command)).into(),
                ));
            }
        }
    }
    Ok(rows)
}

fn field<'r>(row: &'r HashMap<Box<str>, Option<Box<str>>>, name: &str) -> Option<&'r str> {
    row.get(name).and_then(|v| v.as_deref())
}

/// SCP logs in with the password, which RouterOS refuses over SSH for a user with SSH keys
/// unless `always-allow-password-login` is on (up to 7.20) or `password-authentication` is
/// `yes` rather than `yes-if-no-key` (7.24): switched for the upload when keys are there.
/// Returns the setting and the value to put back afterwards.
async fn allow_password_login(
    device: &RawDevice<SimpleResult>,
    log: &mut String,
) -> Result<Option<(&'static str, &'static str)>, Error> {
    let ssh = rows(device, b"/ip/ssh/print").await?;
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
    if rows(device, b"/user/ssh-keys/print").await?.is_empty() {
        return Ok(None);
    }
    set_ssh(device, setting, on).await?;
    let _ = writeln!(log, "ssh {setting}={on} for the upload (the user has ssh keys)");
    Ok(Some((setting, off)))
}

async fn set_ssh(device: &RawDevice<SimpleResult>, setting: &str, value: &str) -> Result<(), Error> {
    let mut stream = device
        .send_command(
            b"/ip/ssh/set".as_slice(),
            |cmd| cmd.attribute(setting.as_bytes(), value.as_bytes()),
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(_) => {}
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { message, .. } => return Err(Error::Update(message)),
        }
    }
    Ok(())
}

/// Marks the packages for removal at the next reboot.
async fn uninstall(device: &RawDevice<SimpleResult>, packages: &[String]) -> Result<(), Error> {
    for row in rows(device, b"/system/package/print").await? {
        let (Some(name), Some(id)) = (field(&row, "name"), field(&row, ".id")) else {
            continue;
        };
        if !packages.iter().any(|p| p == name) {
            continue;
        }
        let mut stream = device
            .send_command(
                b"/system/package/uninstall".as_slice(),
                |cmd| cmd.attribute(b"numbers".as_slice(), id.as_bytes()),
                (),
            )
            .await;
        while let Some(result) = stream.next().await {
            match result {
                SimpleResult::Sentence(_) => {}
                SimpleResult::Error(error) => return Err(error.into()),
                SimpleResult::Trap { message, .. } => {
                    return Err(Error::Update(format!("uninstall {name}: {message}").into()));
                }
            }
        }
    }
    Ok(())
}

pub async fn read_state(device: &RawDevice<SimpleResult>) -> Result<DeviceState, Error> {
    let resource = rows(device, b"/system/resource/print").await?;
    let resource = resource
        .first()
        .ok_or_else(|| Error::Update("no /system/resource".into()))?;
    let version = field(resource, "version")
        .and_then(Version::parse)
        .ok_or_else(|| Error::Update("no RouterOS version".into()))?;
    let architecture = field(resource, "architecture-name")
        .unwrap_or_default()
        .to_string();
    let uptime = field(resource, "uptime")
        .and_then(parse_uptime)
        .ok_or_else(|| Error::Update("no uptime".into()))?;
    let packages = rows(device, b"/system/package/print")
        .await?
        .iter()
        .filter(|row| field(row, "disabled") != Some("true"))
        .filter_map(|row| field(row, "name").map(String::from))
        .collect();
    // CHR has no RouterBOARD; the command fails there
    let firmware = rows(device, b"/system/routerboard/print")
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| {
            Some((
                field(&row, "current-firmware")?.to_string(),
                field(&row, "upgrade-firmware")?.to_string(),
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

async fn download(version: &Version, file: &str) -> Result<Vec<u8>, Error> {
    let url = format!("{DOWNLOAD_URL}/{version}/{file}");
    let failed = |e: reqwest::Error| Error::Update(format!("{url}: {e}").into());
    Ok(reqwest::get(&url)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?
        .bytes()
        .await
        .map_err(failed)?
        .to_vec())
}

/// Reboots and waits until the device answers with `expect` (a version, a firmware).
async fn reboot_and_wait<T, F>(
    link: &impl DeviceLink,
    power: &PowerChain,
    log: &mut String,
    check: F,
) -> Result<T, Error>
where
    F: Fn(&DeviceState) -> Option<T>,
{
    // held until the device is back
    let _rebooting = power.rebooting(log).await;
    let device = link.api().await?;
    let before = read_state(&device).await?.uptime;
    let sent = Instant::now();
    let mut answer = device.send_simple_command(b"/system/reboot".as_slice(), ()).await;
    // the answer, or the connection breaking with the reboot
    let _ = timeout(Duration::from_secs(10), answer.next()).await;
    drop(device);
    let _ = writeln!(log, "reboot");
    // A connection while the device boots may just wait for it (it does through QEMU's port
    // forwarding), so the reboot shows in the uptime, not in a failing connection.
    let deadline = Instant::now() + REBOOT_TIMEOUT;
    loop {
        sleep(Duration::from_secs(5)).await;
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
            return Err(Error::Update(
                format!(
                    "back after the reboot with RouterOS {}, firmware {:?}",
                    state.version, state.firmware
                )
                .into(),
            ));
        }
        if Instant::now() > deadline {
            return Err(Error::Update("not back after the reboot".into()));
        }
    }
}

/// A RouterOS duration like `4d13h54m28s` (also `1w2d`, `500ms`).
fn parse_uptime(text: &str) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut number = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().ok()?;
        number.clear();
        total += match c {
            'w' => Duration::from_secs(n * 7 * 86400),
            'd' => Duration::from_secs(n * 86400),
            'h' => Duration::from_secs(n * 3600),
            'm' if chars.next_if_eq(&'s').is_some() => Duration::from_millis(n),
            'm' => Duration::from_secs(n * 60),
            's' => Duration::from_secs(n),
            _ => return None,
        };
    }
    number.is_empty().then_some(total)
}

/// Updates the device to `target`, step by step; the result carries the log also on failure.
/// Writes to the device without asking for `allow_apply`, that is up to the caller.
/// Updates to `target` with `routeros` and the `wanted` packages (see [`next_step`]); `power`
/// says which devices' updates and reboots it has to wait for.
pub async fn update(
    link: &impl DeviceLink,
    power: &PowerChain,
    target: &Version,
    wanted: &[String],
) -> UpdateResult {
    let mut log = String::new();
    let mut from = String::new();
    let mut to = String::new();
    let result = run(link, power, target, wanted, &mut log, &mut from, &mut to).await;
    UpdateResult {
        from,
        to,
        error: result.err().map(|e| {
            let _ = writeln!(log, "failed: {e}");
            e.to_string()
        }),
        log,
    }
}

async fn run(
    link: &impl DeviceLink,
    power: &PowerChain,
    target: &Version,
    wanted: &[String],
    log: &mut String,
    from: &mut String,
    to: &mut String,
) -> Result<(), Error> {
    let mut state = read_state(&link.api().await?).await?;
    *from = state.version.to_string();
    *to = from.clone();
    let _ = writeln!(
        log,
        "RouterOS {} ({}), packages {}",
        state.version,
        state.architecture,
        state.packages.join(", ")
    );
    let newest_v6 = if state.version.major() < 7 {
        Some(newest(6, Channel::LongTerm).await?)
    } else {
        None
    };
    // from the upload on a power loss can brick the device: it installs at the next boot
    let _updating = power.updating(log).await;
    while let Some(step) = next_step(&state, target, newest_v6.as_ref().unwrap_or(target), wanted) {
        let _ = writeln!(log, "update to {}: {}", step.version, step.files.join(", "));
        info!("update to {}: {}", step.version, step.files.join(", "));
        if !step.uninstall.is_empty() {
            let _ = writeln!(log, "uninstall {}", step.uninstall.join(", "));
        }
        let relaxed = allow_password_login(&link.api().await?, log).await?;
        let uploaded = async {
            for file in &step.files {
                let data = download(&step.version, file).await?;
                let _ = writeln!(log, "{file}: {} bytes downloaded", data.len());
                link.upload(file, &data).await?;
                // the size shows up while the file is still being written out (on 16 MB devices the
                // root directory is in RAM, there it settles within seconds)
                let device = link.api().await?;
                let expected = data.len().to_string();
                let deadline = Instant::now() + Duration::from_secs(60);
                loop {
                    let size = rows(&device, b"/file/print")
                        .await?
                        .iter()
                        .find(|row| field(row, "name") == Some(file))
                        .and_then(|row| field(row, "size").map(String::from));
                    if size.as_deref() == Some(&expected) {
                        break;
                    }
                    if Instant::now() > deadline {
                        return Err(Error::Update(
                            format!("{file} on the device has {size:?} bytes, not {expected}").into(),
                        ));
                    }
                    sleep(Duration::from_secs(2)).await;
                }
                let _ = writeln!(log, "{file}: copied to the device");
            }
            Ok::<_, Error>(())
        }
        .await;
        if let Some((setting, value)) = relaxed {
            set_ssh(&link.api().await?, setting, value).await?;
            let _ = writeln!(log, "ssh {setting}={value} again");
        }
        uploaded?;
        uninstall(&link.api().await?, &step.uninstall).await?;
        let expected = step.version.clone();
        state = reboot_and_wait(link, power, log, |state| {
            (state.version == expected).then(|| state.clone())
        })
        .await?;
        *to = state.version.to_string();
        let _ = writeln!(log, "RouterOS {} running", state.version);
    }
    if let Some((current, upgrade)) = &state.firmware
        && current != upgrade
    {
        let _ = writeln!(log, "firmware {current} -> {upgrade}");
        rows(&link.api().await?, b"/system/routerboard/upgrade").await?;
        let upgrade = upgrade.clone();
        reboot_and_wait(link, power, log, |state| {
            state
                .firmware
                .as_ref()
                .is_some_and(|(current, _)| *current == upgrade)
                .then_some(())
        })
        .await?;
        let _ = writeln!(log, "firmware {upgrade} running");
    }
    let _ = writeln!(log, "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DeviceState, UpdateStep, Version, next_step, parse_uptime};
    use std::time::Duration;

    #[test]
    fn parses_uptimes() {
        assert_eq!(parse_uptime("4d13h54m28s"), Some(Duration::from_secs(((4 * 24 + 13) * 60 + 54) * 60 + 28)));
        assert_eq!(parse_uptime("1w2d"), Some(Duration::from_secs(9 * 86400)));
        assert_eq!(parse_uptime("18s"), Some(Duration::from_secs(18)));
        assert_eq!(parse_uptime("1s500ms"), Some(Duration::from_millis(1500)));
        assert_eq!(parse_uptime("12"), None);
    }

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }
    fn state(version: &str, arch: &str, packages: &[&str]) -> DeviceState {
        DeviceState {
            version: v(version),
            architecture: arch.into(),
            packages: packages.iter().map(|p| p.to_string()).collect(),
            firmware: None,
            uptime: Duration::ZERO,
        }
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
        let wireless = ["wireless".to_string()];
        assert_eq!(
            next_step(&state("7.15.3", "arm", &["routeros", "wireless"]), &target, &v6, &wireless),
            Some(UpdateStep {
                version: target.clone(),
                files: vec!["routeros-7.20.8-arm.npk".into(), "wireless-7.20.8-arm.npk".into()],
                uninstall: vec![],
            })
        );
        assert_eq!(
            next_step(&state("7.24.5", "x86_64", &["routeros"]), &v("7.25"), &v6, &[])
                .unwrap()
                .files,
            vec!["routeros-7.25.npk".to_string()]
        );
        assert_eq!(next_step(&state("7.20.8", "arm", &["routeros"]), &target, &v6, &[]), None);
        // newer than the channel: no downgrade
        assert_eq!(next_step(&state("7.21", "arm", &["routeros"]), &target, &v6, &[]), None);
        let sxt = ["system", "wireless", "routing", "dhcp", "security"];
        assert_eq!(
            next_step(&state("6.45.1", "mipsbe", &sxt), &target, &v6, &wireless)
                .unwrap()
                .files,
            vec!["routeros-mipsbe-6.49.19.npk".to_string()]
        );
        assert_eq!(
            next_step(&state("6.49.19", "mipsbe", &sxt), &target, &v6, &wireless)
                .unwrap()
                .files,
            vec![
                "routeros-7.20.8-mipsbe.npk".to_string(),
                "wireless-7.20.8-mipsbe.npk".to_string()
            ]
        );
    }

    #[test]
    fn brings_the_packages_in_line() {
        let target = v("7.20.8");
        let v6 = v("6.49.19");
        let wifi = ["wifi-qcom-ac".to_string()];
        // an unused radio package goes with the update
        assert_eq!(
            next_step(&state("7.18.2", "arm", &["routeros", "wifi-qcom-ac"]), &target, &v6, &[]),
            Some(UpdateStep {
                version: target.clone(),
                files: vec!["routeros-7.20.8-arm.npk".into()],
                uninstall: vec!["wifi-qcom-ac".into()],
            })
        );
        // at the version already: only the missing package, and what isn't needed goes
        assert_eq!(
            next_step(&state("7.20.8", "arm", &["routeros", "wireless"]), &target, &v6, &wifi),
            Some(UpdateStep {
                version: target.clone(),
                files: vec!["wifi-qcom-ac-7.20.8-arm.npk".into()],
                uninstall: vec!["wireless".into()],
            })
        );
        assert_eq!(
            next_step(&state("7.20.8", "arm", &["routeros", "wifi-qcom-ac"]), &target, &v6, &wifi),
            None
        );
    }
}
