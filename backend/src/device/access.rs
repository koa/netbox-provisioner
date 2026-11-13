//! Scope: who may send what to a device ([`Reader`], [`Session`]) and the replies of the
//! actions; not how a connection is made (`device`), not what is read (the submodules).
//!
//! The facades of `docs/device-access.md`: a [`Reader`] sends only what changes
//! nothing (`print`, monitors, ping), a [`Session`] is a job's own connection and also writes.
//! Both go over a `mikrotik_api` [`Connection`]; each command is a type of mikrotik-model (a
//! menu's model, an action of `mikrotik_model::action`), none a string.
//!
//! A [`Reader`] given an age ([`Reader::at`]) takes what it reads from the facts: each print,
//! monitor once and ping is a fact of its own (`FactKind::Menu`), its replies kept unread and
//! read again by whichever type asks (`docs/concepts/device-access.md`, Basis-Fakten).
use crate::{
    Error,
    facts::{self, FactKey, FactKind, Freshness},
    lock::LockAnyway,
    operation,
    topology::DeviceId,
};
use async_graphql::futures_util::StreamExt;
use async_graphql::futures_util::future::join_all;
use mikrotik_api::{
    command::{EffectKind, Observe, Read, SessionBound, Write},
    prelude::{Command, Connection, Generation, Prepared, Replies, Reply, TrapCategory},
    simple::SimpleResult,
};
use mikrotik_model::{
    action::Ping,
    command::{Reads, Writes},
    resource::{
        self, DeserializeRosResource, KeyedResource, ResourceMutation, RosResource, apply_mutation,
        collect_resource,
    },
};
use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    net::IpAddr,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

/// Reads from a device: what it may send changes nothing. Cheap to clone; all clones share the
/// login and how old the oldest it read is ([`Reader::as_of`]).
#[derive(Clone)]
pub struct Reader {
    source: Source,
    /// the NetBox device, whose commands the operation running counts (`operation`) and whose
    /// facts it reads; none for a connection without (the CHR harness), which asks the device
    /// each time
    device: Option<DeviceId>,
    scope: Scope,
    /// when the oldest it read was asked
    as_of: Arc<Mutex<Option<Instant>>>,
}

/// Whose facts a reader's are: the credentials it logged in with (`Credentials::fact_key`;
/// typed-in ones may see less, so what they read stays apart) and the way it goes, a label of
/// the metrics (`Path::label`).
#[derive(Clone, Default)]
pub(crate) struct Scope {
    pub credentials: Arc<str>,
    pub way: &'static str,
}

/// Where a [`Reader`] takes what it reads.
#[derive(Clone)]
enum Source {
    /// the device, each time
    Device(Connection),
    /// the facts as old as `freshness` takes them, asking the device for what they lack
    Facts {
        connection: Connection,
        freshness: Freshness,
    },
    /// only what the facts hold of the device, whatever its age: it asks nothing
    /// ([`Reader::stored`])
    Stored(DeviceId),
}

