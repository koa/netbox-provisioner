//! Scope: The store of what was asked of devices and MikroTik: how fresh, who asks, when it is
//! dropped and who is told; not the asking itself (the modules that read).
//!
//! What the provisioner found out from devices and MikroTik, kept for a while
//! (`docs/device-state.md`).
//!
//! A fact is the outcome of asking something outside, a failure too, with the time the asking
//! began. Whoever needs one says how old it may be ([`Freshness`]); one too old is asked again.
//! A key is never asked twice at once: who comes while it runs joins it, who comes after takes
//! it from here. Nothing is asked ahead. A job that may change a device drops its facts once it
//! ended ([`invalidate`]); what was asked meanwhile isn't kept, since it may have missed the
//! change. Nothing is kept longer than [`LIMIT`].
//!
//! Times are instants of the monotonic clock ([`Instant`]); only ages go to the frontend.
use crate::{Error, lock::LockAnyway, topology::DeviceId};
use log::warn;
use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry, core::Collector,
};
use std::{
    any::Any,
    collections::HashMap,
    fmt::{self, Display},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{broadcast, watch},
    time::Instant,
};

#[cfg(test)]
mod test;

/// How long a fact is kept at most, whatever a user would take.
pub const LIMIT: Duration = Duration::from_secs(15 * 60);
/// How often the store is swept of facts over [`LIMIT`] (on the next use after that).
const SWEEP: Duration = Duration::from_secs(60);

/// How old a fact may be. There is no "any": every user says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// at most this old; `ZERO` asks anew, but joins one asking already
    MaxAge(Duration),
    /// begun after this instant, e.g. the start of the job asking
    Since(Instant),
}

impl Freshness {
    /// Whether a fact begun `at` will do.
    fn takes(self, at: Instant, now: Instant) -> bool {
        match self {
            Freshness::MaxAge(age) => now.saturating_duration_since(at) <= age.min(LIMIT),
            Freshness::Since(since) => at >= since && now.saturating_duration_since(at) <= LIMIT,
        }
    }
    /// Whether one asking since `started` will do: any for an age (its answer is the newest
    /// there will be), one begun after the instant for `Since`.
    fn joins(self, started: Instant) -> bool {
        match self {
            Freshness::MaxAge(_) => true,
            Freshness::Since(since) => started >= since,
        }
    }
    /// The smaller of two: what an argument from the frontend may do to a resolver's own.
    pub fn at_most(self, max_age: Option<Duration>) -> Self {
        match (self, max_age) {
            (Freshness::MaxAge(own), Some(asked)) => Freshness::MaxAge(own.min(asked)),
            (Freshness::Since(since), Some(asked)) => {
                let after = Instant::now().checked_sub(asked).unwrap_or(since);
                Freshness::Since(since.max(after))
            }
            (freshness, None) => freshness,
        }
    }
}

/// What a fact is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactKind {
    /// a login at one address, way and credentials: works, or why not
    Login,
    /// the way a device is reached at, after trying its addresses and neighbors
    Route,
    /// the config as the device has it (`DeviceCfg::fetch`)
    Config,
    /// RouterOS version and packages (`update::read_state`)
    RouterosState,
    /// the newest release in a channel, from MikroTik
    NewestVersion,
    /// what jobs left on the device (`leftovers::read`)
    Leftovers,
    Partitions,
    DeviceMode,
    /// the neighbors a device sees on a port (`/ip/neighbor`)
    Neighbors,
    /// a device's interfaces as it names them
    Interfaces,
    /// the live checks of the interface view (`interface_live::read`), against NetBox
    InterfaceLive,
    /// the OSPF link-state database a router has (`network_state::read`)
    Lsdb,
    /// a device's health and resources for the network map (`network_state::device_data`)
    DeviceMapData,
    /// a device's WLAN clients, interface counters and leases for `/metrics`
    /// (`device::metrics`)
    DeviceMetrics,
}

impl FactKind {
    /// every kind about a device: what „Neu laden“ drops
    pub const ABOUT_DEVICES: &[FactKind] = &[
        FactKind::Login,
        FactKind::Route,
        FactKind::Config,
        FactKind::RouterosState,
        FactKind::Leftovers,
        FactKind::Partitions,
        FactKind::DeviceMode,
        FactKind::Neighbors,
        FactKind::Interfaces,
        FactKind::InterfaceLive,
        FactKind::Lsdb,
        FactKind::DeviceMapData,
        FactKind::DeviceMetrics,
    ];
}

