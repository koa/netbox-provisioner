//! Scope: Polling a node until a condition holds; nothing else.
//!
//! Polling assertion helpers. Always used over direct access (never through
//! the realistic SOCKS path in `apply.rs`) - waiting for network state to
//! converge is setup/validation, not the access pattern under test.

use backend::device::access::Session;
use mikrotik_model::model::{OspfNeighborState, RoutingOspfNeighborState};
use mikrotik_model::resource::collect_resource;
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// Polls `check` until it returns `Ok(true)` or `timeout` elapses. An
/// `Err` from `check` itself is propagated immediately rather than retried -
/// that indicates a real failure (e.g. a dropped connection), not "not
/// converged yet".
pub async fn wait_until<F, Fut>(mut check: F, timeout: Duration) -> anyhow::Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<bool>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if check().await? {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "condition not met within {timeout:?}"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Waits for every OSPF neighbor `device` has under the given `instance`
/// (the generator always sets up both a `default-v2` and a `default-v3`
/// instance whenever any interface has OSPF enabled, regardless of whether
/// IPv6 is actually in play - a v6-only scenario would need to wait on
/// `default-v3` instead) to reach `Full`, and for there to be at least one.
pub async fn wait_for_ospf_full(
    device: &Session,
    instance: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    wait_until(
        || async {
            let neighbors = collect_resource::<RoutingOspfNeighborState>(device).await?;
            let neighbors = neighbors
                .iter()
                .filter(|neighbor| *neighbor.instance.0 == *instance.as_bytes())
                .collect::<Vec<_>>();
            Ok(!neighbors.is_empty()
                && neighbors
                    .iter()
                    .all(|neighbor| neighbor.state == OspfNeighborState::Full))
        },
        timeout,
    )
    .await
}
