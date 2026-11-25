//! Scope: The CHR test of updates along a chain (`docs/network-update.md`, C10); nothing else.
//!
//! The chain `entry -- a -- target`: the provisioner reaches `a` through `entry` and `target`
//! only through `a`. Both update from the older release at once, `a` started first: `a` must
//! not reboot while `target` updates (it would cut the update off from its device), so
//! `target` reboots first and `a` waits for it (`dependency::DeviceWait::BehindUpdating`).
use crate::chr_network::golden_image::{CHR, CHR_OLDER, ensure_factory_image, ensure_golden_image};
use crate::chr_network::mac_telnet::wait_for_boot;
use crate::chr_network::node::{add_address, forward_ssh};
use crate::chr_network::qemu::{free_tcp_port, nix_unavailable};
use crate::chr_network::update::{PASSWORD, TunnelLink};
use crate::chr_network::vm::{PortWiring, boot_node, mac_of, start_node};
use backend::device::bootstrap::{MacTelnetSetup, set_up};
use backend::device::dependency::{DeviceWait, Paths, PowerChain, WaitingForDevice};
use backend::device::terminal::{Terminal, Tunnel};
use backend::device::update::{DeviceLink, Version, read_state, update};
use backend::device::update_fallback::WayBack;
use backend::device_state::{JobPhase, UpdateState};
use backend::job::{Event, Log};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::Instant;

/// Sets up the factory node at `mac` over mac-telnet from the device of `terminal`: `address`
/// on its `ether2`, the neighbor at `neighbor` on `port` of the device of `terminal`.
async fn set_up_over(
    mut terminal: Terminal,
    mac: mikrotik_model::MacAddress,
    address: &str,
    port: &str,
    neighbor: &str,
) {
    let setup = MacTelnetSetup {
        macs: [mac].into(),
        port: "ether2".into(),
        address: address.parse().unwrap(),
        on_bridge: false,
        neighbor_port: Some(port.into()),
        neighbor_address: neighbor.parse().unwrap(),
    };
    let result = set_up(&mut terminal, &setup, "admin", PASSWORD, None).await;
    eprintln!("--- setup {address} ---\n{}", terminal.transcript());
    result.expect("setup over mac-telnet");
    terminal.close().await;
}

/// A job's log past `Reading`, as the mutation hands it on.
fn reading(device: &str) -> Log {
    let log = Log::new(device);
    log.enter(UpdateState::Reading);
    log
}

