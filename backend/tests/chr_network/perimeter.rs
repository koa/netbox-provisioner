//! Scope: CHR tests of the perimeter firewall; nothing else.
//!
//! The perimeter firewall (`docs/firewall.md`) on a CHR between the internet and a client:
//! `internet -- router -- client`. `internet` stands for the internet and for the provisioner
//! coming from there (its SOCKS proxy, from `100.64.0.1`, the `api_networks`); `router` has its
//! uplink tagged `internet` and `nat`. The router's config goes on through `internet`'s proxy
//! to the uplink address, so the apply crosses the firewall it sets up, and a new connection
//! the same way must get in after it: the provisioner doesn't lock itself out.
//!
//! `internet` and `client` are set up by hand (addresses and routes), only the router is
//! generated. A NetBox service with `forward` opens the client's WebFig (`tcp/80`) from the
//! internet: forwarded at the uplink over IPv4, let through to its address over IPv6.

use crate::chr_network::apply::{apply_mutations_via_entry, enable_socks};
use crate::chr_network::node::{add, add_address, answers_ping, login_over};
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::wait_until;
use backend::Error;
use backend::device::access::{Session, field, observe};
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::access::device::DeviceAccess;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, ForwardData, Interface, IpAddressData, PhysicalPortId,
    SiteData, TopologyHolder,
};
use ipnet::IpNet;
use mikrotik_model::action::FetchProbe;
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::{
    IpFirewallAddressList, IpFirewallFilter, IpFirewallNat, IpRouteCfg, Ipv6AddressCfg,
    Ipv6FirewallAddressList, Ipv6FirewallFilter, Ipv6RouteCfg, ReferenceType,
};
use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation, collect_resource};
use mikrotik_model::value::IpOrInterface;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;
use tokio_socks::tcp::Socks5Stream;

/// The router's uplink address, where the provisioner reaches it from `internet`.
const UPLINK: &str = "100.64.0.2";
/// `internet`'s address: the provisioner's (`api_networks`).
const PROVISIONER: &str = "100.64.0.1";
/// Another address of `internet`: someone else on the internet.
const STRANGER: &str = "100.64.0.3";
const STRANGER_V6: &str = "2001:db8:64::3";
const CLIENT_V6: &str = "2001:db8:31::2";
const CLIENT_V4: &str = "10.0.31.2";

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
    // the router's site, whose router with `nat` forwards to the client
    let site = builder.sites.next_id();
    builder.sites.insert(
        site,
        SiteData {
            name: "site".into(),
            devices: Box::new([router, client]),
        },
    );
    for id in [router, client] {
        builder.devices.get_mut(&id).unwrap().site = Some(site);
    }
    let addresses = [CLIENT_V4, CLIENT_V6].map(|ip| address(&mut builder, &format!("{ip}/24")));
    let service = builder.forwards.next_id();
    builder.forwards.insert(
        service,
        ForwardData {
            name: "webfig".into(),
            device: Some(client),
            site: None,
            addresses: addresses.into(),
            ports: Box::new(["tcp/80".parse().unwrap()]),
        },
    );
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
    let uplink = port(
        &mut builder,
        router,
        2,
        &["100.64.0.2/29", "2001:db8:64::2/64"],
        true,
    );
    let lan = port(
        &mut builder,
        router,
        3,
        &["10.0.31.1/24", "2001:db8:31::1/64"],
        false,
    );
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

/// Whether `address` answers a ping from `device`.
async fn ping(device: &Session, address: &str) -> bool {
    answers_ping(device, address.parse().expect("an address"), 2).await
}

/// Adds `address` (`a:b::c/len`) to `interface`, not advertised.
async fn add_address_v6(device: &Session, address: &str, interface: &str) {
    let address = Ipv6AddressCfg {
        address: address.parse().expect("an IPv6 network"),
        interface: interface.into(),
        advertise: false,
        ..Ipv6AddressCfg::default()
    };
    add(device, &address).await.expect("IPv6 address added");
}

