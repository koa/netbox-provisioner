//! Polling assertion helpers. Always used over direct access (never through
//! the realistic SOCKS path in `apply.rs`) - waiting for network state to
//! converge is setup/validation, not the access pattern under test.

use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;
use tokio_stream::StreamExt;

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
/// No typed `/routing/ospf/neighbor` model exists in mikrotik-model yet, so
/// this uses a raw `SimpleResult` command rather than the typed fetch layer.
pub async fn wait_for_ospf_full(
    device: &MikrotikDevice<SimpleResult>,
    instance: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    wait_until(
        || async {
            let mut stream = device
                .send_simple_command(b"/routing/ospf/neighbor/print".as_slice(), ())
                .await;
            let mut rows = Vec::new();
            while let Some(result) = stream.next().await {
                match result {
                    SimpleResult::Sentence(row) => rows.push(row),
                    SimpleResult::Trap { message, .. } => {
                        anyhow::bail!("ospf neighbor print failed: {message}")
                    }
                    SimpleResult::Error(e) => return Err(e.into()),
                }
            }
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|row| row.get("instance").and_then(|v| v.as_deref()) == Some(instance))
                .collect();
            Ok(!rows.is_empty()
                && rows
                    .iter()
                    .all(|row| row.get("state").and_then(|v| v.as_deref()) == Some("Full")))
        },
        timeout,
    )
    .await
}
