//! The perimeter firewall (`docs/firewall.md`) on a CHR between the internet and a client:
//! `internet -- router -- client`. `internet` stands for the internet and for the provisioner
//! coming from there (its SOCKS proxy, from `100.64.0.1`, the `api_networks`); `router` has its
//! uplink tagged `internet` and `nat`. The router's config goes on through `internet`'s proxy
//! to the uplink address, so the apply crosses the firewall it sets up, and a new connection
//! the same way must get in after it: the provisioner doesn't lock itself out.
//!
//! `internet` and `client` are set up by hand (addresses and routes), only the router is
//! generated.

use crate::chr_network::apply::{apply_mutations_via_entry, enable_socks};
use crate::chr_network::mac_telnet::ping;
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::wait_until;
use backend::device::ros::graphql::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::builder::TopologyBuilder;
use backend::topology::access::device::DeviceAccess;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, IpAddressData, PhysicalPortId, TopologyHolder,
};
use encoding_rs::mem::encode_latin1_lossy;
use ipnet::IpNet;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::ReferenceType;
use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio_socks::tcp::Socks5Stream;
use tokio_stream::StreamExt;

/// The router's uplink address, where the provisioner reaches it from `internet`.
const UPLINK: &str = "100.64.0.2";
/// `internet`'s address: the provisioner's (`api_networks`).
const PROVISIONER: &str = "100.64.0.1";
/// Another address of `internet`: someone else on the internet.
const STRANGER: &str = "100.64.0.3";
const STRANGER_V6: &str = "2001:db8:64::3";
const CLIENT_V6: &str = "2001:db8:31::2";

struct Net {
    builder: TopologyBuilder,
    internet: DeviceId,
    router: DeviceId,
    client: DeviceId,
}

fn build() -> Net {
    let mut builder = TopologyBuilder::default();
    let address = |builder: &mut TopologyBuilder, ip: &str| {
        let id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            id,
            IpAddressData {
                ip: ip.parse().unwrap(),
                interface: None,
                prefix: None,
            },
        );
        id
    };
    let device = |builder: &mut TopologyBuilder, name: &str, loopback: Option<&str>| {
        let id = builder.devices.next_id();
        let loopback = loopback.map(|ip| address(builder, ip));
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                primary_ip_v4: loopback,
                loopback_ip: loopback,
                has_routeros: true,
                ..Default::default()
            },
        );
        id
    };
    let internet = device(&mut builder, "internet", None);
    let router = device(&mut builder, "router", Some("192.168.255.10/32"));
    let client = device(&mut builder, "client", None);
    let port = |builder: &mut TopologyBuilder, device, ether: u16, ips: &[&str], internet: bool| {
        let ips = ips.iter().map(|ip| address(builder, ip)).collect();
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{ether}").into_boxed_str(),
                device,
                external: Some(PhysicalPortId::Ethernet(ether)),
                ips,
                nat: internet,
                internet,
                ..Default::default()
            },
        );
        id
    };
    // only the router's addresses count, the others' ports are there for the cables
    let internet_port = port(&mut builder, internet, 2, &[], false);
    let uplink = port(&mut builder, router, 2, &["100.64.0.2/29", "2001:db8:64::2/64"], true);
    let lan = port(&mut builder, router, 3, &["10.0.31.1/24", "2001:db8:31::1/64"], false);
    let client_port = port(&mut builder, client, 2, &[], false);
    for (a, b) in [(internet_port, uplink), (lan, client_port)] {
        let id = builder.cables.next_id();
        builder.cables.insert(
            id,
            Cable {
                port_a: Box::new([CablePort::Interface(a)]),
                port_b: Box::new([CablePort::Interface(b)]),
                kind: None,
            },
        );
    }
    Net {
        builder,
        internet,
        router,
        client,
    }
}

/// Runs `command` with `attributes`, giving back its rows; a trap fails the test.
async fn run(
    device: &MikrotikDevice<SimpleResult>,
    command: &str,
    attributes: &[(&str, &str)],
) -> Vec<HashMap<Box<str>, Option<Box<str>>>> {
    let mut s = device
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
    let mut rows = Vec::new();
    while let Some(r) = s.next().await {
        match r {
            SimpleResult::Sentence(row) => rows.push(row),
            other => panic!("{command}: {other:?}"),
        }
    }
    rows
}

