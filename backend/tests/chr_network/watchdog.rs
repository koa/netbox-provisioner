//! Scope: CHR tests of suspending watchdogs; nothing else.
//!
//! Suspending a watchdog for an update (`backend::device::watchdog`) on a real CHR:
//! restored by the provisioner it leaves nothing behind; left alone, the scheduler entry
//! restores the watch address by itself; two jobs suspending it at once share it, the last
//! restores it.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::node::{Direct, set_single};
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use crate::chr_network::wait::wait_until;
use backend::device::access::Session;
use backend::device::watchdog::{
    WatchdogRestored, WatchdogStillHeld, WatcherLink, Watching, read, restore, restore_all,
    suspend, suspend_all,
};
use backend::job::{Event, Log};
use mikrotik_model::model::{SystemScheduler, SystemWatchdogCfg};
use mikrotik_model::resource::{SingleResource, collect_resource};
use mikrotik_model::value::HasNone;
use std::net::IpAddr;
use std::time::Duration;

const ENTRY: &str = "netbox-provisioner-watchdog";
/// QEMU's user network gateway: it answers pings, so the watchdog never fires.
const WATCHED: &str = "10.0.2.2";

/// The node's `watch-address`.
async fn watch_address(device: &Session) -> HasNone<IpAddr> {
    SystemWatchdogCfg::fetch(device)
        .await
        .expect("watchdog read")
        .expect("a watchdog")
        .watch_address
}

fn watched() -> HasNone<IpAddr> {
    HasNone::Value(WATCHED.parse().unwrap())
}

/// The scheduler entries named [`ENTRY`].
async fn entries(device: &Session) -> Vec<String> {
    collect_resource::<SystemScheduler>(device)
        .await
        .expect("schedulers")
        .iter()
        .map(|entry| entry.cfg.name.to_string())
        .filter(|name| name == ENTRY)
        .collect()
}

async fn set_watch_address(device: &Session, address: HasNone<IpAddr>) {
    set_single(device, |watchdog: &mut SystemWatchdogCfg| {
        watchdog.watch_address = address
    })
    .await
    .expect("watch-address not set");
}

#[tokio::test]
#[ignore]
async fn watchdog_is_restored_or_restores_itself() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-watchdog-test");
    let node = boot_node(&golden, &work_dir, "watchdog", &[], crate::TEST_PASSWORD)
        .await
        .expect("node did not boot");
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();

    // without a watch address nothing changes
    assert_eq!(watch_address(&device).await, HasNone::NoneValue);
    let nothing = read(&device, &[]).await.unwrap();
    assert!(nothing.is_empty());
    suspend(&device, &nothing, Duration::from_secs(60))
        .await
        .unwrap();
    assert!(entries(&device).await.is_empty());

    // suspended and restored by the provisioner
    set_watch_address(&device, watched()).await;
    let watching = read(&device, &[]).await.unwrap();
    assert_eq!(
        watching,
        Watching {
            watch_address: Some(WATCHED.into()),
            poe_pings: Vec::new(),
        }
    );
    suspend(&device, &watching, Duration::from_secs(600))
        .await
        .unwrap();
    assert_eq!(watch_address(&device).await, HasNone::NoneValue);
    assert_eq!(entries(&device).await, [ENTRY]);
    // a suspend the provisioner didn't see to the end still knows the address
    assert_eq!(read(&device, &[]).await.unwrap(), watching);
    restore(&device, &watching).await.unwrap();
    assert_eq!(watch_address(&device).await, watched());
    assert!(entries(&device).await.is_empty());

    // left alone: the scheduler entry restores it after the safety net and goes
    suspend(&device, &watching, Duration::from_secs(20))
        .await
        .unwrap();
    assert_eq!(watch_address(&device).await, HasNone::NoneValue);
    wait_until(
        || async {
            Ok(watch_address(&device).await == watched() && entries(&device).await.is_empty())
        },
        Duration::from_secs(120),
    )
    .await
    .expect("the scheduler entry did not restore the watch address");
}

/// Two updates behind the same switch suspend its watchdog at once (a rollout, Berg 925): both
/// get it suspended, the first to end leaves it so, the last restores it.
#[tokio::test]
#[ignore]
async fn two_jobs_share_a_suspended_watchdog() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-watchdog-shared-test");
    let node = boot_node(&golden, &work_dir, "shared", &[], crate::TEST_PASSWORD)
        .await
        .expect("node did not boot");
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    set_watch_address(&device, watched()).await;
    let watcher = || {
        vec![WatcherLink {
            device: "shared".into(),
            supplier: true,
            powered: Box::default(),
            link: Ok(Direct(node.api_port)),
        }]
    };
    let (first_log, second_log) = (Log::new("first"), Log::new("second"));
    let (first, second) = tokio::join!(
        suspend_all(watcher(), &first_log),
        suspend_all(watcher(), &second_log)
    );
    let (first, second) = (first.expect("first"), second.expect("second"));
    let watching = Watching {
        watch_address: Some(WATCHED.into()),
        poe_pings: Vec::new(),
    };
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(watch_address(&device).await, HasNone::NoneValue);
    assert_eq!(entries(&device).await, [ENTRY]);

    restore_all(first, &first_log).await;
    assert!(
        first_log
            .events()
            .contains(&Event::WatchdogStillHeld(WatchdogStillHeld {
                device: "shared".into(),
                others: 1,
            })),
        "{:?}",
        first_log.events()
    );
    assert_eq!(watch_address(&device).await, HasNone::NoneValue);
    assert_eq!(entries(&device).await, [ENTRY]);

    restore_all(second, &second_log).await;
    assert!(
        second_log
            .events()
            .contains(&Event::WatchdogRestored(WatchdogRestored {
                device: "shared".into(),
                watching,
            })),
        "{:?}",
        second_log.events()
    );
    assert_eq!(watch_address(&device).await, watched());
    assert!(entries(&device).await.is_empty());
}
