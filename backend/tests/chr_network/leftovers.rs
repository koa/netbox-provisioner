//! What a job left on a device (`backend::device::leftovers`) on a real CHR: the apply
//! fallback, the watchdogs' safety net and an uploaded package are found (the apply script
//! only while it runs),
//! and each action takes its leftover away; "Jetzt zurück" loads the config from before.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use backend::Error;
use backend::device::leftovers::{LeftoverAction, LeftoverCleared, clear, read};
use backend::device::power::PowerChain;
use backend::device::ros::fallback;
use backend::device::update::DeviceLink;
use backend::device::watchdog::{Watching, suspend};
use backend::job::{Event, Log};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio_stream::StreamExt;

struct Direct(u16);

impl DeviceLink for Direct {
    async fn api(&self) -> Result<MikrotikDevice<SimpleResult>, Error> {
        Ok(MikrotikDevice::connect(
            ("127.0.0.1", self.0),
            b"admin",
            Some(encode_latin1_lossy(crate::TEST_PASSWORD)),
        )
        .await?)
    }
    async fn upload(
        &self,
        _name: &str,
        _data: &[u8],
        _progress: &(dyn Fn(usize) + Sync),
    ) -> Result<(), Error> {
        unreachable!("no upload when clearing leftovers")
    }
}

/// Runs `command` with `attributes`, failing on a trap.
async fn run(device: &MikrotikDevice<SimpleResult>, command: &str, attributes: &[(&str, &str)]) {
    let mut stream = device
        .send_command(
            command.as_bytes(),
            |cmd| {
                attributes
                    .iter()
                    .fold(cmd, |cmd, (key, value)| cmd.attribute(key.as_bytes(), value.as_bytes()))
            },
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        if let SimpleResult::Trap { message, .. } = result {
            panic!("{command}: {message}");
        }
    }
}

fn unpowered() -> PowerChain {
    PowerChain::unpowered("leftovers")
}

async fn identity(device: &MikrotikDevice<SimpleResult>) -> String {
    let mut stream = device.send_simple_command(b"/system/identity/print".as_slice(), ()).await;
    let mut name = String::new();
    while let Some(result) = stream.next().await {
        if let SimpleResult::Sentence(row) = result {
            name = row.get("name").cloned().flatten().unwrap_or_default().to_string();
        }
    }
    name
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
    assert_eq!(fallback::arm(&device, Duration::from_secs(3600)).await.unwrap(), None);
    run(&device, "/system/script/add", &[("name", "netbox-provisioner-apply"), ("source", ":delay 600s")]).await;
    run(&device, "/system/watchdog/set", &[("watch-address", "10.0.2.2")]).await;
    let watching = Watching {
        watch_address: Some("10.0.2.2".into()),
        poe_pings: Vec::new(),
    };
    suspend(&device, &watching, Duration::from_secs(7200)).await.unwrap();
    run(&device, "/file/add", &[("name", "routeros-7.99.npk"), ("contents", "not a package")]).await;
    let left = read(&device).await.unwrap();
    eprintln!("{left:#?}");
    assert!(left.apply_fallback.is_some());
    assert!(left.update_fallback.is_none());
    assert_eq!(left.watchdogs.as_ref().map(|w| &w.watching), Some(&watching));
    // the script after every apply: no leftover while it doesn't run
    assert!(!left.apply_script_running);
    assert_eq!(
        left.packages.iter().map(|p| p.file.as_ref()).collect::<Vec<_>>(),
        ["routeros-7.99.npk"]
    );

    for action in [
        LeftoverAction::StopApplyFallback,
        LeftoverAction::RestoreWatchdogs,
        LeftoverAction::RemovePackages,
    ] {
        let log = Log::new("leftovers");
        clear(&link, "leftovers", action, &unpowered(), &log).await.unwrap_or_else(|e| panic!("{action:?}: {e}"));
        assert_eq!(log.events(), [Event::LeftoverCleared(LeftoverCleared { action })]);
        assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    }
    let left = read(&device).await.unwrap();
    assert!(left.is_empty(), "{left:?}");
    run(&device, "/execute", &[("script", "/system script run netbox-provisioner-apply")]).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(read(&device).await.unwrap().apply_script_running);
    let mut jobs = device.send_simple_command(b"/system/script/job/print".as_slice(), ()).await;
    let mut ids = Vec::new();
    while let Some(result) = jobs.next().await {
        // the API session is a job too
        if let SimpleResult::Sentence(row) = result
            && row.get("script").cloned().flatten().as_deref() == Some("netbox-provisioner-apply")
            && let Some(Some(id)) = row.get(".id")
        {
            ids.push(id.to_string());
        }
    }
    drop(jobs);
    for id in ids {
        run(&device, "/system/script/job/remove", &[(".id", &id)]).await;
    }
    // a leftover cleared elsewhere in the meantime
    let result = clear(&link, "leftovers", LeftoverAction::RestoreWatchdogs, &unpowered(), &Log::new("leftovers")).await;
    assert!(matches!(result, Err(Error::LeftoverGone(_))), "{result:?}");

    // "Jetzt zurück": the identity set after the backup is gone after it
    assert_eq!(fallback::arm(&device, Duration::from_secs(3600)).await.unwrap(), None);
    let before = identity(&device).await;
    run(&device, "/system/identity/set", &[("name", "after-the-apply")]).await;
    drop(device);
    let log = Log::new("leftovers");
    clear(&link, "leftovers", LeftoverAction::ApplyFallbackNow, &unpowered(), &log).await.expect("apply fallback now");
    assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    eprintln!("{:#?}", log.events());
    let device = link.api().await.unwrap();
    assert_eq!(identity(&device).await, before);
    assert!(read(&device).await.unwrap().apply_fallback.is_none());
}