/// When `log` first entered `Rebooting`, watched until both updates are through.
async fn first_reboot(log: &Log, done: &tokio::sync::Notify) -> Option<Instant> {
    let finished = done.notified();
    tokio::pin!(finished);
    loop {
        if log.phase() == Some(JobPhase::Update(UpdateState::Rebooting)) {
            return Some(Instant::now());
        }
        tokio::select! {
            _ = &mut finished => return None,
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
}

#[tokio::test]
#[ignore]
async fn a_router_reboots_after_the_device_behind_it() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let older = ensure_factory_image(&crate::cache_dir(), &CHR_OLDER)
        .await
        .expect("older image failed");
    let work_dir = std::env::temp_dir().join("chr-network-rollout-chain-test");
    let (first, second) = (free_tcp_port().unwrap(), free_tcp_port().unwrap());
    let entry = boot_node(
        &golden,
        &work_dir,
        "entry",
        &[PortWiring::Listen(first)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let a = start_node(
        &older,
        &work_dir,
        "a",
        &[PortWiring::Connect(first), PortWiring::Listen(second)],
    )
    .await
    .expect("a boot failed");
    let target = start_node(&older, &work_dir, "target", &[PortWiring::Connect(second)])
        .await
        .expect("target boot failed");
    let entry_api = entry
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("entry api");
    add_address(&entry_api, "192.168.78.1/30", "ether2").await;
    forward_ssh(&entry_api).await;
    drop(entry_api);
    wait_for_boot(&a).await;
    wait_for_boot(&target).await;

    // like new devices: a over mac-telnet from entry, target from a; everything through entry
    let at_entry = Terminal::connect(
        ([127, 0, 0, 1], entry.ssh_port).into(),
        "admin",
        Some(crate::TEST_PASSWORD),
    )
    .await
    .expect("ssh to entry");
    set_up_over(
        at_entry,
        mac_of(a.index, 1),
        "192.168.78.2/30",
        "ether2",
        "192.168.78.1",
    )
    .await;
    let ssh = TcpStream::connect(("127.0.0.1", entry.ssh_port))
        .await
        .expect("ssh to entry");
    let to_a = TunnelLink {
        tunnel: Tunnel::connect_with_stream(ssh, "admin", Some(crate::TEST_PASSWORD))
            .await
            .expect("tunnel to entry"),
        address: "192.168.78.2".parse().unwrap(),
    };
    let a_api = to_a.api().await.expect("a api");
    add_address(&a_api, "192.168.79.1/30", "ether3").await;
    forward_ssh(&a_api).await;
    drop(a_api);
    let to_a_ssh = async || to_a.tunnel.open(to_a.address, 22).await.expect("ssh to a");
    let at_a = Terminal::connect_with_stream(to_a_ssh().await, "admin", Some(PASSWORD))
        .await
        .expect("terminal on a");
    set_up_over(
        at_a,
        mac_of(target.index, 1),
        "192.168.79.2/30",
        "ether3",
        "192.168.79.1",
    )
    .await;
    // target only through a: a's reboot cuts it off
    let to_target = TunnelLink {
        tunnel: Tunnel::connect_with_stream(to_a_ssh().await, "admin", Some(PASSWORD))
            .await
            .expect("tunnel to a"),
        address: "192.168.79.2".parse().unwrap(),
    };
    let older_version = Version::parse(CHR_OLDER.version).unwrap();
    for link in [&to_a as &TunnelLink, &to_target] {
        let state = read_state(&link.api().await.unwrap()).await.unwrap();
        assert_eq!(state.version, older_version);
    }

    let mut paths = Paths::rooted(&["entry"]);
    paths.link("entry", "a");
    paths.link("a", "target");
    let paths = Arc::new(paths);
    let a_power = PowerChain::on_paths("a", &paths);
    let target_power = PowerChain::on_paths("target", &paths);
    let a_back = WayBack {
        device: "a".into(),
        neighbors: ["entry".into()].into(),
        confirmed: false,
    };
    let target_back = WayBack {
        device: "target".into(),
        neighbors: ["a".into()].into(),
        confirmed: false,
    };
    let current = Version::parse(CHR.version).unwrap();
    let fallback = Duration::from_secs(900);
    let a_log = reading("a");
    let target_log = reading("target");
    let done = tokio::sync::Notify::new();

    // a first; target once a has its packages, so both update at once
    let a_update = async {
        let result = update(&to_a, &a_power, &current, &[], fallback, &a_back, &a_log).await;
        (result, Instant::now())
    };
    let target_update = async {
        while !a_log
            .events()
            .iter()
            .any(|event| matches!(event, Event::PackageDownloaded(_)))
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let result = update(
            &to_target,
            &target_power,
            &current,
            &[],
            fallback,
            &target_back,
            &target_log,
        )
        .await;
        (result, Instant::now())
    };
    let updates = async {
        let both = tokio::join!(a_update, target_update);
        done.notify_waiters();
        both
    };
    let (((a_result, _), (target_result, target_done)), a_rebooted, target_rebooted) = tokio::join!(
        updates,
        first_reboot(&a_log, &done),
        first_reboot(&target_log, &done)
    );
    eprintln!("--- a ---\n{:#?}", a_log.events());
    eprintln!("--- target ---\n{:#?}", target_log.events());
    a_result.expect("update of a");
    target_result.expect("update of target");

    // a waited for target, and rebooted only once target's update was through
    assert!(
        a_log
            .events()
            .contains(&Event::WaitingForDevice(WaitingForDevice {
                device: "target".into(),
                reason: DeviceWait::BehindUpdating,
            })),
        "a didn't wait for target"
    );
    let a_rebooted = a_rebooted.expect("a rebooted");
    let target_rebooted = target_rebooted.expect("target rebooted");
    assert!(target_rebooted < a_rebooted, "a rebooted before target");
    assert!(target_done <= a_rebooted, "a rebooted while target updated");
    // the tunnel through a ended with its reboot: a new one to read target
    let to_target = TunnelLink {
        tunnel: Tunnel::connect_with_stream(to_a_ssh().await, "admin", Some(PASSWORD))
            .await
            .expect("tunnel to a after its update"),
        address: to_target.address,
    };
    for link in [&to_a, &to_target] {
        let state = read_state(&link.api().await.unwrap()).await.unwrap();
        assert_eq!(state.version, current);
    }
}
