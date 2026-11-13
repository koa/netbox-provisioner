//! Scope: who may send what to a device ([`Reader`], [`Session`]) and the replies of the
//! actions; not how a connection is made (`device`), not what is read (the submodules).
//!
//! The facades of `docs/device-access.md`: a [`Reader`] sends only what changes
//! nothing (`print`, monitors, ping), a [`Session`] is a job's own connection and also writes.
//! Both go over a `mikrotik_api` [`Connection`]; each command is a type of mikrotik-model (a
//! menu's model, an action of `mikrotik_model::action`), none a string.
use crate::{Error, topology::DeviceId};
use async_graphql::futures_util::StreamExt;
use async_graphql::futures_util::future::join_all;
use mikrotik_api::{
    command::{Observe, Read, SessionBound, Write},
    prelude::{Command, Connection, Generation, Replies, TrapCategory},
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
    sync::{LazyLock, Mutex},
    time::Duration,
};

/// Reads from a device: what it may send changes nothing. Cheap to clone; all clones share the
/// login.
#[derive(Clone)]
pub struct Reader {
    connection: Connection,
}

impl Reader {
    pub(crate) fn new(connection: Connection) -> Self {
        Reader { connection }
    }
    /// The login is gone (the device rebooted, a tunnel closed).
    pub fn is_closed(&self) -> bool {
        self.connection.is_closed()
    }
}

impl Reads for Reader {
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

/// A job's own connection to a device: it reads, writes and holds what lasts as long as the
/// connection (the device mode's countdown). Its lifetime means something (an apply cuts its
/// path, a reboot ends it), so a job opens its own (`AccessibleDevice::connect_again`).
pub struct Session {
    connection: Connection,
}

impl Session {
    /// The provisioner takes one only with the device's lock (`job::start*`); the CHR harness,
    /// which has no jobs, takes its own.
    pub fn new(connection: Connection) -> Self {
        Session { connection }
    }
    /// The same connection, to hand to what only reads.
    pub fn reader(&self) -> Reader {
        Reader::new(self.connection.clone())
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

/// The write counter of `device`, shared by all its connections: every write and session bound
/// command raises it (what was read before is stale then).
pub(crate) fn generation(device: DeviceId) -> Generation {
    static GENERATIONS: LazyLock<Mutex<HashMap<DeviceId, Generation>>> =
        LazyLock::new(Default::default);
    GENERATIONS
        .lock()
        .expect("generations poisoned")
        .entry(device)
        .or_default()
        .clone()
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
mod test {
    use super::is_missing_menu;
    use mikrotik_api::prelude::TrapCategory;

    /// A missing package's menu has no rows; another refusal stays an error.
    #[test]
    fn a_missing_menu_is_told_apart() {
        assert!(is_missing_menu(None, b"no such command prefix"));
        assert!(is_missing_menu(
            Some(TrapCategory::MissingItemOrCommand),
            b""
        ));
        assert!(!is_missing_menu(None, b"not enough permissions (9)"));
    }
}
