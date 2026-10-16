//! The harness's proof scenario (project plan Schritt 7): a 3-node chain
//! `entry -- a -- target` (not 2 nodes - a direct cable would degenerate the
//! "realistic path" into a same-subnet relay that never actually exercises
//! routing). `entry` and `a` bring up OSPF between them directly (that's
//! scenario bootstrap, not the access pattern under test); once OSPF has
//! converged across both hops, an incremental config change is pushed to
//! `target` through `entry`'s SOCKS proxy - the one call in this scenario
//! that must only be reachable via `a`'s OSPF-learned routes, never a direct
//! connection to `target`.

use crate::chr_network::apply::{apply_mutations_via_entry, enable_socks};
use crate::chr_network::golden_image::GoldenImage;
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::{RunningNode, mac_of};
use crate::chr_network::wait::wait_for_ospf_full;
use backend::device::ros::graphql::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::device::terminal::Terminal;
use backend::topology::test::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, IpAddressData, PhysicalPortId, TopologyHolder,
};
use encoding_rs::mem::encode_latin1_lossy;
use ipnet::IpNet;
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::ReferenceType;
use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation, apply_mutation};
use mikrotik_model::value::KeyValuePair;
use std::borrow::Cow;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::Duration;
use tokio_stream::StreamExt;

struct ChainTopology {
    builder: TopologyBuilder,
    entry: DeviceId,
    a: DeviceId,
    target: DeviceId,
    target_loopback: IpAddr,
}

fn build_chain() -> ChainTopology {
    let mut builder = TopologyBuilder::default();
    let entry = builder.devices.next_id();
    let a = builder.devices.next_id();
    let target = builder.devices.next_id();

    let loopback = |builder: &mut TopologyBuilder, octet: u8| {
        let id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            id,
            IpAddressData {
                ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 255, octet)), 32).unwrap(),
                interface: None,
                prefix: None,
            },
        );
        id
    };
    let entry_loopback = loopback(&mut builder, 1);
    let a_loopback = loopback(&mut builder, 2);
    let target_loopback_id = loopback(&mut builder, 3);
    let target_loopback = IpAddr::V4(Ipv4Addr::new(192, 168, 255, 3));

    let link_interface =
        |builder: &mut TopologyBuilder, device: DeviceId, ether: u16, octet4: u8, net3: u8| {
            let ip_id = builder.ip_addresses.next_id();
            builder.ip_addresses.insert(
                ip_id,
                IpAddressData {
                    ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(10, 0, net3, octet4)), 24).unwrap(),
                    interface: None,
                    prefix: None,
                },
            );
            let if_id = builder.interfaces.next_id();
            builder.interfaces.insert(
                if_id,
                Interface {
                    name: format!("ether{ether}").into_boxed_str(),
                    device,
                    external: Some(PhysicalPortId::Ethernet(ether)),
                    ips: Box::new([ip_id]),
                    use_ospf: true,
                    ..Default::default()
                },
            );
            if_id
        };

    // net3 values deliberately avoid 2 (and the slirp default range in
    // general) - QEMU's "user" netdev backing each node's management NIC
    // (ether1) hands out 10.0.2.0/24 internally (gateway 10.0.2.2, guest
    // 10.0.2.15), so an interface address landing in that same /24 collides
    // with the slirp virtual router and silently breaks guest networking,
    // including the API connection the test itself depends on.
    let entry_if = link_interface(&mut builder, entry, 2, 1, 21); // 10.0.21.1/24
    let a_if1 = link_interface(&mut builder, a, 2, 2, 21); // 10.0.21.2/24
    let a_if2 = link_interface(&mut builder, a, 3, 1, 22); // 10.0.22.1/24
    let target_if = link_interface(&mut builder, target, 2, 2, 22); // 10.0.22.2/24

    builder.devices.insert(
        entry,
        Device {
            name: "entry".into(),
            primary_ip_v4: Some(entry_loopback),
            loopback_ip: Some(entry_loopback),
            has_routeros: true,
            ..Default::default()
        },
    );
    builder.devices.insert(
        a,
        Device {
            name: "a".into(),
            primary_ip_v4: Some(a_loopback),
            loopback_ip: Some(a_loopback),
            has_routeros: true,
            ..Default::default()
        },
    );
    builder.devices.insert(
        target,
        Device {
            name: "target".into(),
            primary_ip_v4: Some(target_loopback_id),
            loopback_ip: Some(target_loopback_id),
            has_routeros: true,
            ..Default::default()
        },
    );

    let cable1 = builder.cables.next_id();
    builder.cables.insert(
        cable1,
        Cable {
            port_a: Box::new([CablePort::Interface(entry_if)]),
            port_b: Box::new([CablePort::Interface(a_if1)]),
        },
    );
    let cable2 = builder.cables.next_id();
    builder.cables.insert(
        cable2,
        Cable {
            port_a: Box::new([CablePort::Interface(a_if2)]),
            port_b: Box::new([CablePort::Interface(target_if)]),
        },
    );

    ChainTopology {
        builder,
        entry,
        a,
        target,
        target_loopback,
    }
}