impl FactKind {
    /// the label in the metrics
    pub fn label(self) -> &'static str {
        match self {
            FactKind::Login => "login",
            FactKind::Route => "route",
            FactKind::Config => "config",
            FactKind::RouterosState => "routeros_state",
            FactKind::NewestVersion => "newest_version",
            FactKind::Leftovers => "leftovers",
            FactKind::Partitions => "partitions",
            FactKind::DeviceMode => "device_mode",
            FactKind::Neighbors => "neighbors",
            FactKind::Interfaces => "interfaces",
            FactKind::InterfaceLive => "interface_live",
            FactKind::Lsdb => "lsdb",
            FactKind::DeviceMapData => "device_map_data",
            FactKind::DeviceMetrics => "device_metrics",
        }
    }
}

/// Which fact: its kind, the device (none for one about MikroTik) and what else changes the
/// answer (address, way, credentials; a hash for credentials typed in).
#[derive(Debug, Clone)]
pub struct FactKey {
    pub kind: FactKind,
    pub device: Option<DeviceId>,
    pub detail: Box<str>,
    /// the way it goes (`routed`, `neighbor`), a label of the metrics; empty for none. Not
    /// part of which fact it is: a key built without it finds the fact all the same.
    pub path: &'static str,
}

impl PartialEq for FactKey {
    fn eq(&self, other: &Self) -> bool {
        (self.kind, self.device, &self.detail) == (other.kind, other.device, &other.detail)
    }
}

impl Eq for FactKey {}

impl std::hash::Hash for FactKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.kind, self.device, &self.detail).hash(state);
    }
}

impl FactKey {
    pub fn new(kind: FactKind, device: Option<DeviceId>, detail: impl Display) -> Self {
        FactKey {
            kind,
            device,
            detail: detail.to_string().into(),
            path: "",
        }
    }
    /// The same over `path` (`routed`, `neighbor`).
    pub fn via(mut self, path: &'static str) -> Self {
        self.path = path;
        self
    }
}

impl Display for FactKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.label())?;
        if let Some(device) = self.device {
            write!(f, " of {}", device.0)?;
        }
        if !self.detail.is_empty() {
            write!(f, " ({})", self.detail)?;
        }
        Ok(())
    }
}

/// A fact as a user gets it.
pub struct Fact<T> {
    pub value: Result<Arc<T>, Arc<Error>>,
    /// when its asking began
    pub as_of: Instant,
    /// how the user got it, for the metrics and tests
    pub came: Came,
}

impl<T> Clone for Fact<T> {
    fn clone(&self) -> Self {
        Fact {
            value: self.value.clone(),
            as_of: self.as_of,
            came: self.came,
        }
    }
}

impl<T> Fact<T> {
    /// The value, or the error it failed with (shared with every other user of the fact).
    pub fn result(&self) -> Result<Arc<T>, Error> {
        self.value.clone().map_err(Error::Shared)
    }
    pub fn age(&self) -> Duration {
        Instant::now().saturating_duration_since(self.as_of)
    }
}

/// How a user got a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Came {
    /// from the store
    Hit,
    /// joined the asking running already
    Joined,
    /// asked it
    Asked,
}

impl Came {
    fn label(self) -> &'static str {
        match self {
            Came::Hit => "hit",
            Came::Joined => "joined",
            Came::Asked => "miss",
        }
    }
}

/// What changed, on [`changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// a fact was stored anew: of concern only to who looked without asking ([`peek_newest`]);
    /// who asks gives it again when it gets older than shown ([`Reading::outdated`]), and two
    /// live pages would otherwise keep asking for each other
    Stored(FactKey),
    /// the facts of these kinds about the device were dropped: a job on it ended
    Invalidated(DeviceId, &'static [FactKind]),
    /// a job of the device started, ended or logged
    Job(DeviceId),
    /// the topology was read from NetBox again
    Topology,
}

type Stored = Arc<dyn Any + Send + Sync>;

#[derive(Clone)]
struct Done {
    value: Stored,
    as_of: Instant,
}

struct Running {
    started: Instant,
    /// `None` until it is done
    answer: watch::Receiver<Option<Stored>>,
}

