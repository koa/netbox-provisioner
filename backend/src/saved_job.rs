//! The jobs that go on after a restart of the provisioner (`docs/concepts/device-state.md`,
//! „Speicher“): only an apply whose script runs or whose fallback is being taken away, and an
//! update with its fallback set while it reboots or decides about that fallback. Ending
//! those would let a fallback take back a change that went well; every other job ends as
//! interrupted and is never written down.
//!
//! One file per device, `state_dir/jobs/<netbox-id>.json`, written on entering such a state
//! (to a temporary file first, then renamed, so a crash leaves the old one or the new one) and
//! removed on leaving it. The server holds `state_dir/lock` while it runs: a second one with
//! the same `state_dir` doesn't start. Files read at the start wait in [`waiting`] until their
//! job takes them up again, or someone discards them (a device NetBox doesn't know anymore).
//! Without [`open`] (tests, the CHR harness) nothing is written.
use crate::{
    device::watchdog::Watching,
    device_state::{ApplyState, JobPhase, JobPhaseObject, UpdateState},
    graphql::scalars::ScalarDuration,
    topology::{DeviceId, SiteId},
};
use async_graphql::{Enum, Object};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant, SystemTime},
};

#[cfg(test)]
mod test;

/// What a job saves to go on after a restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedJob {
    pub device: DeviceId,
    /// the device's name then, for a device NetBox doesn't know anymore
    pub device_name: Box<str>,
    pub site: Option<SiteId>,
    /// when it was written, by the server's clock
    #[serde(with = "unix_seconds")]
    pub saved_at: SystemTime,
    pub work: SavedWork,
}

/// The kind of job, with what it needs to go on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SavedWork {
    Apply {
        state: ApplyState,
        #[serde(flatten)]
        apply: Box<SavedApply>,
    },
    Update {
        state: UpdateState,
        #[serde(flatten)]
        update: Box<SavedUpdate>,
    },
}

impl SavedWork {
    pub fn phase(&self) -> JobPhase {
        match self {
            SavedWork::Apply { state, .. } => (*state).into(),
            SavedWork::Update { state, .. } => (*state).into(),
        }
    }
    /// Takes `phase` as the state to save; `false` for another kind's.
    pub fn set_phase(&mut self, phase: JobPhase) -> bool {
        match (self, phase) {
            (SavedWork::Apply { state, .. }, JobPhase::Apply(phase)) => *state = phase,
            (SavedWork::Update { state, .. }, JobPhase::Update(phase)) => *state = phase,
            _ => return false,
        }
        true
    }
}

/// An apply whose script runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedApply {
    /// the credentials it was reached with, by name (`mikrotik_credentials`); it goes on
    /// connecting as for a job without target
    pub credentials: Option<Box<str>>,
    pub script: ScriptRun,
    /// the watchdogs it suspended for a radio it changes (`RadioHold`)
    pub hold: Option<SavedHold>,
    /// applied together with other devices (`device::batch`): they go on together
    #[serde(default)]
    pub batch: Option<SavedBatch>,
}

/// An update with its fallback set, rebooting or deciding about the fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedUpdate {
    /// the credentials it was reached with, by name, as for an apply
    pub credentials: Option<Box<str>>,
    /// the watchdogs of other devices it suspended (`suspendWatchdogs`), by device
    pub watchers: Vec<(Box<str>, Watching)>,
    #[serde(flatten)]
    pub run: UpdateRun,
}

/// What the update itself (`device::update`) needs to go on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateRun {
    /// the version it started from
    pub from: Box<str>,
    pub target: Box<str>,
    /// the newest RouterOS 6, a step on the way from 6 (the target where it started on 7)
    pub newest_v6: Box<str>,
    pub wanted: Vec<Box<str>>,
    /// the partition the fallback switches back to
    pub partition: Box<str>,
    /// how long a reboot is waited for, counted from entering `Rebooting`
    pub wait: Duration,
    /// the OSPF neighbors adjacent before, to be back after it
    pub ospf: BTreeSet<Box<str>>,
    /// the version the reboot brings
    pub expected: Option<Box<str>>,
    /// why it failed, in `NotUpdated` (the error's message)
    pub failed: Option<Box<str>>,
}

/// The batch an apply was part of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedBatch {
    /// all its devices, this one among them
    pub devices: Vec<DeviceId>,
    /// the delay of its fallbacks, firing together
    pub delay: Duration,
}