/// Whether a TCP connection from `device` (from `source`) to `url` gets through: a dropped
/// one stays `connecting` until the fetch gives up, one that gets through goes on to
/// `requesting` (or fails on the answer, SSH and the API speak no HTTP).
async fn reaches(device: &MikrotikDevice<SimpleResult>, url: &str, source: &str) -> bool {
    let mut s = device
        .send_command(
            b"/tool/fetch".as_slice(),
            |cmd| {
                cmd.attribute(b"url".as_slice(), url.as_bytes())
                    .attribute(b"src-address".as_slice(), source.as_bytes())
                    .attribute(b"keep-result".as_slice(), b"no".as_slice())
                    .attribute(b"duration".as_slice(), b"6s".as_slice())
            },
            (),
        )
        .await;
    let mut reached = false;
    while let Some(r) = s.next().await {
        eprintln!("fetch {url} from {source}: {r:?}");
        match r {
            SimpleResult::Sentence(row) => {
                let status = row.get("status").cloned().flatten().unwrap_or_default();
                reached |= &*status != "connecting";
            }
            // the answer of a port speaking no HTTP
            SimpleResult::Trap { message, .. } => reached |= message.contains("parsing"),
            SimpleResult::Error(_) => {}
        }
    }
    reached
}

/// The lists `address` is on in the router's `address-list` menu (`ip` or `ipv6`).
async fn lists_of(device: &MikrotikDevice<SimpleResult>, menu: &str, address: &str) -> Vec<String> {
    let mut lists = run(device, &format!("/{menu}/firewall/address-list/print"), &[])
        .await
        .into_iter()
        .filter(|row| {
            row.get("address")
                .cloned()
                .flatten()
                .is_some_and(|a| a.split('/').next() == Some(address))
        })
        .filter_map(|row| row.get("list").cloned().flatten().map(String::from))
        .collect::<Vec<_>>();
    lists.sort();
    lists
}

/// Waits for `address` on `ssh_blacklist`: an IPv6 entry shows up a moment after the
/// connection that put it there (CHR 7.24.5, the one of the previous attempt came with the
/// next).
async fn blacklisted(device: &MikrotikDevice<SimpleResult>, menu: &str, address: &str) {
    wait_until(
        || async { Ok(lists_of(device, menu, address).await.iter().any(|list| list == "ssh_blacklist")) },
        Duration::from_secs(10),
    )
    .await
    .unwrap_or_else(|e| panic!("{address} never on the {menu} blacklist: {e}"));
}

/// The router's typed connection through `internet`'s proxy to its uplink: the
/// provisioner's way in.
async fn typed_via_internet(internet: &RunningNode) -> anyhow::Result<mikrotik_model::MikrotikDevice> {
    let stream = Socks5Stream::connect(
        ("127.0.0.1", internet.socks_port),
        SocketAddr::new(UPLINK.parse().unwrap(), 8728),
    )
    .await?;
    Ok(mikrotik_model::MikrotikDevice::connect_with_stream(
        stream,
        b"admin",
        Some(encode_latin1_lossy(crate::TEST_PASSWORD)),
    )
    .await?)
}