#[derive(Default)]
struct Slot {
    done: Option<Done>,
    running: Option<Running>,
}

struct Store {
    slots: HashMap<FactKey, Slot>,
    /// when facts of a kind about a device were dropped last ([`invalidate`]; `None`:
    /// about every device, [`invalidate_everywhere`])
    dropped: HashMap<(Option<DeviceId>, FactKind), Instant>,
    /// per device, raised by [`invalidate`]; a fact begun under an older one isn't stored
    generations: HashMap<DeviceId, u64>,
    /// raised by [`invalidate_everywhere`], counts for every device
    everywhere: u64,
    swept: Instant,
}

fn new_store() -> Mutex<Store> {
    Mutex::new(Store {
        slots: HashMap::new(),
        dropped: HashMap::new(),
        generations: HashMap::new(),
        everywhere: 0,
        swept: Instant::now(),
    })
}

#[cfg(not(test))]
static STORE: LazyLock<Mutex<Store>> = LazyLock::new(new_store);

static CHANGES: LazyLock<broadcast::Sender<(u64, Change)>> =
    LazyLock::new(|| broadcast::channel(1024).0);
/// The number of the last change announced: who read before a number didn't see changes after it.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(not(test))]
fn store() -> std::sync::MutexGuard<'static, Store> {
    STORE.lock_anyway()
}

/// A store per thread: each test runs on a thread of its own with a clock of its own
/// (`start_paused`), which a store shared with other tests would mix.
#[cfg(test)]
fn store() -> std::sync::MutexGuard<'static, Store> {
    thread_local! {
        static STORE: &'static Mutex<Store> = Box::leak(Box::new(new_store()));
    }
    STORE.with(|store| *store).lock_anyway()
}

impl Store {
    fn generation(&self, device: Option<DeviceId>) -> u64 {
        self.everywhere
            + device
                .and_then(|d| self.generations.get(&d).copied())
                .unwrap_or(0)
    }
    fn sweep(&mut self, now: Instant) {
        if now.saturating_duration_since(self.swept) < SWEEP {
            return;
        }
        self.swept = now;
        self.slots.retain(|_, slot| {
            if slot
                .done
                .as_ref()
                .is_some_and(|done| now.saturating_duration_since(done.as_of) > LIMIT)
            {
                slot.done = None;
            }
            slot.done.is_some() || slot.running.is_some()
        });
        METRICS.entries.reset();
        for key in self
            .slots
            .iter()
            .filter(|(_, s)| s.done.is_some())
            .map(|(k, _)| k)
        {
            METRICS.entries.with_label_values(&[key.kind.label()]).inc();
        }
    }
}

/// Every change from now on, with its number: what subscriptions wait for.
pub fn changes() -> broadcast::Receiver<(u64, Change)> {
    CHANGES.subscribe()
}

/// The number of the last change announced.
pub fn sequence() -> u64 {
    SEQUENCE.load(Ordering::SeqCst)
}

/// Tells subscriptions about `change` (also for changes outside the store: jobs, topology).
pub fn announce(change: Change) {
    let number = SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
    // nobody listening is no error
    let _ = CHANGES.send((number, change));
}

/// What a subscription read for the answer it gave (`graphql::live`), so that it answers
/// again when that changes: the facts, the devices whose jobs it saw, and when the youngest
/// age it asked for runs out.
#[derive(Debug, Default)]
pub struct Reading {
    pub keys: std::collections::HashSet<FactKey>,
    pub devices: std::collections::HashSet<DeviceId>,
    /// the kinds of facts about devices it looked at without asking ([`peek_newest`])
    pub peeked: std::collections::HashSet<(DeviceId, FactKind)>,
    /// it saw the jobs of all devices (or of a site)
    pub all_jobs: bool,
    /// when the first fact read gets older than its user took it
    pub outdated: Option<Instant>,
    /// how many events of each job (by id) the subscription delivered; unlike the rest kept
    /// from round to round, so each answer brings only the new ones ([`events_from`])
    pub delivered: std::collections::HashMap<u32, u32>,
}