impl Reader {
    pub(crate) fn new(connection: Connection, device: Option<DeviceId>, scope: Scope) -> Self {
        Reader {
            source: Source::Device(connection),
            device,
            scope,
            as_of: Arc::default(),
        }
    }
    /// What the facts hold of `device` as read with the credentials `scope`, without a login:
    /// a command they lack fails as if the connection had (`ConnectionClosed`). A subscription
    /// reading it answers again when a fact of the device is stored or dropped.
    pub(crate) fn stored(device: DeviceId, credentials: &str) -> Self {
        Reader {
            source: Source::Stored(device),
            device: Some(device),
            scope: Scope {
                credentials: credentials.into(),
                way: "",
            },
            as_of: Arc::default(),
        }
    }
    /// The same login, taking what it reads from the facts as old as `freshness` takes it;
    /// its age counts anew ([`Reader::as_of`]).
    pub fn at(&self, freshness: Freshness) -> Reader {
        let source = match &self.source {
            Source::Device(connection) | Source::Facts { connection, .. } => Source::Facts {
                connection: connection.clone(),
                freshness,
            },
            Source::Stored(device) => Source::Stored(*device),
        };
        Reader {
            source,
            device: self.device,
            scope: self.scope.clone(),
            as_of: Arc::default(),
        }
    }
    /// When the oldest of what it read was asked (now before it read anything): what a value
    /// worked out from it is as of.
    pub fn as_of(&self) -> Instant {
        self.as_of.lock_anyway().unwrap_or_else(Instant::now)
    }
    /// The login is gone (the device rebooted, a tunnel closed), or there is none.
    pub fn is_closed(&self) -> bool {
        match &self.source {
            Source::Device(connection) | Source::Facts { connection, .. } => connection.is_closed(),
            Source::Stored(_) => true,
        }
    }
    fn read_at(&self, at: Instant) {
        let mut as_of = self.as_of.lock_anyway();
        *as_of = Some(as_of.map_or(at, |as_of| as_of.min(at)));
    }
    /// Sends `command`, or gives its replies from the facts.
    async fn send<C: Command + Sync>(&self, command: &C) -> Replies<C::Reply>
    where
        C::Effect: Send + 'static,
    {
        let (device, connection, freshness) = match (&self.source, self.device) {
            (
                Source::Facts {
                    connection,
                    freshness,
                },
                Some(device),
            ) => (device, connection, *freshness),
            (Source::Stored(device), _) => return self.stored_replies(*device, command),
            (Source::Device(connection) | Source::Facts { connection, .. }, device) => {
                if let Some(device) = device {
                    operation::note(device, &command.path());
                }
                self.read_at(Instant::now());
                return connection.run(command).await;
            }
        };
        let prepared = Prepared::of(command);
        let key = self.key(device, &prepared);
        if written_since_read(device) {
            facts::invalidate(device, &[FactKind::Menu]);
        }
        let connection = connection.clone();
        let ask = async move {
            operation::note(device, prepared.path());
            let replies: Vec<Reply> = connection.run(&prepared).await.unparsed().collect().await;
            Ok(Kept(replies.into()))
        };
        let fact = facts::get_judged(key, freshness, failed, ask).await;
        self.read_at(fact.as_of);
        let kept = match fact.value {
            Ok(kept) => kept.0.clone(),
            // its asking panicked (`facts` says so)
            Err(_) => not_kept(),
        };
        Replies::kept(kept, command.context())
    }
    /// The fact of `command` on `device` with this reader's credentials.
    fn key<E: EffectKind>(&self, device: DeviceId, prepared: &Prepared<E>) -> FactKey {
        FactKey::new(
            FactKind::Menu,
            Some(device),
            format!(
                "{} {}{}",
                self.scope.credentials,
                prepared.path().escape_ascii(),
                prepared.words().escape_ascii()
            ),
        )
        .via(self.scope.way)
    }
    fn stored_replies<C: Command>(&self, device: DeviceId, command: &C) -> Replies<C::Reply> {
        facts::note_peeked(device, FactKind::Menu);
        let key = self.key(device, &Prepared::of(command));
        let kept = match facts::peek::<Kept>(&key) {
            Some(fact) => {
                self.read_at(fact.as_of);
                match fact.value {
                    Ok(kept) => kept.0.clone(),
                    Err(_) => not_kept(),
                }
            }
            None => not_kept(),
        };
        Replies::kept(kept, command.context())
    }
}

/// The replies of a command not kept: as if the connection had failed.
fn not_kept() -> Arc<[Reply]> {
    Arc::new([Reply::Error(mikrotik_api::error::Error::ConnectionClosed)])
}

/// The replies of a command as they came, for any type to read again.
struct Kept(Arc<[Reply]>);

/// Replies that are the connection failing are no answer of the device: not taken again.
fn failed(kept: &Result<Arc<Kept>, Arc<Error>>) -> Option<Duration> {
    match kept {
        Ok(kept) if kept.0.iter().any(Reply::is_failure) => Some(Duration::ZERO),
        _ => None,
    }
}

impl Reads for Reader {
    fn read<C: Command<Effect = Read> + Sync>(
        &self,
        command: &C,
    ) -> impl Future<Output = Replies<C::Reply>> + Send {
        self.send(command)
    }
    fn observe<C: Command<Effect = Observe> + Sync>(
        &self,
        command: &C,
    ) -> impl Future<Output = Replies<C::Reply>> + Send {
        self.send(command)
    }
}

/// A job's own connection to a device: it reads, writes and holds what lasts as long as the
/// connection (the device mode's countdown). Its lifetime means something (an apply cuts its
/// path, a reboot ends it), so a job opens its own (`AccessibleDevice::connect_again`).
pub struct Session {
    connection: Connection,
    device: Option<DeviceId>,
    scope: Scope,
}

impl Session {
    /// The CHR harness, which has no jobs nor NetBox devices, takes its own.
    pub fn new(connection: Connection) -> Self {
        Session {
            connection,
            device: None,
            scope: Scope::default(),
        }
    }
    /// One to `device`, its readers' facts those of `scope`: the provisioner takes one only
    /// with the device's lock (`job::start*`).
    pub(crate) fn of(connection: Connection, device: DeviceId, scope: Scope) -> Self {
        Session {
            connection,
            device: Some(device),
            scope,
        }
    }
    /// The same connection, to hand to what only reads: it reads from the device, unless told
    /// an age ([`Reader::at`]).
    pub fn reader(&self) -> Reader {
        Reader::new(self.connection.clone(), self.device, self.scope.clone())
    }
    /// Sends `command`, which acts as long as the connection lasts; dropping its replies or the
    /// session ends it.
    pub async fn hold<C: Command<Effect = SessionBound>>(&self, command: &C) -> Replies<C::Reply> {
        self.connection.run(command).await
    }
}

impl Reads for Session {
    fn read<C: Command<Effect = Read> + Sync>(
        &self,
        command: &C,
    ) -> impl Future<Output = Replies<C::Reply>> + Send {
        self.connection.run(command)
    }
    fn observe<C: Command<Effect = Observe> + Sync>(
        &self,
        command: &C,
    ) -> impl Future<Output = Replies<C::Reply>> + Send {
        self.connection.run(command)
    }
}

