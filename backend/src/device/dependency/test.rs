//! Scope: Tests of `dependency`; nothing else.
use super::{DeviceWait, Paths, PowerChain, WaitingForDevice};
use crate::job::{Event, Log};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

// device names are unique per test, the leases are shared by all tests of the process

#[tokio::test]
async fn a_supplier_reboots_after_its_consumers_updates() {
    let supplier = PowerChain::new("t1-switch", &[], &["t1-ap", "t1-behind-ap"]);
    let consumer = PowerChain::new("t1-behind-ap", &["t1-ap", "t1-switch"], &[]);
    let log = Log::new("t1");
    let update = consumer.updating(&log).await;
    let supplier_log = Log::new("t1");
    {
        let reboot = supplier.rebooting(&supplier_log);
        tokio::pin!(reboot);
        assert!(
            timeout(Duration::from_millis(100), reboot.as_mut())
                .await
                .is_err()
        );
        drop(update);
        let _reboot = timeout(Duration::from_secs(1), reboot)
            .await
            .expect("reboot after the update");
    }
    assert_eq!(
        supplier_log.events(),
        [Event::WaitingForDevice(WaitingForDevice {
            device: "t1-behind-ap".into(),
            reason: DeviceWait::ConsumerUpdating,
        })]
    );
}

/// Cutting a device's power waits for its own update, and its next update waits for the
/// power to be back.
#[tokio::test]
async fn a_power_cut_waits_for_the_update_of_the_device() {
    let ap = PowerChain::new("t4-ap", &["t4-switch"], &[]);
    let log = Log::new("t4");
    let update = ap.updating(&log).await;
    let cut_log = Log::new("t4");
    let cut = ap.power_cut("t4-switch", &cut_log);
    tokio::pin!(cut);
    assert!(
        timeout(Duration::from_millis(100), cut.as_mut())
            .await
            .is_err()
    );
    drop(update);
    let cut = timeout(Duration::from_secs(1), cut)
        .await
        .expect("cut after the update");
    assert_eq!(
        cut_log.events(),
        [Event::WaitingForDevice(WaitingForDevice {
            device: "t4-ap".into(),
            reason: DeviceWait::ConsumerUpdating,
        })]
    );
    let update = ap.updating(&log);
    tokio::pin!(update);
    assert!(
        timeout(Duration::from_millis(100), update.as_mut())
            .await
            .is_err()
    );
    drop(cut);
    let _update = timeout(Duration::from_secs(1), update)
        .await
        .expect("update after the cut");
}

#[tokio::test]
async fn an_update_waits_for_its_suppliers_reboot() {
    let supplier = PowerChain::new("t2-switch", &[], &["t2-ap"]);
    let consumer = PowerChain::new("t2-ap", &["t2-switch"], &[]);
    let log = Log::new("t2");
    let reboot = supplier.rebooting(&log).await;
    let consumer_log = Log::new("t2");
    let update = consumer.updating(&consumer_log);
    tokio::pin!(update);
    assert!(
        timeout(Duration::from_millis(100), update.as_mut())
            .await
            .is_err()
    );
    drop(reboot);
    let _update = timeout(Duration::from_secs(1), update)
        .await
        .expect("update after the reboot");
}

#[tokio::test]
async fn unrelated_devices_and_the_device_itself_dont_wait() {
    let switch = PowerChain::new("t3-switch", &[], &["t3-ap"]);
    let other = PowerChain::new("t3-other", &[], &[]);
    let log = Log::new("t3");
    let _other_update = other.updating(&log).await;
    // a device reboots within its own update
    let _update = switch.updating(&log).await;
    let _reboot = timeout(Duration::from_secs(1), switch.rebooting(&log))
        .await
        .expect("no wait");
    let events = log.events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::WaitingForDevice(_))),
        "{events:?}"
    );
}

/// entry -- a -- target (the CHR chain of `ospf_chain.rs`): `a` reboots only after the
/// update of `target`, and `target`'s update waits while `a` reboots.
#[tokio::test]
async fn a_device_reboots_after_the_update_of_one_behind_it() {
    let paths = Arc::new(Paths::new(
        &["t5-entry"],
        &[("t5-entry", "t5-a"), ("t5-a", "t5-target")],
    ));
    let a = PowerChain::on_paths("t5-a", &paths);
    let target = PowerChain::on_paths("t5-target", &paths);
    let log = Log::new("t5");
    let update = target.updating(&log).await;
    let a_log = Log::new("t5");
    let reboot = a.rebooting(&a_log);
    tokio::pin!(reboot);
    assert!(
        timeout(Duration::from_millis(100), reboot.as_mut())
            .await
            .is_err()
    );
    drop(update);
    let reboot = timeout(Duration::from_secs(1), reboot)
        .await
        .expect("reboot after the update");
    assert_eq!(
        a_log.events(),
        [Event::WaitingForDevice(WaitingForDevice {
            device: "t5-target".into(),
            reason: DeviceWait::BehindUpdating,
        })]
    );
    let target_log = Log::new("t5");
    let update = target.updating(&target_log);
    tokio::pin!(update);
    assert!(
        timeout(Duration::from_millis(100), update.as_mut())
            .await
            .is_err()
    );
    drop(reboot);
    let _update = timeout(Duration::from_secs(1), update)
        .await
        .expect("update after the reboot");
    assert!(
        target_log
            .events()
            .contains(&Event::WaitingForDevice(WaitingForDevice {
                device: "t5-a".into(),
                reason: DeviceWait::BehindRebooting,
            }))
    );
}

/// Reached over two ways (lab-sxt-a over lab-np1 or the radio bridge of lab-np2): either
/// side reboots while the device updates, not both at once.
#[tokio::test]
async fn of_two_ways_one_reboots_at_a_time() {
    let paths = Arc::new(Paths::new(
        &["t6-root"],
        &[
            ("t6-root", "t6-left"),
            ("t6-root", "t6-right"),
            ("t6-left", "t6-target"),
            ("t6-right", "t6-target"),
        ],
    ));
    let log = Log::new("t6");
    let _update = PowerChain::on_paths("t6-target", &paths)
        .updating(&log)
        .await;
    let left = timeout(
        Duration::from_secs(1),
        PowerChain::on_paths("t6-left", &paths).rebooting(&log),
    )
    .await
    .expect("one way stays");
    let right_log = Log::new("t6");
    let right = PowerChain::on_paths("t6-right", &paths);
    let reboot = right.rebooting(&right_log);
    tokio::pin!(reboot);
    assert!(
        timeout(Duration::from_millis(100), reboot.as_mut())
            .await
            .is_err()
    );
    drop(left);
    let _reboot = timeout(Duration::from_secs(1), reboot)
        .await
        .expect("the other way is back");
}

/// A device updating that no root reaches over NetBox's ways (reached some other way)
/// holds no reboot back.
#[tokio::test]
async fn a_device_off_the_ways_holds_nothing_back() {
    let paths = Arc::new(Paths::new(&["t7-root"], &[("t7-root", "t7-a")]));
    let log = Log::new("t7");
    let _update = PowerChain::on_paths("t7-elsewhere", &paths)
        .updating(&log)
        .await;
    let _reboot = timeout(
        Duration::from_secs(1),
        PowerChain::on_paths("t7-root", &paths).rebooting(&log),
    )
    .await
    .expect("no wait");
}