/// What an apply knows once its script runs, enough to watch it to the end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptRun {
    /// the run's mark in the progress file
    pub run: Box<str>,
    /// a fallback is set
    pub armed: bool,
    #[serde(with = "unix_seconds")]
    pub armed_at: SystemTime,
    /// when the fallback fires, by the provisioner's count
    #[serde(with = "unix_seconds")]
    pub fires: SystemTime,
    /// a device without marks counts as through after this
    #[serde(with = "unix_seconds")]
    pub unmarked: SystemTime,
    /// the radios provisioned again after it (`reprovisioned_menus`)
    pub reprovision: Vec<Box<str>>,
}

/// The watchdogs an apply holds suspended: the device's own (as it was), and others' by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedHold {
    pub own: Option<Watching>,
    pub others: Vec<(Box<str>, Watching)>,
}

/// `at`, by the server's clock, as an `Instant` of this run.
pub fn instant(at: SystemTime) -> Instant {
    let now = Instant::now();
    match at.duration_since(SystemTime::now()) {
        Ok(ahead) => now + ahead,
        Err(behind) => now.checked_sub(behind.duration()).unwrap_or(now),
    }
}

/// `at`, an `Instant` of this run, by the server's clock.
pub fn system_time(at: Instant) -> SystemTime {
    let now = SystemTime::now();
    let instant_now = Instant::now();
    match at.checked_duration_since(instant_now) {
        Some(ahead) => now + ahead,
        None => now - instant_now.duration_since(at),
    }
}

/// Times in the file are seconds since the epoch: an `Instant` means nothing after a restart.
mod unix_seconds {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        Ok(UNIX_EPOCH + Duration::from_secs(u64::deserialize(deserializer)?))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0} is held by another provisioner")]
    Locked(PathBuf),
    #[error("Cannot use {0}: {1}")]
    Io(PathBuf, io::Error),
}

/// The open `state_dir`; the lock is held as long as the file is open.
struct Store {
    jobs: PathBuf,
    _lock: File,
}

static STORE: OnceLock<Store> = OnceLock::new();

/// The files read at the start that no job took up yet, by device.
static WAITING: Mutex<BTreeMap<DeviceId, Waiting>> = Mutex::new(BTreeMap::new());

/// A file read at the start.
#[derive(Debug, Clone)]
pub struct Waiting {
    pub device: DeviceId,
    /// what it holds, or why it couldn't be read
    pub saved: Result<SavedJob, Box<str>>,
    /// why no job went on with it, once the start looked at it
    pub held_back: Option<HeldBack>,
}

/// Why a saved job didn't go on.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum HeldBack {
    /// NetBox doesn't have its device anymore
    DeviceGone,
    /// a batch whose other devices aren't all saved or known: their fallbacks take all of it
    /// back
    BatchIncomplete,
}

/// Leaves the file of `device` waiting, saying why.
pub fn hold_back(device: DeviceId, why: HeldBack) {
    if let Some(waiting) = WAITING.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&device) {
        waiting.held_back = Some(why);
    }
}

/// Takes `dir` for this server: holds its lock, reads the jobs saved there (they wait in
/// [`waiting`]), and from now on saves there. Called once at the start.
pub fn open(dir: &Path) -> Result<(), StoreError> {
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |error| StoreError::Io(path, error)
    };
    let jobs = dir.join("jobs");
    fs::create_dir_all(&jobs).map_err(io_error(&jobs))?;
    let lock = lock(dir)?;
    let read = read_all(&jobs).map_err(io_error(&jobs))?;
    info!("{} saved jobs in {}", read.len(), jobs.display());
    *WAITING.lock().unwrap_or_else(PoisonError::into_inner) =
        read.into_iter().map(|waiting| (waiting.device, waiting)).collect();
    if STORE.set(Store { jobs, _lock: lock }).is_err() {
        warn!("state_dir opened twice, the first one stays");
    }
    Ok(())
}

/// Holds `dir/lock` as long as the file is open.
fn lock(dir: &Path) -> Result<File, StoreError> {
    let path = dir.join("lock");
    let lock = File::create(&path).map_err(|error| StoreError::Io(path.clone(), error))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(fs::TryLockError::WouldBlock) => Err(StoreError::Locked(dir.to_path_buf())),
        Err(fs::TryLockError::Error(error)) => Err(StoreError::Io(path, error)),
    }
}

