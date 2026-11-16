//! Scope: One GraphQL operation (a query, a mutation, a round of a subscription): the topology
//! and HTTP answers it works on and what it sent to the devices, counted and logged at its end; not the sending
//! (`device::access`) nor what is kept of it (`facts`).
//!
//! The topology is the one the operation got first ([`topology`]): every resolver sees the
//! same, read from NetBox at most once, however long the operation takes; the next operation
//! or round reads anew. So is what it reads over HTTP, by URL: NetBox's REST answers to a
//! reader ([`netbox`]) and the sizes of MikroTik's packages ([`package_size`]).
//!
//! The measurement of `docs/concepts/device-access.md` („Prüfen“): every command a [`Reader`]
//! sends within an operation is noted by device and path; at the end the operation logs on
//! `debug` how many it sent and every path sent more than once to the same device. A fact asked
//! within an operation counts for it, also in the task `facts::get` spawns for the asking
//! ([`within`]). Jobs run outside any operation and aren't counted.
//!
//! [`Reader`]: crate::device::access::Reader
use crate::{
    lock::LockAnyway,
    topology::{DeviceId, Topology},
};
use async_graphql::{
    Response,
    async_trait::async_trait,
    extensions::{Extension, ExtensionContext, ExtensionFactory, NextExecute, NextSubscribe},
    futures_util::stream::{BoxStream, Stream, StreamExt},
};
use log::{debug, log_enabled};
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

/// What an operation works on and sent, by device and path.
#[derive(Debug, Default)]
pub struct Operation {
    name: Box<str>,
    sent: Mutex<BTreeMap<Sent, u32>>,
    /// the topology it got first, of the holder that gave it
    topology: tokio::sync::OnceCell<Pinned>,
    /// NetBox's REST answers it read, by URL
    netbox: Answers<serde_json::Value>,
    /// the sizes of MikroTik's package files it asked, by URL
    package_sizes: Answers<Option<u64>>,
}

/// What was read over HTTP, by URL; a failed reading isn't kept, the next asker reads again.
type Answers<T> = Mutex<HashMap<Box<str>, Arc<tokio::sync::OnceCell<T>>>>;