/// Fetches, generates and applies a node's own config directly against
/// itself - scenario bootstrap (bringing the network up at all), not the
/// access pattern under test. Real deployments bring a device up some other
/// way (on-site, or a one-time provisioning step) before it ever becomes
/// reachable only through routing; this harness isn't trying to prove that
/// part is safe, only that a later in-place change through the constrained
/// path is.
async fn bootstrap_node_directly(
    node: &RunningNode,
    holder: &TopologyHolder,
    device_id: DeviceId,
    password: &str,
) -> anyhow::Result<()> {
    let device_access = holder
        .devices_by_id(device_id)
        .await
        .expect("device not found in topology");
    let typed_client: mikrotik_model::MikrotikDevice = mikrotik_model::MikrotikDevice::connect(
        ("127.0.0.1", node.api_port),
        b"admin",
        Some(encode_latin1_lossy(password)),
    )
    .await?;
    eprintln!("  [{}] fetching current config...", node.name);
    // CHR has no /system/routerboard at all (it's not a physical
    // RouterBOARD), so the board model has to be supplied explicitly rather
    // than detected live - it's the same board this node was booted with.
    let mut cfg = DeviceCfg::fetch_with_board_model(&typed_client, b"RB750Gr3", &device_access).await?;
    eprintln!("  [{}] generating target config...", node.name);
    cfg.generate_from(&device_access, KeepNameGenerator)?;
    eprintln!("  [{}] generating mutations...", node.name);
    let mutations = cfg.generate_mutations()?;
    eprintln!("  [{}] sorting {} mutations...", node.name, mutations.len());
    let sorted = ResourceMutation::sort_mutations_with_provided_dependencies(
        mutations.as_ref(),
        [
            (ReferenceType::Interface, b"lo".into()),
            (ReferenceType::RoutingTable, b"main".into()),
            (ReferenceType::FirewallChain, b"input".into()),
            (ReferenceType::FirewallChain, b"output".into()),
            (ReferenceType::FirewallChain, b"forward".into()),
        ],
    )
    .map_err(|e| anyhow::anyhow!("sorting mutations failed: {e:?}"))?;

    // ether1 is this harness's own out-of-band management NIC (the hostfwd
    // slot reserved in vm::boot_node), never part of a scenario's declared
    // topology. CHR factory-defaults it to a dhcp-client, so the generator
    // always wants to remove that as unmanaged leftover - but applying that
    // removal over the very connection riding on ether1's DHCP-assigned
    // address severs the connection mid-command (the interface loses its
    // address before the reply can be sent back), hanging this bootstrap
    // step forever. Skip it here: a real device's actual management path
    // is never the interface being reconfigured by its own generated diff.
    //
    // CHR's virtio-net ports don't implement hardware bandwidth limiting at
    // all (unlike a real board's switch chip) - RouterOS rejects `bandwidth`
    // on `/interface/ethernet/set` outright on CHR, even though a real
    // RB750Gr3 accepts it. Known CHR emulation gap (alongside the
    // already-documented lack of PoE/combo/SFP+ emulation), not a generator
    // bug: strip the field for this CHR-only bootstrap rather than for the
    // product's real apply path. `disable-running-check` is the opposite
    // case: it only exists on x86/CHR, so a real board's target leaves it
    // empty and the diff would clear it here, which RouterOS rejects.
    let sorted: Vec<_> = sorted
        .into_iter()
        .filter(|m| m.resource != b"ip/dhcp-client".as_slice())
        .map(|m| {
            if m.resource == b"interface/ethernet".as_slice() {
                let fields = m
                    .fields
                    .iter()
                    .filter(|f| {
                        f.key != b"bandwidth".as_slice()
                            && f.key != b"disable-running-check".as_slice()
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                ResourceMutation {
                    fields,
                    ..m.clone()
                }
            } else {
                m.clone()
            }
        })
        .collect();

    let simple_client = node.connect_api(password).await?;
    for (i, mutation) in sorted.iter().enumerate() {
        eprintln!(
            "  [{}] applying mutation {}/{}: {} {:?}",
            node.name,
            i + 1,
            sorted.len(),
            String::from_utf8_lossy(mutation.resource),
            mutation.operation
        );
        apply_mutation(&simple_client, mutation).await?;
    }
    Ok(())
}

/// Whether a mac-telnet session from `from` to `mac` gets to the other device's prompt (the
/// client asks for login and password itself, before it reaches the other end).
async fn mac_telnet_login(from: &RunningNode, mac: &str) -> bool {
    let mut terminal = Terminal::connect(
        ([127, 0, 0, 1], from.ssh_port).into(),
        "admin",
        Some(crate::TEST_PASSWORD),
    )
    .await
    .expect("ssh");
    terminal.sync(Duration::from_secs(20)).await.expect("sync");
    terminal.send_line(&format!("/tool mac-telnet {mac}")).await.unwrap();
    terminal.expect(&["Login:"], Duration::from_secs(20)).await.expect("login prompt");
    terminal.send_line("admin").await.unwrap();
    terminal.expect(&["Password:"], Duration::from_secs(20)).await.expect("password prompt");
    terminal.send_line(crate::TEST_PASSWORD).await.unwrap();
    let connected = terminal
        .expect(&["Connected to"], Duration::from_secs(30))
        .await
        .is_ok();
    eprintln!("--- mac-telnet {mac} ---\n{}", terminal.transcript());
    terminal.close().await;
    connected
}

#[tokio::test]
#[ignore]
async fn three_node_ospf_chain_apply_via_entry() {
    if crate::chr_network::qemu::nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden: GoldenImage = crate::chr_network::golden_image::ensure_golden_image(
        &crate::cache_dir(),
        crate::TEST_PASSWORD,
    )
    .await
    .expect("golden image build failed");

    let ChainTopology {
        builder,
        entry,
        a,
        target,
        target_loopback,
    } = build_chain();

    let boards = HashMap::from([
        (entry, DeviceType::RB750Gr3),
        (a, DeviceType::RB750Gr3),
        (target, DeviceType::RB750Gr3),
    ]);
    let work_dir = Path::new("/tmp").join("chr-network-ospf-chain-test");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");

    let holder: TopologyHolder = builder.into();

    for device_id in [entry, a, target] {
        eprintln!("bootstrapping device {device_id:?} directly...");
        bootstrap_node_directly(&nodes[&device_id], &holder, device_id, crate::TEST_PASSWORD)
            .await
            .unwrap_or_else(|e| panic!("bootstrap of device {device_id:?} failed: {e:#}"));
        eprintln!("bootstrapped device {device_id:?}");
    }

    // Both hops must reach Full before the realistic-path apply below has
    // any chance of reaching target's loopback at all.
    let a_simple = nodes[&a]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("a api");
    if let Err(e) = wait_for_ospf_full(&a_simple, "default-v2", Duration::from_secs(90)).await {
        for (name, port) in [
            ("entry", nodes[&entry].api_port),
            ("a", nodes[&a].api_port),
            ("target", nodes[&target].api_port),
        ] {
            let c: mikrotik_api::prelude::MikrotikDevice<mikrotik_api::simple::SimpleResult> =
                mikrotik_api::prelude::MikrotikDevice::connect(
                    ("127.0.0.1", port),
                    b"admin",
                    Some(encode_latin1_lossy(crate::TEST_PASSWORD)),
                )
                .await
                .unwrap();
            for cmd in [
                "/routing/ospf/neighbor/print",
                "/routing/ospf/interface/print",
                "/interface/ethernet/print",
            ] {
                eprintln!("=== {name} {cmd} ===");
                let mut s = c.send_simple_command(cmd.as_bytes(), ()).await;
                while let Some(r) = s.next().await {
                    eprintln!("{r:?}");
                }
            }
        }
        panic!("ospf did not converge to Full on both hops in time: {e:#}");
    }

    let entry_simple = nodes[&entry]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("entry api");
    // entry's own route to target's loopback is what the SOCKS-proxied
    // apply below actually depends on, not just a's neighbor state.
    wait_for_ospf_full(&entry_simple, "default-v2", Duration::from_secs(30))
        .await
        .expect("entry's own ospf neighbor did not reach Full");
    enable_socks(&entry_simple)
        .await
        .expect("enable socks on entry failed");

    // mac-telnet on a's ports to its neighbors and on its ports not in NetBox (the harness's
    // management port ether1 among them), and it lets entry in over the cable
    let mut members = Vec::new();
    let mut s = a_simple
        .send_simple_command(b"/interface/list/member/print".as_slice(), ())
        .await;
    while let Some(r) = s.next().await {
        if let mikrotik_api::simple::SimpleResult::Sentence(row) = r {
            let field = |name| row.get(name).cloned().flatten().unwrap_or_default();
            assert_eq!(field("list").as_ref(), "maintenance");
            members.push(field("interface").to_string());
        }
    }
    members.sort();
    assert_eq!(members, ["e01", "e04", "e05", "ether2", "ether3"]);
    // neighbor discovery on the same list: without it the generated config left it naming
    // the factory list `LAN` it removes (discovery off)
    let mut s = a_simple
        .send_simple_command(b"/ip/neighbor/discovery-settings/print".as_slice(), ())
        .await;
    let mut discover = None;
    while let Some(r) = s.next().await {
        if let mikrotik_api::simple::SimpleResult::Sentence(row) = r {
            discover = row.get("discover-interface-list").cloned().flatten();
        }
    }
    assert_eq!(discover.as_deref(), Some("maintenance"));
    assert!(
        mac_telnet_login(&nodes[&entry], &mac_of(nodes[&a].index, 1).to_string()).await,
        "no mac-telnet from entry to a over their cable"
    );

    // The call under test: an incremental change to target, reached only
    // via entry's SOCKS proxy and a's OSPF-learned route to target's
    // loopback - never a direct connection to target. Even once OSPF
    // neighbors report Full, route installation into the FIB can lag
    // noticeably behind under this harness's software-emulated (TCG, no
    // KVM) CHR instances - the isolated two-node apply test documents the
    // same settling behavior for ARP - so retry the known-transient "General
    // SOCKS server failure" against a generous deadline rather than treating
    // it as fatal immediately.
    let proof_mutation = [ResourceMutation {
        resource: b"interface/bridge",
        operation: ResourceMutationOperation::Add,
        fields: Box::new([KeyValuePair {
            key: b"name",
            value: Cow::Borrowed(b"applied-via-ospf-chain"),
        }]),
        depends: Box::new([]),
        provides: Box::new([]),
        ordering: Default::default(),
    }];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match apply_mutations_via_entry(
            nodes[&entry].socks_port,
            target_loopback,
            &proof_mutation,
            crate::TEST_PASSWORD,
        )
        .await
        {
            Ok(()) => break,
            Err(e) if tokio::time::Instant::now() < deadline => {
                eprintln!("apply via entry attempt {attempt} failed ({e:#}), retrying...");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            Err(e) => panic!(
                "realistic-path apply to target via entry failed after {attempt} attempts: {e:#}"
            ),
        }
    }

    // Verify directly against target (assertion access, allowed) that the
    // change actually landed - proving it really went all the way through,
    // not just that the proxy call returned without error.
    let target_simple = nodes[&target]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("target api (direct, assertion-only)");
    let mut stream = target_simple
        .send_command(
            b"/interface/bridge/print".as_slice(),
            |cmd| cmd.query_equal(b"name".as_slice(), b"applied-via-ospf-chain".as_slice()),
            (),
        )
        .await;
    let mut found = false;
    while let Some(result) = stream.next().await {
        if matches!(result, mikrotik_api::simple::SimpleResult::Sentence(_)) {
            found = true;
        }
    }
    assert!(
        found,
        "config applied via entry's SOCKS proxy through a's OSPF route was not found on target"
    );
}