/// Adds a static route to `dst` over `gateway`.
async fn add_route(device: &Session, dst: &str, gateway: &str) {
    let route = IpRouteCfg {
        check_gateway: None,
        comment: None,
        dst_address: dst.parse().expect("a network"),
        gateway: IpOrInterface::Ip(gateway.parse().expect("an address")),
        local_address: None,
        routing_table: None,
        scope: None,
        suppress_hw_offload: None,
        target_scope: None,
        vrf_interface: None,
    };
    add(device, &route).await.expect("route added");
}

/// Adds a static IPv6 route to `dst` over `gateway`.
async fn add_route_v6(device: &Session, dst: &str, gateway: &str) {
    let route = Ipv6RouteCfg {
        dst_address: dst.parse().expect("an IPv6 network"),
        gateway: Some(gateway.into()),
        comment: None,
    };
    add(device, &route).await.expect("IPv6 route added");
}

/// Whether a TCP connection from `device` (from `source`) to `url` gets through: a dropped
/// one stays `connecting` until the fetch gives up, one that gets through goes on to
/// `requesting` or ends in a trap (refused, or SSH and the API speaking no HTTP).
async fn reaches(device: &Session, url: &str, source: &str) -> bool {
    let probe = FetchProbe {
        url,
        src_address: Some(source.parse().expect("an address")),
        duration: Some(Duration::from_secs(6)),
        check_certificate: false,
    };
    match observe(device, &probe).await {
        Ok(rows) => {
            eprintln!("fetch {url} from {source}: {rows:?}");
            rows.iter()
                .any(|row| field(row, "status").is_some_and(|status| status != "connecting"))
        }
        Err(Error::Refused(message)) => {
            eprintln!("fetch {url} from {source}: {message}");
            true
        }
        Err(error) => panic!("fetch {url} from {source}: {error}"),
    }
}

/// An address family's firewall: `/ip/firewall` or `/ipv6/firewall`.
#[derive(Debug, Clone, Copy)]
enum Family {
    V4,
    V6,
}

/// The lists `address` is on in the router's address list of `family`.
async fn lists_of(device: &Session, family: Family, address: &str) -> Vec<String> {
    let mut lists: Vec<String> = match family {
        Family::V4 => collect_resource::<IpFirewallAddressList>(device)
            .await
            .expect("address list")
            .into_iter()
            .filter(|entry| entry.cfg.address.to_string().split('/').next() == Some(address))
            .map(|entry| entry.cfg.list.to_string())
            .collect(),
        Family::V6 => {
            let address: IpAddr = address.parse().expect("an address");
            collect_resource::<Ipv6FirewallAddressList>(device)
                .await
                .expect("address list")
                .into_iter()
                .filter(|entry| IpAddr::V6(entry.cfg.address.addr()) == address)
                .map(|entry| entry.cfg.list.to_string())
                .collect()
        }
    };
    lists.sort();
    lists
}

/// The comments of the rules in the router's filter of `family`, in their order.
async fn filter_comments(device: &Session, family: Family) -> Vec<String> {
    let comments = match family {
        Family::V4 => collect_resource::<IpFirewallFilter>(device)
            .await
            .expect("filter")
            .into_iter()
            .map(|rule| rule.cfg.comment)
            .collect::<Vec<_>>(),
        Family::V6 => collect_resource::<Ipv6FirewallFilter>(device)
            .await
            .expect("filter")
            .into_iter()
            .map(|rule| rule.cfg.comment)
            .collect(),
    };
    comments
        .into_iter()
        .flatten()
        .map(|comment| comment.to_string())
        .collect()
}

/// Waits for `address` on `ssh_blacklist`: an IPv6 entry shows up a moment after the
/// connection that put it there (CHR 7.24.5, the one of the previous attempt came with the
/// next).
async fn blacklisted(device: &Session, family: Family, address: &str) {
    wait_until(
        || async {
            Ok(lists_of(device, family, address)
                .await
                .iter()
                .any(|list| list == "ssh_blacklist"))
        },
        Duration::from_secs(10),
    )
    .await
    .unwrap_or_else(|e| panic!("{address} never on the {family:?} blacklist: {e}"));
}