/// Every `<id>.json` in `jobs`; another file there is left alone, a temporary one of a write
/// that didn't end removed.
fn read_all(jobs: &Path) -> io::Result<Vec<Waiting>> {
    let mut read = Vec::new();
    for entry in fs::read_dir(jobs)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.ends_with(".json.tmp") {
            let _ = fs::remove_file(&path);
            continue;
        }
        let Some(device) = name.strip_suffix(".json").and_then(|id| id.parse().ok()).map(DeviceId) else {
            continue;
        };
        let saved = fs::read(&path)
            .map_err(|error| error.to_string().into_boxed_str())
            .and_then(|bytes| serde_json::from_slice::<SavedJob>(&bytes).map_err(|error| error.to_string().into()))
            .and_then(|saved| match saved.device == device {
                true => Ok(saved),
                false => Err(format!("saved for device {}", saved.device.0).into()),
            });
        read.push(Waiting {
            device,
            saved,
            held_back: None,
        });
    }
    read.sort_by_key(|waiting| waiting.device);
    Ok(read)
}

fn file(jobs: &Path, device: DeviceId) -> PathBuf {
    jobs.join(format!("{}.json", device.0))
}

/// Writes `saved` for its device, replacing what was saved for it. A failure only goes to the
/// log: the job goes on as it would without `state_dir`, a restart then ends it.
pub fn save(saved: &SavedJob) {
    let Some(store) = STORE.get() else {
        return;
    };
    if let Err(error) = write(&store.jobs, saved) {
        warn!("Cannot save the job of {}: {error}", saved.device_name);
    }
}

fn write(jobs: &Path, saved: &SavedJob) -> io::Result<()> {
    let path = file(jobs, saved.device);
    let temporary = path.with_extension("json.tmp");
    let file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&file, saved)?;
    file.sync_all()?;
    fs::rename(&temporary, &path)
}

/// Removes what was saved for `device`: its job left the states that go on, or ended.
pub fn remove(device: DeviceId) {
    let Some(store) = STORE.get() else {
        return;
    };
    match fs::remove_file(file(&store.jobs, device)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            warn!("Cannot remove the saved job of device {}: {error}", device.0)
        }
        _ => {}
    }
}

/// The files read at the start that no job took up yet.
pub fn waiting() -> Vec<Waiting> {
    WAITING.lock().unwrap_or_else(PoisonError::into_inner).values().cloned().collect()
}

/// Takes the file of `device` out of [`waiting`], to go on with it.
pub fn take(device: DeviceId) -> Option<Waiting> {
    WAITING.lock().unwrap_or_else(PoisonError::into_inner).remove(&device)
}

/// Discards the file of `device` read at the start; `false` if none waits.
pub fn discard(device: DeviceId) -> bool {
    match take(device) {
        Some(_) => {
            remove(device);
            true
        }
        None => false,
    }
}

/// What kind of job a saved one is.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum SavedJobKind {
    Apply,
    Update,
}

/// A job saved before the restart that didn't go on (yet): its device may be gone from NetBox.
#[Object]
impl Waiting {
    async fn device_id(&self) -> u32 {
        self.device.0
    }
    /// the name saved with it; `null` for a file that couldn't be read
    async fn device_name(&self) -> Option<&str> {
        self.saved.as_ref().ok().map(|saved| &*saved.device_name)
    }
    async fn site_id(&self) -> Option<u32> {
        self.saved.as_ref().ok().and_then(|saved| saved.site).map(|site| site.0)
    }
    async fn kind(&self) -> Option<SavedJobKind> {
        self.saved.as_ref().ok().map(|saved| match saved.work {
            SavedWork::Apply { .. } => SavedJobKind::Apply,
            SavedWork::Update { .. } => SavedJobKind::Update,
        })
    }
    /// the state it was saved in
    async fn phase(&self) -> Option<JobPhaseObject> {
        self.saved.as_ref().ok().map(|saved| saved.work.phase().into())
    }
    /// how long ago it was saved
    async fn age(&self) -> Option<ScalarDuration> {
        self.saved
            .as_ref()
            .ok()
            .map(|saved| SystemTime::now().duration_since(saved.saved_at).unwrap_or(Duration::ZERO).into())
    }
    /// why it didn't go on; `null` with `unreadable`
    async fn held_back(&self) -> Option<HeldBack> {
        self.held_back
    }
    /// why the file couldn't be read (as serde or the file system said)
    async fn unreadable(&self) -> Option<&str> {
        self.saved.as_ref().err().map(Box::as_ref)
    }
}