/// The router's diff as the provisioner sees it through `internet`, sorted, without what a
/// CHR doesn't take (as `ospf_chain`'s bootstrap: the harness's own dhcp-client on ether1,
/// `bandwidth` and `disable-running-check`; its virtio ports also keep their flow control
/// and `l2mtu`, taking the change silently) and without updates that change nothing.
async fn router_diff(internet: &RunningNode, device: &DeviceAccess) -> Vec<ResourceMutation<'static>> {
    let client = tokio::time::timeout(Duration::from_secs(30), typed_via_internet(internet))
        .await
        .expect("the provisioner's connection through the uplink timed out: locked out")
        .expect("the provisioner's connection through the uplink");
    let mut cfg = DeviceCfg::fetch_with_board_model(&client, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator).expect("generate");
    let api_networks = [
        format!("{PROVISIONER}/32").parse::<IpNet>().unwrap(),
        "2001:db8:64::1/128".parse().unwrap(),
    ];
    // as `generate_for` with these `api_networks`
    cfg.setup_services(Some(&api_networks));
    cfg.allow_provisioner(&api_networks);
    let mutations = cfg.generate_mutations().expect("mutations");
    let sorted = ResourceMutation::sort_mutations_with_provided_dependencies(
        mutations.as_ref(),
        [
            (ReferenceType::Interface, b"lo".into()),
            (ReferenceType::Interface, b"all".into()),
            (ReferenceType::RoutingTable, b"main".into()),
            (ReferenceType::FirewallChain, b"input".into()),
            (ReferenceType::FirewallChain, b"output".into()),
            (ReferenceType::FirewallChain, b"forward".into()),
        ],
    )
    .expect("sort");
    sorted
        .into_iter()
        .filter(|m| m.resource != b"ip/dhcp-client".as_slice())
        .map(|m| {
            let fields = m
                .fields
                .iter()
                .filter(|f| {
                    m.resource != b"interface/ethernet".as_slice()
                        || ![
                            b"bandwidth".as_slice(),
                            b"disable-running-check",
                            b"tx-flow-control",
                            b"rx-flow-control",
                            b"l2mtu",
                        ]
                        .contains(&f.key)
                })
                .cloned()
                .map(|f| f.into_owned())
                .collect::<Box<[_]>>();
            ResourceMutation {
                fields,
                ..m.clone()
            }
            .into_owned()
        })
        .filter(|m| {
            !m.fields.is_empty()
                || !matches!(
                    m.operation,
                    ResourceMutationOperation::UpdateSingle | ResourceMutationOperation::UpdateByKey(_)
                )
        })
        .collect()
}

