//! Queries kept live (`docs/concepts/device-state.md`): a query sent over the WebSocket is
//! answered, then answered again whenever what it read changed: a job on a device it showed
//! ended or logged, the topology changed in NetBox, or the youngest age it took a fact at ran
//! out. The same query as over HTTP, with the same selection and the same resolvers, so it
//! shows what they would. What it read is noted while it runs (`facts::reading`).
use super::AuthenticatedGraphqlSchema;
use crate::{
    context::UserInfo,
    facts::{self, Reading},
    topology::TopologyHolder,
};
use async_graphql::{
    Data, Executor, Request, Response, Variables,
    futures_util::stream::{self, BoxStream, StreamExt},
    parser::{parse_query, types::OperationType},
};
use std::{
    any::TypeId,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::broadcast::{Receiver, error::RecvError};

/// How long a live query waits after a change for more: a job logs several events at once.
const SETTLE: Duration = Duration::from_millis(300);

/// The live queries open.
static OPEN: AtomicUsize = AtomicUsize::new(0);

/// How often NetBox is read while a live query is open.
const NETBOX_EVERY: Duration = Duration::from_secs(10);

/// Reads NetBox regularly while a live query is open, so a change there shows within
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

/// Counts a live query open while it lives.
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

/// Runs queries over HTTP once and over the WebSocket live.
#[derive(Clone)]
pub struct LiveExecutor(pub AuthenticatedGraphqlSchema);

impl Executor for LiveExecutor {
    fn execute(&self, request: Request) -> impl Future<Output = Response> + Send {
        let schema = self.0.clone();
        async move { schema.execute(request).await }
    }

    fn execute_stream(&self, request: Request, session_data: Option<Arc<Data>>) -> BoxStream<'static, Response> {
        let user = session_data
            .as_ref()
            .and_then(|data| data.get(&TypeId::of::<UserInfo>()))
            .and_then(|user| user.downcast_ref::<UserInfo>())
            .cloned();
        let live = Live {
            schema: self.0.clone(),
            query: request.query,
            operation_name: request.operation_name,
            variables: request.variables,
            user,
            changes: facts::changes(),
            started: 0,
            reading: Default::default(),
            _open: Open::new(),
        };
        // a mutation goes once: running it again would do it again
        if !live.is_query() {
            return stream::once(async move { live.schema.execute(live.request()).await }).boxed();
        }
        stream::unfold((live, true), |(mut live, first)| async move {
            if !first {
                live.wait().await;
            }
            let response = live.run().await;
            Some((response, (live, false)))
        })
        .boxed()
    }
}

struct Live {
    schema: AuthenticatedGraphqlSchema,
    query: String,
    operation_name: Option<String>,
    variables: Variables,
    user: Option<UserInfo>,
    /// subscribed before the first run, so no change after it is missed
    changes: Receiver<(u64, Change)>,
    /// the number of the last change before the latest run began
    started: u64,
    reading: Arc<Mutex<Reading>>,
    _open: Open,
}

use facts::Change;

impl Live {
    fn request(&self) -> Request {
        let request = Request::new(self.query.clone()).variables(self.variables.clone());
        let request = match &self.operation_name {
            Some(name) => request.operation_name(name.clone()),
            None => request,
        };
        match &self.user {
            Some(user) => request.data(user.clone()),
            None => request,
        }
    }
    /// Whether the operation run is a query (an unparsable one fails as a query would).
    fn is_query(&self) -> bool {
        let Ok(document) = parse_query(&self.query) else {
            return true;
        };
        document.operations.iter().all(|(name, operation)| {
            let chosen = match (&self.operation_name, name) {
                (Some(wanted), Some(name)) => wanted.as_str() == name.as_str(),
                _ => true,
            };
            !chosen || operation.node.ty == OperationType::Query
        })
    }
    async fn run(&mut self) -> Response {
        self.reading = Default::default();
        self.started = facts::sequence();
        facts::reading(self.reading.clone(), self.schema.execute(self.request())).await
    }
    /// Until something it read changed (and a moment for more), or got older than it was taken.
    async fn wait(&mut self) {
        let outdated = self.reading.lock().unwrap_or_else(PoisonError::into_inner).outdated;
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
                change = self.changes.recv() => match change {
                    Ok((number, change)) => {
                        let concerns = number > self.started
                            && self.reading.lock().unwrap_or_else(PoisonError::into_inner).concerns(&change);
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
        while self.changes.try_recv().is_ok() {}
    }
}