impl Reading {
    /// Whether `change` concerns what was read.
    pub fn concerns(&self, change: &Change) -> bool {
        match change {
            Change::Stored(key) => key
                .device
                .is_some_and(|device| self.peeked.contains(&(device, key.kind))),
            Change::Invalidated(device, kinds) => {
                self.keys
                    .iter()
                    .any(|key| key.device == Some(*device) && kinds.contains(&key.kind))
                    || kinds
                        .iter()
                        .any(|kind| self.peeked.contains(&(*device, *kind)))
            }
            Change::Job(device) => self.all_jobs || self.devices.contains(device),
            Change::Topology => true,
        }
    }
}

tokio::task_local! {
    /// what the subscription answering in this task reads
    static READING: Arc<Mutex<Reading>>;
}

/// Runs `work` noting what it reads into `reading` (facts asked, jobs listed).
#[cfg(test)]
pub async fn reading<F: Future>(reading: Arc<Mutex<Reading>>, work: F) -> F::Output {
    READING.scope(reading, work).await
}

/// Runs `poll` (a step of a subscription's stream) noting what it reads into `reading`.
pub fn reading_in<R>(reading: Arc<Mutex<Reading>>, poll: impl FnOnce() -> R) -> R {
    READING.sync_scope(reading, poll)
}

/// What the subscription answering in this task reads, if one does.
pub fn current_reading() -> Option<Arc<Mutex<Reading>>> {
    READING.try_with(Arc::clone).ok()
}

/// Notes that the subscription answering (if any) read `key`, taken at `max_age` from `as_of`.
fn note(key: &FactKey, as_of: Instant, max_age: Option<Duration>) {
    let _ = READING.try_with(|reading| {
        let mut reading = reading.lock_anyway();
        reading.keys.insert(key.clone());
        if let Some(device) = key.device {
            reading.devices.insert(device);
        }
        // an age of a moment is the moment (live views): no running again for it
        if let Some(age) = max_age.filter(|age| *age >= MIN_OUTDATED) {
            let outdated = as_of + age;
            reading.outdated = Some(reading.outdated.map_or(outdated, |o| o.min(outdated)));
        }
    });
}

/// Where the events of `job` asked `after` start for the subscription answering: after those
/// it delivered already; `after` itself outside a subscription.
pub fn events_from(job: u32, after: u32) -> u32 {
    READING
        .try_with(|reading| {
            let reading = reading.lock_anyway();
            reading
                .delivered
                .get(&job)
                .map_or(after, |delivered| after.max(*delivered))
        })
        .unwrap_or(after)
}

/// The subscription answering delivers the events of `job` up to `until`.
pub fn note_delivered(job: u32, until: u32) {
    let _ = READING.try_with(|reading| {
        reading.lock_anyway().delivered.insert(job, until);
    });
}

/// Notes that the subscription answering (if any) read the jobs of `device`, or of all devices.
pub fn note_jobs(device: Option<DeviceId>) {
    let _ = READING.try_with(|reading| {
        let mut reading = reading.lock_anyway();
        match device {
            Some(device) => {
                reading.devices.insert(device);
            }
            None => reading.all_jobs = true,
        }
    });
}

/// The youngest age that makes a subscription answer again when it runs out.
const MIN_OUTDATED: Duration = Duration::from_secs(5);

/// The fact `key` as `freshness` takes it: from the store, joining the asking running, or
/// asking it with `ask`. The asking runs on its own (`tokio::spawn`): a user going away
/// (a page closed) leaves it to the others and to the store.
pub async fn get<T, F>(key: FactKey, freshness: Freshness, ask: F) -> Fact<T>
where
    T: Send + Sync + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
{
    obtain(key, freshness, |_: &Result<Arc<T>, Arc<Error>>| None, ask).await
}

/// As [`get`], with the age a stored fact may have depending on what it says: `max_age` gives
/// it for the value or failure (`None`: `freshness`'s). A refused login stays refused until
/// the credentials change, while a device not answering may answer a second later.
pub async fn get_judged<T, F>(
    key: FactKey,
    freshness: Freshness,
    max_age: impl Fn(&Result<Arc<T>, Arc<Error>>) -> Option<Duration>,
    ask: F,
) -> Fact<T>
where
    T: Send + Sync + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
{
    obtain(key, freshness, max_age, ask).await
}