#[tokio::test]
#[ignore]
async fn perimeter_firewall_keeps_the_internet_out_but_the_provisioner() {
    if crate::chr_network::qemu::nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = crate::chr_network::golden_image::ensure_golden_image(
        &crate::cache_dir(),
        crate::TEST_PASSWORD,
    )
    .await
    .expect("golden image build failed");
    let Net {
        builder,
        internet,
        router,
        client,
    } = build();
    let boards = HashMap::from([
        (internet, DeviceType::RB750Gr3),
        (router, DeviceType::RB750Gr3),
        (client, DeviceType::RB750Gr3),
    ]);
    let work_dir = Path::new("/tmp").join("chr-network-perimeter-test");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    // once: the holder would fetch NetBox again after a while
    let router_access = holder.devices_by_id(router).await.expect("router in the topology");

    // by hand: the internet with the provisioner and a stranger, the client behind the router,
    // and the router's uplink address, so that the provisioner reaches it there
    let internet_api = nodes[&internet].connect_api(crate::TEST_PASSWORD).await.expect("internet api");
    for address in ["100.64.0.1/29", "100.64.0.3/29"] {
        run(&internet_api, "/ip/address/add", &[("address", address), ("interface", "ether2")]).await;
    }
    for address in ["2001:db8:64::1/64", "2001:db8:64::3/64"] {
        run(&internet_api, "/ipv6/address/add", &[("address", address), ("interface", "ether2"), ("advertise", "no")]).await;
    }
    run(&internet_api, "/ipv6/route/add", &[("dst-address", "2001:db8:31::/64"), ("gateway", "2001:db8:64::2")]).await;
    enable_socks(&internet_api).await.expect("socks on internet");
    let client_api = nodes[&client].connect_api(crate::TEST_PASSWORD).await.expect("client api");
    run(&client_api, "/ip/address/add", &[("address", "10.0.31.2/24"), ("interface", "ether2")]).await;
    run(&client_api, "/ipv6/address/add", &[("address", "2001:db8:31::2/64"), ("interface", "ether2"), ("advertise", "no")]).await;
    run(&client_api, "/ip/route/add", &[("dst-address", "100.64.0.0/29"), ("gateway", "10.0.31.1")]).await;
    run(&client_api, "/ipv6/route/add", &[("dst-address", "2001:db8:64::/64"), ("gateway", "2001:db8:31::1")]).await;
    let router_api = nodes[&router].connect_api(crate::TEST_PASSWORD).await.expect("router api");
    run(&router_api, "/ip/address/add", &[("address", "100.64.0.2/29"), ("interface", "ether2")]).await;
    run(&router_api, "/ipv6/address/add", &[("address", "2001:db8:31::1/64"), ("interface", "ether3"), ("advertise", "no")]).await;
    run(&router_api, "/ipv6/address/add", &[("address", "2001:db8:64::2/64"), ("interface", "ether2"), ("advertise", "no")]).await;

    // without the firewall the internet reaches the client and the router's API
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !ping(&internet_api, CLIENT_V6).await {
        assert!(tokio::time::Instant::now() < deadline, "the client never answered from the internet");
    }
    assert!(reaches(&internet_api, &format!("http://{UPLINK}:8728/"), STRANGER).await);

    // the apply through the uplink: the firewall it sets up is between the provisioner and
    // the router
    let mutations = router_diff(&nodes[&internet], &router_access).await;
    for mutation in &mutations {
        eprintln!("{} {:?} {:?}", String::from_utf8_lossy(mutation.resource), mutation.operation, mutation.fields);
    }
    apply_mutations_via_entry(nodes[&internet].socks_port, UPLINK.parse().unwrap(), &mutations, crate::TEST_PASSWORD)
        .await
        .expect("apply through the uplink");

    // a new connection the same way gets in, and the device reports what was generated
    let left = router_diff(&nodes[&internet], &router_access).await;
    for mutation in &left {
        eprintln!("left: {} {:?} {:?}", String::from_utf8_lossy(mutation.resource), mutation.operation, mutation.fields);
    }
    assert!(left.is_empty(), "the diff after the apply isn't empty");
    for menu in ["ip", "ipv6"] {
        let comments = run(&router_api, &format!("/{menu}/firewall/filter/print"), &[])
            .await
            .into_iter()
            .filter_map(|row| row.get("comment").cloned().flatten())
            .collect::<Vec<_>>();
        eprintln!("{menu} rules: {comments:#?}");
        assert!(comments.last().is_some_and(|c| c.ends_with("last forward drop")));
    }

    // the client gets out (masquerade, the answers come back as established), ICMP to the
    // router gets in, the internet doesn't reach the client over IPv6 (no release yet, C2)
    assert!(ping(&client_api, PROVISIONER).await, "the client doesn't reach the internet (IPv4)");
    assert!(ping(&client_api, "2001:db8:64::1").await, "the client doesn't reach the internet (IPv6)");
    assert!(ping(&internet_api, UPLINK).await, "the router doesn't answer ping");
    assert!(!ping(&internet_api, CLIENT_V6).await, "the internet reaches the client");

    // the API: the provisioner gets in, a stranger doesn't, over IPv4 and IPv6
    assert!(reaches(&internet_api, &format!("http://{UPLINK}:8728/"), PROVISIONER).await);
    assert!(!reaches(&internet_api, &format!("http://{UPLINK}:8728/"), STRANGER).await);
    assert!(!reaches(&internet_api, "http://[2001:db8:64::2]:8728/", STRANGER_V6).await);

    // SSH: the stranger's fourth new connection within a minute puts it on the blacklist, the
    // fifth doesn't get through; the provisioner's connections never count
    for attempt in 1..=4 {
        assert!(
            reaches(&internet_api, &format!("http://{UPLINK}:22/"), STRANGER).await,
            "SSH attempt {attempt} of the stranger didn't get through"
        );
    }
    blacklisted(&router_api, "ip", STRANGER).await;
    assert_eq!(lists_of(&router_api, "ip", STRANGER).await, ["ssh_blacklist", "ssh_stage1", "ssh_stage2", "ssh_stage3"]);
    assert!(!reaches(&internet_api, &format!("http://{UPLINK}:22/"), STRANGER).await);
    for _ in 1..=5 {
        assert!(reaches(&internet_api, &format!("http://{UPLINK}:22/"), PROVISIONER).await);
    }
    assert!(lists_of(&router_api, "ip", PROVISIONER).await.is_empty());
    // the same for IPv6
    for _ in 1..=4 {
        assert!(reaches(&internet_api, "http://[2001:db8:64::2]:22/", STRANGER_V6).await);
    }
    blacklisted(&router_api, "ipv6", STRANGER_V6).await;
    assert!(!reaches(&internet_api, "http://[2001:db8:64::2]:22/", STRANGER_V6).await);
}