#[derive(Debug)]
struct Pinned {
    /// the holder's data, by address: the server has one, a test may have several
    holder: usize,
    topology: Option<Arc<Topology>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Sent {
    device: DeviceId,
    path: Box<str>,
}

tokio::task_local! {
    static OPERATION: Arc<Operation>;
}

impl Operation {
    fn new(name: Option<&str>) -> Arc<Self> {
        Arc::new(Operation {
            name: name.unwrap_or("(unnamed)").into(),
            sent: Mutex::default(),
            topology: tokio::sync::OnceCell::new(),
            netbox: Mutex::default(),
            package_sizes: Mutex::default(),
        })
    }
    /// How often each path went to each device.
    fn counts(&self) -> Vec<(DeviceId, Box<str>, u32)> {
        self.sent
            .lock_anyway()
            .iter()
            .map(|(sent, count)| (sent.device, sent.path.clone(), *count))
            .collect()
    }
    /// Logs what it sent: the number, and each path sent to a device more than once.
    fn report(&self) {
        if !log_enabled!(log::Level::Debug) {
            return;
        }
        let counts = self.counts();
        let total = counts.iter().map(|(.., count)| count).sum::<u32>();
        if total == 0 {
            return;
        }
        let devices = counts
            .iter()
            .map(|(device, ..)| device)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        debug!(
            "operation {}: {total} commands to {devices} devices",
            self.name
        );
        for (device, path, count) in counts.iter().filter(|(.., count)| *count > 1) {
            debug!(
                "operation {}: {path} ×{count} to device {}",
                self.name, device.0
            );
        }
    }
}

/// Notes that `path` went to `device` in the operation running, if any.
pub fn note(device: DeviceId, path: &[u8]) {
    let _ = OPERATION.try_with(|operation| {
        let path = String::from_utf8_lossy(path).into();
        *operation
            .sent
            .lock_anyway()
            .entry(Sent { device, path })
            .or_default() += 1;
    });
}

/// The topology of the holder `holder` (its data's address) for the operation running: the one
/// it got first, `load`ed on the first ask; outside an operation (a job), or from another holder
/// than the first, `load`ed each time.
pub async fn topology(
    holder: usize,
    load: impl Future<Output = Option<Arc<Topology>>>,
) -> Option<Arc<Topology>> {
    let Some(operation) = current() else {
        return load.await;
    };
    let mut load = Some(load);
    let pinned = operation
        .topology
        .get_or_init(|| async {
            let load = load.take().expect("loaded once");
            Pinned {
                holder,
                topology: load.await,
            }
        })
        .await;
    match load {
        // the cell was another holder's: this one's isn't kept
        Some(load) if pinned.holder != holder => load.await,
        _ => pinned.topology.clone(),
    }
}

/// NetBox's answer to the GET of `url` for the operation running: the one it got first,
/// `load`ed on the first ask; outside an operation (a job) `load`ed each time.
pub async fn netbox<E>(
    url: &str,
    load: impl Future<Output = Result<serde_json::Value, E>>,
) -> Result<serde_json::Value, E> {
    let Some(operation) = current() else {
        return load.await;
    };
    once_per_url(&operation.netbox, url, load).await
}

/// The size of MikroTik's package file at `url` for the operation running, as [`netbox`].
pub async fn package_size<E>(
    url: &str,
    load: impl Future<Output = Result<Option<u64>, E>>,
) -> Result<Option<u64>, E> {
    let Some(operation) = current() else {
        return load.await;
    };
    once_per_url(&operation.package_sizes, url, load).await
}

async fn once_per_url<T: Clone, E>(
    answers: &Answers<T>,
    url: &str,
    load: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let cell = answers.lock_anyway().entry(url.into()).or_default().clone();
    cell.get_or_try_init(|| load).await.cloned()
}

/// The operation running, to hand to a task spawned for it ([`within`]).
pub fn current() -> Option<Arc<Operation>> {
    OPERATION.try_with(Arc::clone).ok()
}

/// Runs `work` within `operation` (none: outside any).
pub async fn within<F: Future>(operation: Option<Arc<Operation>>, work: F) -> F::Output {
    match operation {
        Some(operation) => OPERATION.scope(operation, work).await,
        None => work.await,
    }
}

/// Runs `work` as an operation of its own and gives what it sent, by device and path: for tests.
#[cfg(test)]
pub async fn counted<F: Future>(work: F) -> (F::Output, Vec<(DeviceId, Box<str>, u32)>) {
    let operation = Operation::new(Some("test"));
    let output = OPERATION.scope(operation.clone(), work).await;
    (output, operation.counts())
}

/// The async-graphql extension putting each operation into its own [`Operation`]: a query or
/// mutation as a whole, a subscription per answer it gives.
pub struct OperationExtension;

impl ExtensionFactory for OperationExtension {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(OperationExtension)
    }
}

#[async_trait]
impl Extension for OperationExtension {
    async fn execute(
        &self,
        ctx: &ExtensionContext<'_>,
        operation_name: Option<&str>,
        next: NextExecute<'_>,
    ) -> Response {
        let operation = Operation::new(operation_name);
        let response = OPERATION
            .scope(operation.clone(), next.run(ctx, operation_name))
            .await;
        operation.report();
        response
    }
    fn subscribe<'s>(
        &self,
        ctx: &ExtensionContext<'_>,
        stream: BoxStream<'s, Response>,
        next: NextSubscribe<'_>,
    ) -> BoxStream<'s, Response> {
        Rounds {
            operation: Operation::new(Some("subscription")),
            inner: next.run(ctx, stream),
        }
        .boxed()
    }
}

/// A subscription's stream, each answer an operation of its own.
struct Rounds<'s> {
    operation: Arc<Operation>,
    inner: BoxStream<'s, Response>,
}

