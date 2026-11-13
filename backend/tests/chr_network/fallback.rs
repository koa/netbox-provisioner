//! Scope: CHR tests of an apply's fallback; nothing else.
//!
//! The fallback of an apply (`backend::device::ros::fallback`) on a real CHR: stopped, it
//! leaves nothing behind; left alone, it loads the backup from before and the device reboots
//! with the old config.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::node::{add_script, execute, identity, set_identity};
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use crate::chr_network::wait::wait_until;
use backend::device::access::Session;
use backend::device::ros::fallback::{NAME, Pending, arm, disarm, pending};
use backend::device::ros::progress::{
    Instrument, file_path, instrument, provisioner_address, read,
};
use mikrotik_model::model::{File, SystemScheduler};
use mikrotik_model::resource::collect_resource;
use std::time::Duration;

/// The names of the scheduler entries.
async fn schedulers(device: &Session) -> Vec<String> {
    collect_resource::<SystemScheduler>(device)
        .await
        .expect("schedulers")
        .iter()
        .map(|entry| entry.cfg.name.to_string())
        .collect()
}

/// The names of the files.
async fn files(device: &Session) -> Vec<String> {
    collect_resource::<File>(device)
        .await
        .expect("files")
        .iter()
        .map(|file| file.cfg.name.to_string())
        .collect()
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
    assert_eq!(schedulers(&device).await, [FALLBACK]);
    assert!(files(&device).await.contains(&format!("{FALLBACK}.backup")));
    disarm(&device).await.unwrap();
    assert!(schedulers(&device).await.is_empty());
    assert!(!files(&device).await.contains(&format!("{FALLBACK}.backup")));

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
    assert!(schedulers(&device).await.is_empty());
}

/// The marks of an apply script (`backend::device::ros::progress`) on a real CHR: a script
/// that takes the device's address away (as an apply that cuts the device off), the ping to
/// the provisioner stops after that line, the end brings the fallback forward, and the marks
/// outlast the reboot of the fallback, read once the device is back.
#[tokio::test]
#[ignore]
async fn marks_outlast_the_fallback() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-marks-test");
    let node = boot_node(&golden, &work_dir, "marks", &[], crate::TEST_PASSWORD)
        .await
        .expect("node did not boot");
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    let before = identity(&device).await;

    arm(&device, Duration::from_secs(600)).await.unwrap();
    let probe = provisioner_address(&device).await.unwrap();
    assert!(probe.is_some(), "no address of the provisioner");
    // line 4 takes the address of ether1 away, the only way to the provisioner (QEMU's
    // gateway); a drop rule in `output` isn't one, the ping right after it still answers at
    // times (7.24.5)
    let script = "/system/identity\nset name=changed-by-apply\n/ip/dhcp-client\n\
                  set [ find interface=ether1 ] disabled=yes\n/system/identity\nset name=x\n";
    let file = file_path(&device).await.unwrap();
    let source = instrument(
        script,
        &Instrument {
            file: &file,
            run: "t1",
            lead: Duration::from_secs(1),
            probe,
            rearm: Some(Duration::from_secs(20)),
            fallback: NAME,
        },
    );
    add_script(&device, "marks", &source).await;
    execute(&device, "/system script run marks").await;
    drop(device);
    let cut = std::time::Instant::now();
    wait_until(
        || async {
            let api = node.connect_api(crate::TEST_PASSWORD);
            Ok(!matches!(
                tokio::time::timeout(Duration::from_secs(5), api).await,
                Ok(Ok(_))
            ))
        },
        Duration::from_secs(60),
    )
    .await
    .expect("the script did not cut the device off");

    // the fallback brings the identity back, the marks stay
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
    // the 20 s from the script's end, not the 600 s armed
    assert!(
        cut.elapsed() < Duration::from_secs(200),
        "{:?}",
        cut.elapsed()
    );
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    assert_eq!(pending(&device, None).await.unwrap(), Pending::Gone);
    let progress = read(&device, "t1")
        .await
        .unwrap()
        .expect("no marks after the fallback");
    eprintln!("{}", progress.text);
    assert!(progress.done);
    assert_eq!(progress.lines, 6);
    assert!(
        progress
            .reach
            .first()
            .is_some_and(|(line, lost)| *lost && *line >= 4),
        "{progress:?}"
    );
    assert_eq!(progress.rearmed, Some(20));
    assert!(progress.rearmed_at.is_some(), "{progress:?}");
}

/// A clock jumping forward after the script (NTP, once the apply set the DNS servers) leaves
/// the fallback's time as it was: it counts from the uptime the script brought it forward at
/// (1og-sw1: by `next-run` and the clock it looked due at once and was left to fire).
#[tokio::test]
#[ignore]
async fn a_clock_jump_leaves_the_fallback_time() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-clock-jump-test");
    let node = boot_node(&golden, &work_dir, "clock", &[], crate::TEST_PASSWORD)
        .await
        .expect("node did not boot");
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    arm(&device, Duration::from_secs(600)).await.unwrap();
    let file = file_path(&device).await.unwrap();
    let source = instrument(
        "/system/identity\nset name=jumped\n",
        &Instrument {
            file: &file,
            run: "j1",
            lead: Duration::from_secs(1),
            probe: None,
            rearm: Some(Duration::from_secs(120)),
            fallback: NAME,
        },
    );
    let jump = ":delay 1s; /system clock set time=([/system clock get time] + 01:00:00)";
    add_script(&device, "jump", &source).await;
    execute(&device, "/system script run jump").await;
    let mut rearmed = None;
    for _ in 0..60 {
        rearmed = read(&device, "j1").await.unwrap().and_then(|p| p.rearm());
        if rearmed.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let rearmed = rearmed.expect("the script didn't bring the fallback forward");
    execute(&device, jump).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let Pending::Armed(Some(fires_in)) = pending(&device, Some(rearmed)).await.unwrap() else {
        panic!("no time of the fallback");
    };
    assert!(
        fires_in > Duration::from_secs(60) && fires_in <= Duration::from_secs(120),
        "{fires_in:?}"
    );
    disarm(&device).await.unwrap();
}