/// The router's typed connection through `internet`'s proxy to its uplink: the
/// provisioner's way in.
async fn typed_via_internet(internet: &RunningNode) -> anyhow::Result<Session> {
    let stream = Socks5Stream::connect(
        ("127.0.0.1", internet.socks_port),
        SocketAddr::new(UPLINK.parse().unwrap(), 8728),
    )
    .await?;
    Ok(login_over(stream, crate::TEST_PASSWORD).await?)
}

/// The router's diff as the provisioner sees it through `internet`, sorted, without what a
/// CHR doesn't take (as `ospf_chain`'s bootstrap: the harness's own dhcp-client on ether1,
/// `bandwidth` and `disable-running-check`; its virtio ports also keep their flow control
/// and `l2mtu`, taking the change silently) and without updates that change nothing.
async fn router_diff(
    internet: &RunningNode,
    device: &DeviceAccess,
) -> Vec<ResourceMutation<'static>> {
    let client = tokio::time::timeout(Duration::from_secs(30), typed_via_internet(internet))
        .await
        .expect("the provisioner's connection through the uplink timed out: locked out")
        .expect("the provisioner's connection through the uplink");
    let mut cfg = DeviceCfg::fetch_with_board_model(&client, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator)
        .expect("generate");
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
                    ResourceMutationOperation::UpdateSingle
                        | ResourceMutationOperation::UpdateByKey(_)
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
    let router_access = holder
        .devices_by_id(router)
        .await
        .expect("router in the topology");

    // by hand: the internet with the provisioner and a stranger, the client behind the router,
    // and the router's uplink address, so that the provisioner reaches it there
    let internet_api = nodes[&internet]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("internet api");
    for address in ["100.64.0.1/29", "100.64.0.3/29"] {
        add_address(&internet_api, address, "ether2").await;
    }
    for address in ["2001:db8:64::1/64", "2001:db8:64::3/64"] {
        add_address_v6(&internet_api, address, "ether2").await;
    }
    add_route_v6(&internet_api, "2001:db8:31::/64", "2001:db8:64::2").await;
    enable_socks(&internet_api)
        .await
        .expect("socks on internet");
    let client_api = nodes[&client]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("client api");
    add_address(&client_api, "10.0.31.2/24", "ether2").await;
    add_address_v6(&client_api, "2001:db8:31::2/64", "ether2").await;
    add_route(&client_api, "100.64.0.0/29", "10.0.31.1").await;
    add_route_v6(&client_api, "2001:db8:64::/64", "2001:db8:31::1").await;
    let router_api = nodes[&router]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("router api");
    add_address(&router_api, "100.64.0.2/29", "ether2").await;
    add_address_v6(&router_api, "2001:db8:31::1/64", "ether3").await;
    add_address_v6(&router_api, "2001:db8:64::2/64", "ether2").await;

    // without the firewall the internet reaches the client and the router's API
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !ping(&internet_api, CLIENT_V6).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the client never answered from the internet"
        );
    }
    assert!(reaches(&internet_api, &format!("http://{UPLINK}:8728/"), STRANGER).await);

    // the apply through the uplink: the firewall it sets up is between the provisioner and
    // the router
    let mutations = router_diff(&nodes[&internet], &router_access).await;
    for mutation in &mutations {
        eprintln!(
            "{} {:?} {:?}",
            String::from_utf8_lossy(mutation.resource),
            mutation.operation,
            mutation.fields
        );
    }
    apply_mutations_via_entry(
        nodes[&internet].socks_port,
        UPLINK.parse().unwrap(),
        &mutations,
        crate::TEST_PASSWORD,
    )
    .await
    .expect("apply through the uplink");

    // a new connection the same way gets in, and the device reports what was generated
    let left = router_diff(&nodes[&internet], &router_access).await;
    for mutation in &left {
        eprintln!(
            "left: {} {:?} {:?}",
            String::from_utf8_lossy(mutation.resource),
            mutation.operation,
            mutation.fields
        );
    }
    assert!(left.is_empty(), "the diff after the apply isn't empty");
    for family in [Family::V4, Family::V6] {
        let comments = filter_comments(&router_api, family).await;
        eprintln!("{family:?} rules: {comments:#?}");
        assert!(
            comments
                .last()
                .is_some_and(|c| c.ends_with("last forward drop"))
        );
    }

    // the client gets out (masquerade, the answers come back as established), ICMP to the
    // router gets in, the internet doesn't reach the client over IPv6 (its service opens
    // only `tcp/80`)
    assert!(
        ping(&client_api, PROVISIONER).await,
        "the client doesn't reach the internet (IPv4)"
    );
    assert!(
        ping(&client_api, "2001:db8:64::1").await,
        "the client doesn't reach the internet (IPv6)"
    );
    assert!(
        ping(&internet_api, UPLINK).await,
        "the router doesn't answer ping"
    );
    assert!(
        !ping(&internet_api, CLIENT_V6).await,
        "the internet reaches the client"
    );

    // the API: the provisioner gets in, a stranger doesn't, over IPv4 and IPv6
    assert!(
        reaches(
            &internet_api,
            &format!("http://{UPLINK}:8728/"),
            PROVISIONER
        )
        .await
    );
    assert!(!reaches(&internet_api, &format!("http://{UPLINK}:8728/"), STRANGER).await);
    assert!(!reaches(&internet_api, "http://[2001:db8:64::2]:8728/", STRANGER_V6).await);

    // the client's service with `forward`: its WebFig from the internet at the uplink (IPv4,
    // forwarded) and at its own address (IPv6), its API not
    let nat = collect_resource::<IpFirewallNat>(&router_api).await;
    eprintln!("nat: {nat:#?}");
    assert!(
        reaches(&internet_api, &format!("http://{UPLINK}/"), STRANGER).await,
        "the forward to the client (IPv4)"
    );
    assert!(
        reaches(
            &internet_api,
            &format!("http://[{CLIENT_V6}]/"),
            STRANGER_V6
        )
        .await,
        "the client's WebFig (IPv6)"
    );
    assert!(
        !reaches(
            &internet_api,
            &format!("http://[{CLIENT_V6}]:8728/"),
            STRANGER_V6
        )
        .await,
        "the client's API (IPv6)"
    );

    // SSH: the stranger's fourth new connection within a minute puts it on the blacklist, the
    // fifth doesn't get through; the provisioner's connections never count
    for attempt in 1..=4 {
        assert!(
            reaches(&internet_api, &format!("http://{UPLINK}:22/"), STRANGER).await,
            "SSH attempt {attempt} of the stranger didn't get through"
        );
    }
    blacklisted(&router_api, Family::V4, STRANGER).await;
    assert_eq!(
        lists_of(&router_api, Family::V4, STRANGER).await,
        ["ssh_blacklist", "ssh_stage1", "ssh_stage2", "ssh_stage3"]
    );
    assert!(!reaches(&internet_api, &format!("http://{UPLINK}:22/"), STRANGER).await);
    for _ in 1..=5 {
        assert!(reaches(&internet_api, &format!("http://{UPLINK}:22/"), PROVISIONER).await);
    }
    assert!(
        lists_of(&router_api, Family::V4, PROVISIONER)
            .await
            .is_empty()
    );
    // the same for IPv6
    for _ in 1..=4 {
        assert!(reaches(&internet_api, "http://[2001:db8:64::2]:22/", STRANGER_V6).await);
    }
    blacklisted(&router_api, Family::V6, STRANGER_V6).await;
    assert!(!reaches(&internet_api, "http://[2001:db8:64::2]:22/", STRANGER_V6).await);
}