async fn obtain<T, F>(
    key: FactKey,
    freshness: Freshness,
    max_age: impl Fn(&Result<Arc<T>, Arc<Error>>) -> Option<Duration>,
    ask: F,
) -> Fact<T>
where
    T: Send + Sync + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
{
    let takes = |done: &Done, now| {
        let judged = done
            .value
            .downcast_ref::<Result<Arc<T>, Arc<Error>>>()
            .and_then(&max_age);
        judged
            .map_or(freshness, Freshness::MaxAge)
            .takes(done.as_of, now)
    };
    let mut ask = Some(ask);
    loop {
        let waiting = {
            let mut store = store();
            let now = Instant::now();
            store.sweep(now);
            let generation = store.generation(key.device);
            let slot = store.slots.entry(key.clone()).or_default();
            if let Some(done) = slot.done.as_ref().filter(|done| takes(done, now)) {
                let done = done.clone();
                drop(store);
                note(&key, done.as_of, age_of(freshness, &done.value, &max_age));
                return fact(&key, done.value, done.as_of, Came::Hit);
            }
            match &slot.running {
                Some(running) if freshness.joins(running.started) => {
                    Waiting::Join(running.started, running.answer.clone())
                }
                // an older one asking: its end first, never two at once
                Some(running) => Waiting::Before(running.answer.clone()),
                None => {
                    let (sender, answer) = watch::channel(None);
                    slot.running = Some(Running {
                        started: now,
                        answer: answer.clone(),
                    });
                    let ask = ask.take().expect("asked once");
                    // what it sends counts for the operation asking (`operation`)
                    let asking = run(key.clone(), now, generation, ask, sender);
                    tokio::spawn(crate::operation::within(
                        crate::operation::current(),
                        asking,
                    ));
                    Waiting::Join(now, answer)
                }
            }
        };
        match waiting {
            Waiting::Join(started, mut answer) => {
                let came = if ask.is_none() {
                    Came::Asked
                } else {
                    Came::Joined
                };
                let value = match answer.wait_for(Option::is_some).await {
                    Ok(value) => value.clone().expect("waited for it"),
                    // its task panicked
                    Err(_) => {
                        let error = Error::Panic(format!("asking {key} panicked").into());
                        let failed: Result<Arc<T>, Arc<Error>> = Err(Arc::new(error));
                        Arc::new(failed)
                    }
                };
                note(&key, started, age_of(freshness, &value, &max_age));
                return fact(&key, value, started, came);
            }
            Waiting::Before(mut answer) => {
                let _ = answer.wait_for(Option::is_some).await;
            }
        }
    }
}

/// The age a user took a fact at: `freshness`'s, or what `max_age` says for the value.
fn age_of<T: Send + Sync + 'static>(
    freshness: Freshness,
    value: &Stored,
    max_age: &impl Fn(&Result<Arc<T>, Arc<Error>>) -> Option<Duration>,
) -> Option<Duration> {
    let judged = value
        .downcast_ref::<Result<Arc<T>, Arc<Error>>>()
        .and_then(max_age);
    match judged.map_or(freshness, Freshness::MaxAge) {
        Freshness::MaxAge(age) => Some(age),
        Freshness::Since(_) => None,
    }
}

enum Waiting {
    Join(Instant, watch::Receiver<Option<Stored>>),
    Before(watch::Receiver<Option<Stored>>),
}

fn fact<T: Send + Sync + 'static>(
    key: &FactKey,
    value: Stored,
    as_of: Instant,
    came: Came,
) -> Fact<T> {
    METRICS
        .requests
        .with_label_values(&[key.kind.label(), came.label()])
        .inc();
    let value = value
        .downcast::<Result<Arc<T>, Arc<Error>>>()
        .map(|value| (*value).clone())
        .unwrap_or_else(|_| panic!("fact {key} asked as another type"));
    Fact { value, as_of, came }
}