impl Stream for Rounds<'_> {
    type Item = Response;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Response>> {
        let operation = self.operation.clone();
        let polled = OPERATION.sync_scope(operation, || self.inner.poll_next_unpin(cx));
        if let Poll::Ready(Some(_)) = &polled {
            self.operation.report();
            self.operation = Operation::new(Some("subscription"));
        }
        polled
    }
}

#[cfg(test)]
mod test {
    use super::{counted, note};
    use crate::{
        facts::{self, FactKey, FactKind, Freshness},
        topology::DeviceId,
    };
    use std::time::Duration;

    /// A fact is asked in a task of its own (`facts::get`): what it sends counts for the
    /// operation that asked, twice the same path twice.
    #[tokio::test]
    async fn a_fact_asked_counts_for_the_operation() {
        let ((), sent) = counted(async {
            let key = FactKey::new(FactKind::Menu, Some(DeviceId(7)), "");
            let fact = facts::get(key, Freshness::MaxAge(Duration::ZERO), async {
                note(DeviceId(7), b"/system/resource/print");
                note(DeviceId(7), b"/system/resource/print");
                note(DeviceId(7), b"/system/health/print");
                Ok(1u32)
            })
            .await;
            assert!(fact.value.is_ok());
        })
        .await;
        assert_eq!(
            sent,
            [
                (DeviceId(7), "/system/health/print".into(), 1),
                (DeviceId(7), "/system/resource/print".into(), 2),
            ]
        );
    }

    /// Within an operation the topology is loaded once and the same for every ask; outside
    /// (a job) each ask loads.
    #[tokio::test]
    async fn an_operation_keeps_the_topology_it_got_first() {
        use super::topology;
        use crate::topology::builder::TopologyBuilder;
        use std::sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        };
        let loads = AtomicU32::new(0);
        let load = || async {
            loads.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new(TopologyBuilder::default().build()))
        };
        let ((first, second, other), _) = counted(async {
            let first = topology(1, load()).await;
            let second = topology(1, load()).await;
            // another holder's is loaded, not kept
            let other = topology(2, load()).await;
            (first, second, other)
        })
        .await;
        assert!(Arc::ptr_eq(
            first.as_ref().unwrap(),
            second.as_ref().unwrap()
        ));
        assert!(other.is_some());
        assert_eq!(loads.load(Ordering::SeqCst), 2);
        topology(1, load()).await;
        topology(1, load()).await;
        assert_eq!(loads.load(Ordering::SeqCst), 4);
    }

    /// NetBox is asked once per URL within an operation, again after a failure; outside one
    /// each time.
    #[tokio::test]
    async fn an_operation_reads_each_url_once() {
        use super::netbox;
        use serde_json::{Value, json};
        use std::sync::atomic::{AtomicU32, Ordering};
        let counter = AtomicU32::new(0);
        let loads = &counter;
        let load = |ok: bool| async move {
            loads.fetch_add(1, Ordering::SeqCst);
            match ok {
                true => Ok(json!({ "id": 1 })),
                false => Err(()),
            }
        };
        let (answers, _) = counted(async {
            let failed = netbox("dcim/devices/1/", load(false)).await;
            let first = netbox("dcim/devices/1/", load(true)).await;
            let second = netbox("dcim/devices/1/", load(true)).await;
            let other = netbox("dcim/devices/2/", load(true)).await;
            [failed, first, second, other]
        })
        .await;
        let ok = Ok::<Value, ()>(json!({ "id": 1 }));
        assert_eq!(answers, [Err(()), ok.clone(), ok.clone(), ok]);
        assert_eq!(loads.load(Ordering::SeqCst), 3);
        netbox("dcim/devices/1/", load(true)).await.unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 4);
    }

    /// Outside any operation nothing is noted (a job).
    #[tokio::test]
    async fn outside_nothing_counts() {
        note(DeviceId(7), b"/system/resource/print");
        let ((), sent) = counted(async {}).await;
        assert!(sent.is_empty());
    }
}
