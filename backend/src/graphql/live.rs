//! Scope: Keeping a subscription live: its rounds, what they read and when to answer again; not the
//! fields it serves (`graphql`).
//!
//! Subscriptions (`docs/device-state.md`): `SubscriptionAuthenticated` has the fields
//! of the query with the same types, each a stream of [`rounds`]. It gives the object, the
//! selection under it is resolved by the same resolvers as in the query, and it gives it again
//! whenever what they read changed: a job on a device shown ended or logged, facts read were
//! dropped, the topology changed in NetBox, or the youngest age a fact was taken at ran out.
//! What they read is noted while the subscription's stream is polled ([`LiveExtension`]).
use super::AuthenticatedGraphqlSchema;
use crate::{
    facts::{self, Change, Reading},
    lock::LockAnyway,
    topology::TopologyHolder,
};
use async_graphql::{
    Request, Response,
    extensions::{Extension, ExtensionContext, ExtensionFactory, NextSubscribe},
    futures_util::stream::{self, BoxStream, Stream, StreamExt},
    parser::types::OperationType,
};
use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::broadcast::{Receiver, error::RecvError};

/// How long a subscription waits after a change for more: a job logs several events at once.
const SETTLE: Duration = Duration::from_millis(300);
/// How often NetBox is read while a subscription is open.
const NETBOX_EVERY: Duration = Duration::from_secs(10);

/// The subscriptions open.
static OPEN: AtomicUsize = AtomicUsize::new(0);

/// Reads NetBox regularly while a subscription is open, so a change there shows within
/// [`NETBOX_EVERY`] (a topology that changed is announced); without, nobody would read it until
/// what a page shows got old.
pub fn watch_netbox(topology: TopologyHolder) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(NETBOX_EVERY);
        loop {
            interval.tick().await;
            if OPEN.load(Ordering::Relaxed) > 0 {
                topology.current().await;
            }
        }
    });
}

/// Counts a subscription open while it lives.
struct Open;
impl Open {
    fn new() -> Self {
        OPEN.fetch_add(1, Ordering::Relaxed);
        Open
    }
}
impl Drop for Open {
    fn drop(&mut self) {
        OPEN.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Answers `request` once, as over HTTP: a subscription with its first answer, so that a view
/// asking once shares the subscription's document (and the types generated from it) with the
/// views keeping it live.
pub async fn execute_once(schema: &AuthenticatedGraphqlSchema, mut request: Request) -> Response {
    if is_subscription(&mut request) {
        schema
            .execute_stream(request)
            .next()
            .await
            .unwrap_or_default()
    } else {
        schema.execute(request).await
    }
}

/// Whether the operation `request` runs is a subscription (an unparsable one fails as a query).
fn is_subscription(request: &mut Request) -> bool {
    let name = request.operation_name.clone();
    let Ok(document) = request.parsed_query() else {
        return false;
    };
    let mut operations = document.operations.iter();
    let operation = match &name {
        Some(name) => operations
            .find(|(operation, _)| operation.is_some_and(|operation| operation.as_str() == name)),
        None => operations.next(),
    };
    operation.is_some_and(|(_, operation)| operation.node.ty == OperationType::Subscription)
}

/// Polls each subscription's stream noting what it reads (`facts::reading_in`): the field's
/// stream, and the resolvers of the selection under each object it gives.
pub struct LiveExtension;

impl ExtensionFactory for LiveExtension {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(LiveExtension)
    }
}

impl Extension for LiveExtension {
    fn subscribe<'s>(
        &self,
        ctx: &ExtensionContext<'_>,
        stream: BoxStream<'s, Response>,
        next: NextSubscribe<'_>,
    ) -> BoxStream<'s, Response> {
        Noting {
            reading: Default::default(),
            inner: next.run(ctx, stream),
        }
        .boxed()
    }
}

struct Noting<'s> {
    reading: Arc<Mutex<Reading>>,
    inner: BoxStream<'s, Response>,
}

impl Stream for Noting<'_> {
    type Item = Response;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Response>> {
        let reading = self.reading.clone();
        facts::reading_in(reading, || self.inner.poll_next_unpin(cx))
    }
}

/// A subscription field's stream: the value `make` gives, again whenever what was read for the
/// answer before changed (or got older than taken). Outside a subscription ([`LiveExtension`])
/// it gives the value once.
pub fn rounds<T, F, Fut>(make: F) -> impl Stream<Item = T> + Send
where
    T: Send,
    F: FnMut() -> Fut + Send,
    Fut: Future<Output = T> + Send,
{
    let state = Rounds {
        make,
        changes: facts::changes(),
        started: None,
        _open: Open::new(),
    };
    stream::unfold(state, |mut state| async move {
        if let Some(started) = state.started {
            let reading = facts::current_reading()?;
            wait(&reading, started, &mut state.changes).await;
        }
        if let Some(reading) = facts::current_reading() {
            let mut reading = reading.lock_anyway();
            *reading = Reading {
                delivered: std::mem::take(&mut reading.delivered),
                ..Reading::default()
            };
        }
        state.started = Some(facts::sequence());
        let value = (state.make)().await;
        Some((value, state))
    })
}

struct Rounds<F> {
    make: F,
    /// subscribed before the first round, so no change after it is missed
    changes: Receiver<(u64, Change)>,
    /// the number of the last change before the latest round began, `None` before the first
    started: Option<u64>,
    _open: Open,
}

/// Until something read changed (and a moment for more), or got older than it was taken.
async fn wait(reading: &Mutex<Reading>, started: u64, changes: &mut Receiver<(u64, Change)>) {
    let outdated = reading.lock_anyway().outdated;
    let deadline = async {
        match outdated {
            Some(at) => tokio::time::sleep_until(at).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            () = &mut deadline => return,
            change = changes.recv() => match change {
                Ok((number, change)) => {
                    let concerns = number > started
                        && reading.lock_anyway().concerns(&change);
                    if concerns {
                        break;
                    }
                }
                // missed some: they may have concerned it
                Err(RecvError::Lagged(_)) => break,
                Err(RecvError::Closed) => std::future::pending::<()>().await,
            },
        }
    }
    tokio::time::sleep(SETTLE).await;
    while changes.try_recv().is_ok() {}
}