/// Asks `key`, hands the answer to who waits and stores it unless a job on the device ended
/// meanwhile.
async fn run<T, F>(
    key: FactKey,
    started: Instant,
    generation: u64,
    ask: F,
    sender: watch::Sender<Option<Stored>>,
) where
    T: Send + Sync + 'static,
    F: Future<Output = Result<T, Error>> + Send + 'static,
{
    let label = key.kind.label();
    METRICS.running.with_label_values(&[label]).inc();
    let result = ask.await;
    let took = started.elapsed();
    METRICS.running.with_label_values(&[label]).dec();
    let outcome = outcome(&result);
    let device = key.device.map(|d| d.0.to_string()).unwrap_or_default();
    // every outcome's series is there from the first asking on, at 0: Prometheus' `increase`
    // doesn't count the first sample of a new series, so the first failure would be lost
    for each in OUTCOMES {
        METRICS.duration.with_label_values(&[label, each, key.path]);
        METRICS
            .total
            .with_label_values(&[device.as_str(), label, each]);
    }
    METRICS
        .duration
        .with_label_values(&[label, outcome, key.path])
        .observe(took.as_secs_f64());
    METRICS
        .total
        .with_label_values(&[device.as_str(), label, outcome])
        .inc();
    let value: Stored = Arc::new(result.map(Arc::new).map_err(Arc::new));
    let kept = {
        let mut store = store();
        let current = store.generation(key.device);
        let slot = store.slots.entry(key.clone()).or_default();
        slot.running = None;
        let keep = current == generation;
        if keep {
            let before = slot.done.replace(Done {
                value: value.clone(),
                as_of: started,
            });
            if before.is_none() {
                METRICS.entries.with_label_values(&[label]).inc();
            }
        }
        keep
    };
    let _ = sender.send(Some(value));
    if kept {
        announce(Change::Stored(key));
    }
}

/// The facts of `kinds` about `device` go, and nothing asked before now is stored anymore:
/// a job on the device ended.
pub fn invalidate(device: DeviceId, kinds: &'static [FactKind]) {
    {
        let mut store = store();
        *store.generations.entry(device).or_default() += 1;
        let now = Instant::now();
        for kind in kinds {
            store.dropped.insert((Some(device), *kind), now);
        }
        for (key, slot) in &mut store.slots {
            if key.device == Some(device) && kinds.contains(&key.kind) && slot.done.take().is_some()
            {
                METRICS.entries.with_label_values(&[key.kind.label()]).dec();
            }
        }
    }
    announce(Change::Invalidated(device, kinds));
}

/// The facts of `kinds` about every device go: NetBox changed what they are worked out from
/// (the generated config, the packages an update keeps).
pub fn invalidate_everywhere(kinds: &'static [FactKind]) {
    let mut store = store();
    store.everywhere += 1;
    let now = Instant::now();
    for kind in kinds {
        store.dropped.insert((None, *kind), now);
    }
    for (key, slot) in &mut store.slots {
        if kinds.contains(&key.kind) && slot.done.take().is_some() {
            METRICS.entries.with_label_values(&[key.kind.label()]).dec();
        }
    }
}

/// The fact `key` as stored, whatever its age (but within [`LIMIT`]); `None` without one.
/// Asks nothing and counts as no use.
pub fn peek<T: Send + Sync + 'static>(key: &FactKey) -> Option<Fact<T>> {
    let store = store();
    let done = store.slots.get(key)?.done.as_ref()?;
    if done.as_of.elapsed() > LIMIT {
        return None;
    }
    let value = done
        .value
        .downcast_ref::<Result<Arc<T>, Arc<Error>>>()?
        .clone();
    Some(Fact {
        value,
        as_of: done.as_of,
        came: Came::Hit,
    })
}

/// The youngest fact of `kind` about `device` among the keys `filter` takes, whatever its way
/// and credentials, without asking: what the provisioner knows. A subscription reading it
/// answers again when one is stored or dropped.
pub fn peek_newest<T: Send + Sync + 'static>(
    kind: FactKind,
    device: DeviceId,
    filter: impl Fn(&FactKey) -> bool,
) -> Option<Fact<T>> {
    let _ = READING.try_with(|reading| {
        reading.lock_anyway().peeked.insert((device, kind));
    });
    let store = store();
    let (_, done) = store
        .slots
        .iter()
        .filter(|(key, _)| key.kind == kind && key.device == Some(device) && filter(key))
        .filter_map(|(key, slot)| Some((key, slot.done.as_ref()?)))
        .filter(|(_, done)| done.as_of.elapsed() <= LIMIT)
        .max_by_key(|(_, done)| done.as_of)?;
    let value = done
        .value
        .downcast_ref::<Result<Arc<T>, Arc<Error>>>()?
        .clone();
    Some(Fact {
        value,
        as_of: done.as_of,
        came: Came::Hit,
    })
}

/// When facts of `kind` about `device` were dropped last (a job on it, NetBox changed), if
/// they were.
pub fn dropped(kind: FactKind, device: DeviceId) -> Option<Instant> {
    let store = store();
    let about_device = store.dropped.get(&(Some(device), kind));
    let everywhere = store.dropped.get(&(None, kind));
    about_device.into_iter().chain(everywhere).max().copied()
}

