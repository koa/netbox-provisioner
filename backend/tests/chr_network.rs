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
    pub mod apply;
    pub mod fallback;
    pub mod golden_image;
    pub mod mac_telnet;
    pub mod ospf_chain;
    pub mod qemu;
    pub mod scenario;
    pub mod serial;
    pub mod update;
    pub mod vm;
    pub mod wait;
}

use backend::topology::test::TopologyBuilder;
use backend::topology::{Device, Interface, PhysicalPortId};
use chr_network::golden_image::ensure_golden_image;
use chr_network::qemu::{free_tcp_port, nix_unavailable};
use chr_network::scenario::boot_scenario;
use chr_network::vm::{PortWiring, boot_node};
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
        if let mikrotik_api::simple::SimpleResult::Sentence(row) = &result
            && row.get("received").and_then(|v| v.as_deref()) == Some("1")
        {
            saw_reply = true;
        }
    }
    assert!(
        saw_reply,
        "node a never got a ping reply from node b over the cabled NIC"
    );
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

    let device_a = nodes[&dev_a]
        .connect_api(TEST_PASSWORD)
        .await
        .expect("a api");
    let device_b = nodes[&dev_b]
        .connect_api(TEST_PASSWORD)
        .await
        .expect("b api");

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
        if let mikrotik_api::simple::SimpleResult::Sentence(row) = &result
            && row.get("received").and_then(|v| v.as_deref()) == Some("1")
        {
            saw_reply = true;
        }
    }
    assert!(
        saw_reply,
        "scenario-derived wiring never delivered a ping reply between the two nodes"
    );
}

/// Regression test for a real finding: RouterOS's `/ip socks` defaults to
/// SOCKS4, which silently drops a SOCKS5 greeting (connection closes with no
/// bytes sent back - surfaces upstream as a confusing "unexpected end of
/// file" rather than a clear protocol-mismatch error). `enable_socks` must
/// keep requesting `version=5` for `apply.rs`'s SOCKS5 client to work at all.
#[tokio::test]
#[ignore]
async fn entry_socks_proxy_speaks_socks5() {
    use chr_network::apply::enable_socks;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-socks5-handshake-test");
    let entry = boot_node(&golden, &work_dir, "entry", &[], TEST_PASSWORD)
        .await
        .expect("entry boot failed");
    let entry_device = entry.connect_api(TEST_PASSWORD).await.expect("entry api");
    enable_socks(&entry_device)
        .await
        .expect("enable socks failed");

    let mut conn = tokio::net::TcpStream::connect(("127.0.0.1", entry.socks_port))
        .await
        .expect("tcp connect to socks port failed");
    conn.write_all(&[0x05, 0x01, 0x00])
        .await
        .expect("write socks5 greeting");
    let mut buf = [0u8; 2];
    conn.read_exact(&mut buf)
        .await
        .expect("read socks5 greeting reply");
    assert_eq!(
        buf,
        [0x05, 0x00],
        "proxy did not accept a SOCKS5 no-auth greeting"
    );
}

/// Validates the realistic-path apply helper in isolation (SOCKS proxying +
/// connect_with_stream + apply_mutation), on a simple directly-cabled
/// two-node setup rather than the full OSPF chain - that's the next thing
/// to build on top of this.
#[tokio::test]
#[ignore]
async fn apply_via_entry_socks_proxy() {
    use chr_network::apply::{apply_mutations_via_entry, enable_socks};
    use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation};
    use mikrotik_model::value::KeyValuePair;
    use std::borrow::Cow;
    use tokio_stream::StreamExt;

    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let cable_port = free_tcp_port().expect("free port");
    let work_dir = std::env::temp_dir().join("chr-network-apply-via-entry-test");

    let entry = boot_node(
        &golden,
        &work_dir,
        "entry",
        &[PortWiring::Listen(cable_port)],
        TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let target = boot_node(
        &golden,
        &work_dir,
        "target",
        &[PortWiring::Connect(cable_port)],
        TEST_PASSWORD,
    )
    .await
    .expect("target boot failed");

    let entry_device = entry.connect_api(TEST_PASSWORD).await.expect("entry api");
    let target_device = target
        .connect_api(TEST_PASSWORD)
        .await
        .expect("target api (direct, setup-only)");

    // Direct setup on both ends is allowed: giving the target an address to
    // be reachable at, and turning on the entry's SOCKS proxy.
    run_to_done(
        &target_device,
        b"/ip/address/add",
        &[(b"address", b"10.30.30.2/24"), (b"interface", b"ether2")],
    )
    .await;
    run_to_done(
        &entry_device,
        b"/ip/address/add",
        &[(b"address", b"10.30.30.1/24"), (b"interface", b"ether2")],
    )
    .await;
    enable_socks(&entry_device)
        .await
        .expect("enable socks failed");

    // Warm up ARP/neighbor resolution for the target before asking the
    // SOCKS proxy to dial it - without this, the very first connection
    // attempt through a just-addressed interface can fail with RouterOS's
    // generic "General SOCKS server failure" (observed live) rather than a
    // clear reachability error. The real Schritt 7 scenario doesn't need
    // this explicitly: waiting for OSPF to reach Full already provides the
    // same warm-up time.
    {
        let mut stream = entry_device
            .send_command(
                b"/ping".as_slice(),
                |cmd| {
                    cmd.attribute(b"address".as_slice(), b"10.30.30.2".as_slice())
                        .attribute(b"count".as_slice(), b"4".as_slice())
                },
                (),
            )
            .await;
        while let Some(result) = stream.next().await {
            eprintln!("warmup ping: {result:?}");
        }
    }

    // The actual call under test: apply to the target through the entry's
    // SOCKS proxy, never touching target's own hostfwd API port.
    let mutations = [ResourceMutation {
        resource: b"interface/bridge",
        operation: ResourceMutationOperation::Add,
        fields: Box::new([KeyValuePair {
            key: b"name",
            value: Cow::Borrowed(b"apply-via-entry-test"),
        }]),
        depends: Box::new([]),
        provides: Box::new([]),
        ordering: Default::default(),
    }];
    apply_mutations_via_entry(
        entry.socks_port,
        "10.30.30.2".parse().unwrap(),
        &mutations,
        TEST_PASSWORD,
    )
    .await
    .expect("apply via entry failed");

    // Verify directly against the target (allowed for assertions) that the
    // bridge really exists, proving the mutation actually reached it
    // through the proxy rather than silently no-opping.
    let mut stream = target_device
        .send_command(
            b"/interface/bridge/print".as_slice(),
            |cmd| cmd.query_equal(b"name".as_slice(), b"apply-via-entry-test".as_slice()),
            (),
        )
        .await;
    let mut found = false;
    while let Some(result) = stream.next().await {
        eprintln!("verify: {result:?}");
        if matches!(result, mikrotik_api::simple::SimpleResult::Sentence(_)) {
            found = true;
        }
    }
    assert!(
        found,
        "bridge applied via entry's SOCKS proxy was not found on the target"
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