impl Writes for Session {
    fn write<C: Command<Effect = Write> + Sync>(
        &self,
        command: &C,
    ) -> impl Future<Output = Replies<C::Reply>> + Send {
        self.connection.run(command)
    }
}

/// The write counter of a device, shared by all its connections, and where it stood when its
/// facts were last read.
#[derive(Default)]
struct Counter {
    generation: Generation,
    read_at: AtomicU64,
}

fn counter<R>(device: DeviceId, with: impl FnOnce(&Counter) -> R) -> R {
    static COUNTERS: LazyLock<Mutex<HashMap<DeviceId, Counter>>> = LazyLock::new(Default::default);
    with(
        COUNTERS
            .lock()
            .expect("counters poisoned")
            .entry(device)
            .or_default(),
    )
}

/// The write counter of `device`, shared by all its connections: every write and session bound
/// command raises it (what was read before is stale then).
pub(crate) fn generation(device: DeviceId) -> Generation {
    counter(device, |counter| counter.generation.clone())
}

/// Whether `device` was written to since its facts were last read (on any connection): what
/// was read before may be stale. Counts as read from now on.
fn written_since_read(device: DeviceId) -> bool {
    counter(device, |counter| {
        let now = counter.generation.current();
        counter.read_at.swap(now, Ordering::SeqCst) != now
    })
}

/// The rows of `R`'s menu, none where the device hasn't the menu (a package or a RouterOS
/// version without it, `/partitions` on a CHR).
pub async fn collect_if_present<R: DeserializeRosResource + RosResource>(
    device: &impl Reads,
) -> Result<Box<[R]>, Error> {
    match collect_resource::<R>(device).await {
        Err(resource::Error::Trap(trap)) if is_missing_menu(trap.category, &trap.message) => {
            Ok(Box::new([]))
        }
        rows => Ok(rows?),
    }
}

/// RouterOS's answer to a menu it hasn't: `no such command prefix` without a category (the
/// package missing), or a missing item or command.
pub fn is_missing_menu(category: Option<TrapCategory>, message: &[u8]) -> bool {
    match category {
        None => message == b"no such command prefix",
        Some(category) => category == TrapCategory::MissingItemOrCommand,
    }
}

/// Removes the entry of `R`'s menu with `key`, where there is one (removing one that isn't
/// fails).
pub async fn remove_if_present<R>(device: &impl Writes, key: &R::Key) -> Result<(), Error>
where
    R: KeyedResource + Send,
    R::Key: Sync,
{
    if R::fetch(device, key).await?.is_some() {
        apply_mutation(device, &ResourceMutation::remove::<R>(key)).await?;
    }
    Ok(())
}

/// One answer of an action (a monitor, a ping), by field name.
pub type Row = HashMap<Box<str>, Option<Box<str>>>;

pub fn field<'r>(row: &'r Row, name: &str) -> Option<&'r str> {
    row.get(name).and_then(|v| v.as_deref())
}

/// The rows an action answered; a refusal is [`Error::Refused`] naming the command.
pub async fn rows<C: Command<Reply = SimpleResult>>(
    command: &C,
    replies: impl Future<Output = Replies<SimpleResult>>,
) -> Result<Vec<Row>, Error> {
    let mut replies = replies.await;
    let mut rows = Vec::new();
    while let Some(result) = replies.next().await {
        match result {
            SimpleResult::Sentence(row) => rows.push(row),
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { message, .. } => {
                let path = String::from_utf8_lossy(&command.path()).into_owned();
                return Err(Error::Refused(format!("{path}: {message}").into()));
            }
        }
    }
    Ok(rows)
}

/// Sends the action `command`; the rows it answered.
pub async fn act<C: Command<Effect = Write, Reply = SimpleResult> + Sync>(
    device: &impl Writes,
    command: &C,
) -> Result<Vec<Row>, Error> {
    rows(command, device.write(command)).await
}

/// Observes `command` (a monitor once, a ping); the rows it answered.
pub async fn observe<C: Command<Effect = Observe, Reply = SimpleResult> + Sync>(
    device: &impl Reads,
    command: &C,
) -> Result<Vec<Row>, Error> {
    rows(command, device.observe(command)).await
}

/// The ones of `addresses` that answer a ping from the device, all asked at once.
pub async fn answering(device: &impl Reads, addresses: &[IpAddr]) -> BTreeSet<IpAddr> {
    let pings = addresses.iter().map(|address| async move {
        let ping = Ping {
            address: *address,
            count: 3,
            interval: Duration::from_millis(300),
        };
        let answered = tokio::time::timeout(Duration::from_secs(10), observe(device, &ping))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| field(row, "received").is_some_and(|received| received != "0"))
            });
        answered.then_some(*address)
    });
    join_all(pings).await.into_iter().flatten().collect()
}

#[cfg(test)]
mod test;
