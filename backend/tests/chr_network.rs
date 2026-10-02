//! Multi-CHR network test harness: boots several MikroTik CHR instances
//! under QEMU, wires them up per a `TopologyBuilder` scenario, and proves
//! real distributed network behavior (OSPF convergence across hops) rather
//! than just single-device command acceptance.
//!
//! Gated behind `--ignored` since it needs `nix`/QEMU and real wall-clock
//! time to boot VMs - this must never slow down or break a normal
//! `cargo test` run. Every test here starts by calling `qemu::nix_unavailable()`
//! and skipping with a clear message if it's missing.

mod chr_network {
    pub mod golden_image;
    pub mod qemu;
    pub mod scenario;
    pub mod serial;
    pub mod vm;
}

use backend::topology::test::TopologyBuilder;
use backend::topology::{Device, Interface, PhysicalPortId};
use chr_network::golden_image::ensure_golden_image;
use chr_network::qemu::{free_tcp_port, nix_unavailable};
use chr_network::scenario::boot_scenario;
use chr_network::vm::{boot_node, PortWiring};
use mikrotik_model::hwconfig::DeviceType;
use std::collections::HashMap;
use std::path::PathBuf;

const TEST_PASSWORD: &str = "NetboxTest123!";

fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CHR_TEST_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend/ has a parent")
        .join(".chr-test-cache")
}

#[tokio::test]
#[ignore]
async fn build_golden_image() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    assert!(golden.path.is_file());
}

#[tokio::test]
#[ignore]
async fn boot_single_overlay_node() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-boot-single-test");
    let node = boot_node(&golden, &work_dir, "solo", &[], TEST_PASSWORD)
        .await
        .expect("node boot failed");
    let device = node
        .connect_api(TEST_PASSWORD)
        .await
        .expect("api connect failed");
    let mut stream = device
        .send_simple_command(b"/system/identity/print".as_slice(), ())
        .await;
    use tokio_stream::StreamExt;
    let result = stream.next().await.expect("no reply");
    eprintln!("identity: {result:?}");
}

#[tokio::test]
#[ignore]
async fn kill_minus_9_does_not_corrupt_golden_image() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let golden_before = tokio::fs::read(&golden.path).await.expect("read golden");

    let work_dir = std::env::temp_dir().join("chr-network-kill9-test");
    let mut node = boot_node(&golden, &work_dir, "victim", &[], TEST_PASSWORD)
        .await
        .expect("node boot failed");
    // Hard kill, bypassing the graceful shutdown path entirely - this is
    // exactly the failure mode that corrupted a hand-built CHR image
    // earlier in this project. The whole point of the overlay design is
    // that this must be harmless now (unlike golden_image.rs's build path,
    // which goes out of its way to shut down cleanly instead).
    node.kill().await.expect("kill failed");
    drop(node);

    let golden_after = tokio::fs::read(&golden.path).await.expect("read golden");
    assert_eq!(
        golden_before, golden_after,
        "golden image bytes changed after kill -9 of an overlay-booted node"
    );

    // And a fresh node boots fine from the still-good golden image.
    let work_dir2 = std::env::temp_dir().join("chr-network-kill9-test-retry");
    let node2 = boot_node(&golden, &work_dir2, "retry", &[], TEST_PASSWORD)
        .await
        .expect("node boot after kill-9 failed");
    node2
        .connect_api(TEST_PASSWORD)
        .await
        .expect("api connect after kill-9 failed");
}

/// Proves the `socket,listen=`/`socket,connect=` NIC-slot pairing in
/// `vm.rs` actually carries real L2/L3 traffic between two independently
/// booted CHR instances, before building any topology-driven wiring on top
/// of it.
#[tokio::test]
#[ignore]
async fn two_nodes_directly_cabled_can_ping() {
    use tokio_stream::StreamExt;

    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let cable_port = free_tcp_port().expect("free port");

    // The listener must be up before the connector dials it.
    let work_dir = std::env::temp_dir().join("chr-network-cable-test");
    let node_a = boot_node(
        &golden,
        &work_dir,
        "a",
        &[PortWiring::Listen(cable_port)],
        TEST_PASSWORD,
    )
    .await
    .expect("node a boot failed");
    let node_b = boot_node(
        &golden,
        &work_dir,
        "b",
        &[PortWiring::Connect(cable_port)],
        TEST_PASSWORD,
    )
    .await
    .expect("node b boot failed");

    let device_a = node_a.connect_api(TEST_PASSWORD).await.expect("a api");
    let device_b = node_b.connect_api(TEST_PASSWORD).await.expect("b api");

    run_to_done(
        &device_a,
        b"/ip/address/add",
        &[(b"address", b"10.10.10.1/24"), (b"interface", b"ether2")],
    )
    .await;
    run_to_done(
        &device_b,
        b"/ip/address/add",
        &[(b"address", b"10.10.10.2/24"), (b"interface", b"ether2")],
    )
    .await;

    let mut stream = device_a
        .send_command(
            b"/ping".as_slice(),
            |cmd| {
                cmd.attribute(b"address".as_slice(), b"10.10.10.2".as_slice())
                    .attribute(b"count".as_slice(), b"4".as_slice())
            },
            (),
        )
        .await;
    let mut saw_reply = false;
    while let Some(result) = stream.next().await {
        eprintln!("ping result: {result:?}");
        if let mikrotik_api::simple::SimpleResult::Sentence(row) = &result {
            if row.get("received").and_then(|v| v.as_deref()) == Some("1") {
                saw_reply = true;
            }
        }
    }
    assert!(saw_reply, "node a never got a ping reply from node b over the cabled NIC");
}

