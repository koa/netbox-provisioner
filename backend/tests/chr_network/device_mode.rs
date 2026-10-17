//! Turning a feature on in the device mode (`backend::device::device_mode`) against a real CHR:
//! the job asks for the change, tells the user to confirm it, and the confirmation is a cold
//! boot (the VM's power cut, [`RunningNode::power_cycle`]). Cancelling before it leaves the
//! device mode as it was, also when the power is cut afterwards.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::{RunningNode, boot_node};
use backend::Error;
use backend::device::device_mode::{DeviceModeChanged, DeviceModeReason, change_to};
use backend::device::power::PowerChain;
use backend::device::update::{DeviceLink, Reboot, RebootReason};
use backend::job::{Event, Log};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio_stream::StreamExt;

/// Straight to the node's forwarded API port.
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
        unreachable!("no upload in a device-mode change")
    }
}

/// `/system/device-mode` `feature` on the node.
async fn feature(node: &RunningNode, feature: &str) -> String {
    let device = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    let mut rows = device
        .send_simple_command(b"/system/device-mode/print".as_slice(), ())
        .await;
    while let Some(row) = rows.next().await {
        if let SimpleResult::Sentence(row) = row {
            return row.get(feature).cloned().flatten().unwrap_or_default().into();
        }
    }
    panic!("no device mode")
}

/// Waits until the job asks for the confirmation.
async fn confirm_asked(log: &Log) {
    tokio::time::timeout(Duration::from_secs(60), async {
        while !log.events().iter().any(|e| matches!(e, Event::DeviceModeConfirm(_))) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("no confirmation asked for");
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
    let wanted = [("partitions", DeviceModeReason::UpdateFallback)];
    let power = PowerChain::unpowered("mode");
    let link = Direct(node.api_port);
    assert_eq!(feature(&node, "partitions").await, "false");

    // cancelled while it waits: a power cut afterwards changes nothing
    let log = Log::new("mode");
    let job = tokio::spawn({
        let log = log.clone();
        let (link, power) = (Direct(node.api_port), power.clone());
        async move { change_to(&link, &wanted, &power, &log).await }
    });
    confirm_asked(&log).await;
    log.cancel();
    let result = job.await.expect("job");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    tokio::time::sleep(Duration::from_secs(2)).await;
    node.power_cycle().await.expect("power cycle");
    assert_eq!(feature(&node, "partitions").await, "false");

    // confirmed by cutting the power while it counts down
    let log = Log::new("mode");
    let job = tokio::spawn({
        let log = log.clone();
        let power = power.clone();
        async move { change_to(&link, &wanted, &power, &log).await }
    });
    confirm_asked(&log).await;
    node.power_cycle().await.expect("power cycle");
    job.await.expect("job").expect("device-mode change");
    assert_eq!(feature(&node, "partitions").await, "true");
    let events = log.events();
    assert!(
        events.contains(&Event::Reboot(Reboot {
            reason: RebootReason::DeviceMode,
            timeout: backend::device::update::REBOOT_TIMEOUT.into(),
        })),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(Event::DeviceModeChanged(changed)) if changed.changes.len() == 1),
        "{events:?}"
    );

    // on already: nothing to confirm
    let log = Log::new("mode");
    change_to(&Direct(node.api_port), &wanted, &power, &log)
        .await
        .expect("nothing to change");
    assert_eq!(
        log.events(),
        [Event::DeviceModeChanged(DeviceModeChanged { changes: vec![] })]
    );
}