/// Stores `value` as the fact `key`, read at `as_of`: what a job saw itself at its end (an
/// apply's check), put after its end dropped the facts about the device.
pub fn put<T: Send + Sync + 'static>(key: FactKey, value: T, as_of: Instant) {
    {
        let mut store = store();
        let value: Stored = Arc::new(Ok::<_, Arc<Error>>(Arc::new(value)));
        let slot = store.slots.entry(key.clone()).or_default();
        if slot.done.replace(Done { value, as_of }).is_none() {
            METRICS.entries.with_label_values(&[key.kind.label()]).inc();
        }
    }
    announce(Change::Stored(key));
}

/// Drops the one fact `key`, e.g. a connection found broken.
pub fn forget(key: &FactKey) {
    let mut store = store();
    if let Some(slot) = store.slots.get_mut(key)
        && slot.done.take().is_some()
    {
        METRICS.entries.with_label_values(&[key.kind.label()]).dec();
    }
}

/// What [`outcome`] gives.
const OUTCOMES: [&str; 5] = ["ok", "refused", "timeout", "wrong_device", "error"];

/// The few kinds of outcome the metrics tell apart.
fn outcome<T>(result: &Result<T, Error>) -> &'static str {
    use crate::device::terminal::TerminalError;
    use mikrotik_api::error::Error as ApiError;
    let Err(error) = result else {
        return "ok";
    };
    match error.root() {
        Error::Timeout(_)
        | Error::NeighborNoAnswer(..)
        | Error::MikrotikApi(ApiError::LoginTimeout(_) | ApiError::ReplyTimeout(_)) => "timeout",
        Error::MikrotikApi(ApiError::LoginFailed)
        | Error::Terminal(TerminalError::LoginRefused(_))
        | Error::MissingCredentials => "refused",
        Error::WrongDevice { .. } => "wrong_device",
        _ => "error",
    }
}

/// The metrics of the facts, registered with the server's registry ([`register`]).
struct Metrics {
    /// how long askings took, by kind and outcome
    duration: HistogramVec,
    /// askings by device, kind and outcome
    total: IntCounterVec,
    /// users by kind and how they got the fact
    requests: IntCounterVec,
    entries: IntGaugeVec,
    running: IntGaugeVec,
}

/// Fine and logarithmic for heatmaps, 15 s being the timeout of a connection.
const BUCKETS: &[f64] = &[
    0.025, 0.05, 0.1, 0.2, 0.35, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0, 7.5, 10.0, 15.0, 20.0, 30.0,
    60.0,
];

static METRICS: LazyLock<Metrics> = LazyLock::new(|| Metrics {
    duration: HistogramVec::new(
        HistogramOpts::new(
            "device_query_duration_seconds",
            "How long asking a device or MikroTik took (not the facts from the store)",
        )
        .buckets(BUCKETS.to_vec()),
        &["kind", "outcome", "path"],
    )
    .expect("histogram"),
    total: IntCounterVec::new(
        Opts::new("device_query_total", "Askings of a device or MikroTik"),
        &["device", "kind", "outcome"],
    )
    .expect("counter"),
    requests: IntCounterVec::new(
        Opts::new(
            "fact_cache_requests_total",
            "Users of a fact, by how they got it",
        ),
        &["kind", "result"],
    )
    .expect("counter"),
    entries: IntGaugeVec::new(
        Opts::new("fact_cache_entries", "Facts in the store"),
        &["kind"],
    )
    .expect("gauge"),
    running: IntGaugeVec::new(
        Opts::new("fact_queries_running", "Askings running"),
        &["kind"],
    )
    .expect("gauge"),
});

/// Puts the metrics of the facts into `registry` (`/metrics`).
pub fn register(registry: &Registry) {
    let collectors: [Box<dyn Collector>; 5] = [
        Box::new(METRICS.duration.clone()),
        Box::new(METRICS.total.clone()),
        Box::new(METRICS.requests.clone()),
        Box::new(METRICS.entries.clone()),
        Box::new(METRICS.running.clone()),
    ];
    for collector in collectors {
        if let Err(error) = registry.register(collector) {
            warn!("Cannot register the metrics of the facts: {error}");
        }
    }
}
