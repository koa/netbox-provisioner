//! Scope: CHR tests of what a job leaves on a device; nothing else.
//!
//! What a job left on a device (`backend::device::leftovers`) on a real CHR: the apply
//! fallback, the watchdogs' safety net and an uploaded package are found (the apply script
//! only while it runs),
//! and each action takes its leftover away; "Jetzt zurück" loads the config from before.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::node::{
    Direct, add, add_script, execute, identity, set_identity, set_single,
};
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use backend::Error;
use backend::device::access::{Session, act};
use backend::device::dependency::PowerChain;
use backend::device::leftovers::{LeftoverAction, LeftoverCleared, clear, read};
use backend::device::ros::fallback;
use backend::device::update::DeviceLink;
use backend::device::watchdog::{Watching, suspend};
use backend::job::{Event, Log};
use mikrotik_model::action::ScriptJobRemove;
use mikrotik_model::model::{File, FileCfg, SystemScriptJobState, SystemWatchdogCfg};
use mikrotik_model::resource::collect_resource;
use mikrotik_model::value::{HasNone, RosValue};
use std::time::Duration;

/// Adds the file `name`, a directory without `contents`.
async fn add_file(device: &Session, name: &str, contents: Option<&str>) {
    let mut file = FileCfg::new(name.into());
    match contents {
        Some(contents) => file.contents = Some(contents.into()),
        None => file._type = Some("directory".into()),
    }
    add(device, &file).await.expect("file added");
}

fn unpowered() -> PowerChain {
    PowerChain::unpowered("leftovers")
}

/// A package for the CAPs, in a directory (`sd1-part1/upgrade/…` on a router with an SD card).
const CAP_PACKAGE: &str = "upgrade/routeros-7.99-arm.npk";

async fn files(device: &Session) -> Vec<String> {
    collect_resource::<File>(device)
        .await
        .expect("files")
        .iter()
        .map(|file| file.cfg.name.to_string())
        .collect()
}

#[tokio::test]
#[ignore]
async fn leftovers_are_found_and_cleared() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-leftovers-test");
    let node = boot_node(&golden, &work_dir, "leftovers", &[], crate::TEST_PASSWORD)
        .await
        .expect("node did not boot");
    let link = Direct(node.api_port);
    let device = link.api().await.unwrap();
    let left = read(&device).await.unwrap();
    assert!(left.is_empty(), "{left:?}");

    // what jobs leave: an apply's fallback and script, a suspended watchdog, a package
    assert_eq!(
        fallback::arm(&device, Duration::from_secs(3600))
            .await
            .unwrap(),
        None
    );
    add_script(&device, "netbox-provisioner-apply", ":delay 600s").await;
    set_single(&device, |watchdog: &mut SystemWatchdogCfg| {
        watchdog.watch_address = HasNone::Value("10.0.2.2".parse().unwrap())
    })
    .await
    .expect("watch address");
    let watching = Watching {
        watch_address: Some("10.0.2.2".into()),
        poe_pings: Vec::new(),
    };
    suspend(&device, &watching, Duration::from_secs(7200))
        .await
        .unwrap();
    add_file(&device, "routeros-7.99.npk", Some("not a package")).await;
    // what a CAPsMAN router keeps for its CAPs, on purpose: no leftover, never removed
    add_file(&device, "upgrade", None).await;
    add_file(&device, CAP_PACKAGE, Some("not a package")).await;
    let left = read(&device).await.unwrap();
    eprintln!("{left:#?}");
    assert!(left.apply_fallback.is_some());
    assert!(left.update_fallback.is_none());
    assert_eq!(
        left.watchdogs.as_ref().map(|w| &w.watching),
        Some(&watching)
    );
    // the script after every apply: no leftover while it doesn't run
    assert!(!left.apply_script_running);
    assert_eq!(
        left.packages
            .iter()
            .map(|p| p.file.as_ref())
            .collect::<Vec<_>>(),
        ["routeros-7.99.npk"]
    );

    for action in [
        LeftoverAction::StopApplyFallback,
        LeftoverAction::RestoreWatchdogs,
        LeftoverAction::RemovePackages,
    ] {
        let log = Log::new("leftovers");
        clear(&link, "leftovers", action, &unpowered(), &log)
            .await
            .unwrap_or_else(|e| panic!("{action:?}: {e}"));
        assert_eq!(
            log.events(),
            [Event::LeftoverCleared(LeftoverCleared { action })]
        );
        assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    }
    let left = read(&device).await.unwrap();
    assert!(left.is_empty(), "{left:?}");
    assert!(files(&device).await.iter().any(|name| name == CAP_PACKAGE));
    execute(&device, "/system script run netbox-provisioner-apply").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(read(&device).await.unwrap().apply_script_running);
    // the API session is a job too
    let jobs = collect_resource::<SystemScriptJobState>(&device)
        .await
        .expect("script jobs");
    for job in jobs.iter().filter(|job| {
        job.script
            .as_ref()
            .is_some_and(|script| *script.0 == *b"netbox-provisioner-apply")
    }) {
        let number = String::from_utf8_lossy(&job.id.encode_ros()).into_owned();
        act(&device, &ScriptJobRemove { number: &number })
            .await
            .expect("job removed");
    }
    // a leftover cleared elsewhere in the meantime
    let result = clear(
        &link,
        "leftovers",
        LeftoverAction::RestoreWatchdogs,
        &unpowered(),
        &Log::new("leftovers"),
    )
    .await;
    assert!(matches!(result, Err(Error::LeftoverGone(_))), "{result:?}");

    // "Jetzt zurück": the identity set after the backup is gone after it
    assert_eq!(
        fallback::arm(&device, Duration::from_secs(3600))
            .await
            .unwrap(),
        None
    );
    let before = identity(&device).await;
    set_identity(&device, "after-the-apply").await;
    drop(device);
    let log = Log::new("leftovers");
    clear(
        &link,
        "leftovers",
        LeftoverAction::ApplyFallbackNow,
        &unpowered(),
        &log,
    )
    .await
    .expect("apply fallback now");
    assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    eprintln!("{:#?}", log.events());
    let device = link.api().await.unwrap();
    assert_eq!(identity(&device).await, before);
    assert!(read(&device).await.unwrap().apply_fallback.is_none());
}
