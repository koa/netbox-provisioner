//! The fallback of an apply (`backend::device::ros::fallback`) on a real CHR: stopped, it
//! leaves nothing behind; left alone, it loads the backup from before and the device reboots
//! with the old config.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use crate::chr_network::wait::wait_until;
use backend::device::ros::fallback::{arm, disarm};
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio_stream::StreamExt;

/// The `name`s under `path` (`/system/scheduler`, `/file`).
async fn names(device: &MikrotikDevice<SimpleResult>, path: &str) -> Vec<String> {
    let command = format!("{path}/print");
    let mut stream = device.send_simple_command(command.as_bytes(), ()).await;
    let mut names = Vec::new();
    while let Some(result) = stream.next().await {
        if let SimpleResult::Sentence(row) = result
            && let Some(Some(name)) = row.get("name")
        {
            names.push(name.to_string());
        }
    }
    names
}

async fn identity(device: &MikrotikDevice<SimpleResult>) -> String {
    names(device, "/system/identity").await.concat()
}

async fn set_identity(device: &MikrotikDevice<SimpleResult>, name: &str) {
    let mut stream = device
        .send_command(
            b"/system/identity/set".as_slice(),
            |cmd| cmd.attribute(b"name".as_slice(), name.as_bytes()),
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        if let SimpleResult::Trap { message, .. } = result {
            panic!("identity not set: {message}");
        }
    }
}

const FALLBACK: &str = "netbox-provisioner-fallback";

#[tokio::test]
#[ignore]
async fn fallback_is_stopped_or_restores() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-fallback-test");
    let node = boot_node(&golden, &work_dir, "fallback", &[], crate::TEST_PASSWORD)
        .await
        .expect("node did not boot");
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    let before = identity(&device).await;

    // stopped: no scheduler, no backup left
    arm(&device, Duration::from_secs(60)).await.unwrap();
    assert_eq!(names(&device, "/system/scheduler").await, [FALLBACK]);
    assert!(names(&device, "/file").await.contains(&format!("{FALLBACK}.backup")));
    disarm(&device).await.unwrap();
    assert!(names(&device, "/system/scheduler").await.is_empty());
    assert!(!names(&device, "/file").await.contains(&format!("{FALLBACK}.backup")));

    // left alone: the config from before comes back with a reboot
    arm(&device, Duration::from_secs(10)).await.unwrap();
    set_identity(&device, "changed-by-apply").await;
    assert_eq!(identity(&device).await, "changed-by-apply");
    drop(device);
    wait_until(
        || async {
            let Ok(device) = node.connect_api(crate::TEST_PASSWORD).await else {
                return Ok(false);
            };
            Ok(identity(&device).await == before)
        },
        Duration::from_secs(240),
    )
    .await
    .expect("the backup was not loaded");
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    // the scheduler was saved before it was added, the backup stays until the next apply
    assert!(names(&device, "/system/scheduler").await.is_empty());
}