/// Validates `scenario::boot_scenario`'s TopologyBuilder -> VM-wiring
/// derivation itself (NIC slot counting from `DeviceType`, cable ->
/// listen/connect assignment, boot ordering) - the lower-level cabling
/// primitive this builds on is already covered by
/// `two_nodes_directly_cabled_can_ping`.
#[tokio::test]
#[ignore]
async fn scenario_derived_wiring_can_ping() {
    use tokio_stream::StreamExt;

    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");

    let mut builder = TopologyBuilder::default();
    let dev_a = builder.devices.next_id();
    let dev_b = builder.devices.next_id();
    builder.devices.insert(
        dev_a,
        Device {
            name: "a".into(),
            ..Default::default()
        },
    );
    builder.devices.insert(
        dev_b,
        Device {
            name: "b".into(),
            ..Default::default()
        },
    );
    let if_a = builder.interfaces.next_id();
    builder.interfaces.insert(
        if_a,
        Interface {
            name: "ether2".into(),
            device: dev_a,
            external: Some(PhysicalPortId::Ethernet(2)),
            ..Default::default()
        },
    );
    let if_b = builder.interfaces.next_id();
    builder.interfaces.insert(
        if_b,
        Interface {
            name: "ether2".into(),
            device: dev_b,
            external: Some(PhysicalPortId::Ethernet(2)),
            ..Default::default()
        },
    );
    let cable_id = builder.cables.next_id();
    builder.cables.insert(
        cable_id,
        backend::topology::Cable {
            port_a: Box::new([backend::topology::CablePort::Interface(if_a)]),
            port_b: Box::new([backend::topology::CablePort::Interface(if_b)]),
        },
    );

    let boards = HashMap::from([(dev_a, DeviceType::RB750Gr3), (dev_b, DeviceType::RB750Gr3)]);
    let work_dir = std::env::temp_dir().join("chr-network-scenario-test");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, TEST_PASSWORD)
        .await
        .expect("scenario boot failed");

    let device_a = nodes[&dev_a].connect_api(TEST_PASSWORD).await.expect("a api");
    let device_b = nodes[&dev_b].connect_api(TEST_PASSWORD).await.expect("b api");

    run_to_done(
        &device_a,
        b"/ip/address/add",
        &[(b"address", b"10.20.20.1/24"), (b"interface", b"ether2")],
    )
    .await;
    run_to_done(
        &device_b,
        b"/ip/address/add",
        &[(b"address", b"10.20.20.2/24"), (b"interface", b"ether2")],
    )
    .await;

    let mut stream = device_a
        .send_command(
            b"/ping".as_slice(),
            |cmd| {
                cmd.attribute(b"address".as_slice(), b"10.20.20.2".as_slice())
                    .attribute(b"count".as_slice(), b"4".as_slice())
            },
            (),
        )
        .await;
    let mut saw_reply = false;
    while let Some(result) = stream.next().await {
        eprintln!("ping result: {result:?}");
        if let mikrotik_api::simple::SimpleResult::Sentence(row) = &result {
            if row.get("received").and_then(|v| v.as_deref()) == Some("1") {
                saw_reply = true;
            }
        }
    }
    assert!(
        saw_reply,
        "scenario-derived wiring never delivered a ping reply between the two nodes"
    );
}

async fn run_to_done(
    device: &mikrotik_api::prelude::MikrotikDevice<mikrotik_api::simple::SimpleResult>,
    command: &'static [u8],
    attrs: &[(&'static [u8], &'static [u8])],
) {
    use tokio_stream::StreamExt;
    let attrs = attrs.to_vec();
    let mut stream = device
        .send_command(
            command,
            move |mut cmd| {
                for (k, v) in &attrs {
                    cmd = cmd.attribute(*k, *v);
                }
                cmd
            },
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        eprintln!("{command:?} -> {result:?}");
    }
}
