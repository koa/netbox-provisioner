//! Scope: CHR tests of the device mode; nothing else.
//!
//! Turning a feature on in the device mode (`backend::device::device_mode`) against a real CHR:
//! the job asks for the change, tells the user to confirm it, and the confirmation is a cold
//! boot (the VM's power cut, [`RunningNode::power_cycle`]). Cancelling before it leaves the
//! device mode as it was, also when the power is cut afterwards. The job reaches the node
//! through [`unplugged_relay`]: killed, QEMU closes its connections, a RouterBOARD whose power
//! is cut doesn't.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::node::{Direct, add};
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::{RunningNode, boot_node};
use backend::Error;
use backend::device::dependency::PowerChain;
use backend::device::device_mode::{
    DeviceModeChange, DeviceModeChanged, DeviceModeFeature, DeviceModeReason, change_to,
};
use backend::device::reboot::{Reboot, RebootReason};
use backend::device_state::DeviceModeState;
use backend::job::{Event, Log};
use mikrotik_model::action::DeviceModeUpdate;
use mikrotik_model::model::{DeviceMode, File, SystemDeviceModeState, SystemSchedulerCfg};
use mikrotik_model::resource::{SingleResource, collect_resource};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::StreamExt;

/// A relay to the node's `port`, whose connections stay open when the node's side goes, until
/// the job closes them: a RouterBOARD whose power is cut sends neither FIN nor RST, its
/// connection only falls silent. Gives back the relay's port.
async fn unplugged_relay(port: u16) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("relay");
    let relay = listener.local_addr().expect("relay port").port();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            tokio::spawn(async move {
                let Ok(node) = TcpStream::connect(("127.0.0.1", port)).await else {
                    return;
                };
                let (mut client_read, mut client_write) = client.into_split();
                let (mut node_read, mut node_write) = node.into_split();
                // to the node until the job closes, what the node no longer takes is lost
                let up = tokio::spawn(async move {
                    let mut buffer = [0; 4096];
                    while let Ok(read @ 1..) = client_read.read(&mut buffer).await {
                        let _ = node_write.write_all(&buffer[..read]).await;
                    }
                });
                let _ = tokio::io::copy(&mut node_read, &mut client_write).await;
                // the node's side is gone: the job's stays open and silent
                let _ = up.await;
                drop(client_write);
            });
        }
    });
    relay
}

/// `/system/device-mode` of the node.
async fn device_mode(node: &RunningNode) -> SystemDeviceModeState {
    let device = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    SystemDeviceModeState::fetch(&device)
        .await
        .expect("device mode read")
        .expect("a device mode")
}

/// Waits until the job asks for the confirmation.
async fn confirm_asked(log: &Log) {
    tokio::time::timeout(Duration::from_secs(60), async {
        while !log
            .events()
            .iter()
            .any(|e| matches!(e, Event::DeviceModeConfirm(_)))
        {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("no confirmation asked for");
}

/// A job's log as the mutation hands it on: past `Reading` (`device_state::DeviceModeState`).
fn reading() -> Log {
    let log = Log::new("mode");
    log.enter(DeviceModeState::Reading);
    log
}

#[tokio::test]
#[ignore]
async fn device_mode_change_confirmed_by_a_cold_boot() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-device-mode-test");
    let mut node = boot_node(&golden, &work_dir, "mode", &[], crate::TEST_PASSWORD)
        .await
        .expect("node boot failed");
    // off in the CHR's advanced mode, like on the lab's RouterBOARDs
    let wanted = [DeviceModeChange {
        feature: DeviceModeFeature::Partitions,
        reason: DeviceModeReason::UpdateFallback,
    }];
    let power = PowerChain::unpowered("mode");
    assert_eq!(device_mode(&node).await.partitions, Some(false));

    // cancelled while it waits: a power cut afterwards changes nothing
    let log = reading();
    let job = tokio::spawn({
        let log = log.clone();
        let (link, power, wanted) = (Direct(node.api_port), power.clone(), wanted.clone());
        async move { change_to(&link, &wanted, &power, &log).await }
    });
    confirm_asked(&log).await;
    log.cancel();
    let result = job.await.expect("job");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    tokio::time::sleep(Duration::from_secs(2)).await;
    node.power_cycle().await.expect("power cycle");
    assert_eq!(device_mode(&node).await.partitions, Some(false));

    // confirmed by cutting the power while it counts down; the job sees its connection fall
    // silent, not closed
    let unplugged = Direct(unplugged_relay(node.api_port).await);
    let log = reading();
    let job = tokio::spawn({
        let log = log.clone();
        let (power, wanted) = (power.clone(), wanted.clone());
        async move { change_to(&unplugged, &wanted, &power, &log).await }
    });
    confirm_asked(&log).await;
    node.power_cycle().await.expect("power cycle");
    job.await.expect("job").expect("device-mode change");
    assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    assert_eq!(device_mode(&node).await.partitions, Some(true));
    let events = log.events();
    assert!(
        events.contains(&Event::Reboot(Reboot {
            reason: RebootReason::DeviceMode,
            timeout: backend::device::reboot::REBOOT_TIMEOUT.into(),
        })),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(Event::DeviceModeChanged(changed)) if changed.changes.len() == 1),
        "{events:?}"
    );

    // on already: nothing to confirm
    let log = reading();
    change_to(&Direct(node.api_port), &wanted, &power, &log)
        .await
        .expect("nothing to change");
    assert_eq!(
        log.events(),
        [Event::DeviceModeChanged(DeviceModeChanged {
            changes: Box::default()
        })]
    );
}

/// In the device mode `home` the scheduler is off: RouterOS refuses a scheduler entry ("not
/// allowed by device-mode", berghof-hansueli 7.21.5), so an apply has no fallback. `arm` says
/// so instead of failing, and leaves no backup behind.
#[tokio::test]
#[ignore]
async fn home_mode_has_no_fallback() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-home-mode-test");
    let mut node = boot_node(&golden, &work_dir, "home", &[], crate::TEST_PASSWORD)
        .await
        .expect("node boot failed");
    // the update counts down as long as its connection stays open; a cold boot confirms it
    let counting = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    let update = tokio::spawn(async move {
        let home = DeviceModeUpdate {
            mode: Some(&DeviceMode::Home),
            features: &[],
            activation_timeout: Duration::from_secs(300),
        };
        let mut answers = counting.hold(&home).await;
        while answers.next().await.is_some() {}
    });
    tokio::time::sleep(Duration::from_secs(5)).await;
    node.power_cycle().await.expect("power cycle");
    update.abort();
    let mode = device_mode(&node).await;
    assert_eq!(mode.mode, DeviceMode::Home);
    assert_eq!(mode.scheduler, Some(false));

    let device = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    let mut probe = SystemSchedulerCfg::new("probe".into());
    probe.interval = Some(Duration::from_secs(3600));
    probe.on_event = Some(":nothing".into());
    let refused = add(&device, &probe)
        .await
        .expect_err("the scheduler is off in `home`");
    assert!(
        format!("{refused:#}").contains("device-mode"),
        "{refused:#}"
    );
    let armed = backend::device::ros::fallback::arm(&device, Duration::from_secs(60))
        .await
        .expect("arm");
    assert_eq!(armed.as_deref(), Some("home"));
    for file in collect_resource::<File>(&device).await.expect("files") {
        let name = file.cfg.name.to_string();
        assert!(!name.contains("netbox-provisioner"), "{name}");
    }
    // taking a fallback away that isn't there works too
    backend::device::ros::fallback::disarm(&device)
        .await
        .expect("disarm");
}
