//! Suspending a watchdog for an update (`backend::device::watchdog`) on a real CHR:
//! restored by the provisioner it leaves nothing behind; left alone, the scheduler entry
//! restores the watch address by itself.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use crate::chr_network::wait::wait_until;
use backend::device::watchdog::{Watching, read, restore, suspend};
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio_stream::StreamExt;

const ENTRY: &str = "netbox-provisioner-watchdog";
/// QEMU's user network gateway: it answers pings, so the watchdog never fires.
const WATCHED: &str = "10.0.2.2";

/// The rows of `path`, each as its `field`.
async fn column(device: &MikrotikDevice<SimpleResult>, path: &str, field: &str) -> Vec<String> {
    let command = format!("{path}/print");
    let mut stream = device.send_simple_command(command.as_bytes(), ()).await;
    let mut values = Vec::new();
    while let Some(result) = stream.next().await {
        if let SimpleResult::Sentence(row) = result
            && let Some(Some(value)) = row.get(field)
        {
            values.push(value.to_string());
        }
    }
    values
}

async fn watch_address(device: &MikrotikDevice<SimpleResult>) -> String {
    column(device, "/system/watchdog", "watch-address").await.concat()
}

async fn entries(device: &MikrotikDevice<SimpleResult>) -> Vec<String> {
    column(device, "/system/scheduler", "name")
        .await
        .into_iter()
        .filter(|name| name == ENTRY)
        .collect()
}

async fn set_watch_address(device: &MikrotikDevice<SimpleResult>, address: &str) {
    let mut stream = device
        .send_command(
            b"/system/watchdog/set".as_slice(),
            |cmd| cmd.attribute(b"watch-address".as_slice(), address.as_bytes()),
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        if let SimpleResult::Trap { message, .. } = result {
            panic!("watch-address not set: {message}");
        }
    }
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
    assert_eq!(watch_address(&device).await, "none");
    let nothing = read(&device, &[]).await.unwrap();
    assert!(nothing.is_empty());
    suspend(&device, &nothing, Duration::from_secs(60)).await.unwrap();
    assert!(entries(&device).await.is_empty());

    // suspended and restored by the provisioner
    set_watch_address(&device, WATCHED).await;
    let watching = read(&device, &[]).await.unwrap();
    assert_eq!(
        watching,
        Watching {
            watch_address: Some(WATCHED.into()),
            poe_pings: Vec::new(),
        }
    );
    suspend(&device, &watching, Duration::from_secs(600)).await.unwrap();
    assert_eq!(watch_address(&device).await, "none");
    assert_eq!(entries(&device).await, [ENTRY]);
    // a suspend the provisioner didn't see to the end still knows the address
    assert_eq!(read(&device, &[]).await.unwrap(), watching);
    restore(&device, &watching).await.unwrap();
    assert_eq!(watch_address(&device).await, WATCHED);
    assert!(entries(&device).await.is_empty());

    // left alone: the scheduler entry restores it after the safety net and goes
    suspend(&device, &watching, Duration::from_secs(20)).await.unwrap();
    assert_eq!(watch_address(&device).await, "none");
    wait_until(
        || async { Ok(watch_address(&device).await == WATCHED && entries(&device).await.is_empty()) },
        Duration::from_secs(120),
    )
    .await
    .expect("the scheduler entry did not restore the watch address");
}
