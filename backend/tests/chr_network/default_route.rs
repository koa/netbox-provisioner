//! Scope: CHR test of an internet uplink's default route over OSPF; nothing else.
//!
//! The default route of an internet uplink reaches the network over OSPF:
//! `internet -- router -- inner`. `internet` is the provider, set up by hand with a DHCP server;
//! `router` has its uplink tagged `internet`, `nat` and `dhcp-client` and the custom field
//! `propagate_default_route`, so it gets its default from DHCP (through its own perimeter
//! firewall) and originates it; `inner` learns it over OSPF.

use crate::chr_network::node::add;
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::{wait_for_ospf_full, wait_until};
use backend::device::access::Session;
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, IpAddressData, PhysicalPortId, TopologyHolder,
};
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::{
    IpAddressCfg, IpDhcpClientState, IpDhcpServerBootpLeaseTime, IpDhcpServerBootpSupport,
    IpDhcpServerCfg, IpDhcpServerNetworkCfg, IpDhcpServerUseRadius, IpPoolCfg, IpRoute,
    ReferenceType, RoutingOspfInstanceCfg, RoutingOspfLsaState,
};
use mikrotik_model::resource::{
    ResourceMutation, ResourceMutationOperation, apply_mutation, collect_resource,
};
use mikrotik_model::value::RosValue;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

struct Net {
    builder: TopologyBuilder,
    internet: DeviceId,
    router: DeviceId,
    inner: DeviceId,
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
    let device =
        |builder: &mut TopologyBuilder, name: &str, loopback: Option<&str>, propagate: bool| {
            let id = builder.devices.next_id();
            let loopback = loopback.map(|ip| address(builder, ip));
            builder.devices.insert(
                id,
                Device {
                    name: name.into(),
                    primary_ip_v4: loopback,
                    loopback_ip: loopback,
                    has_routeros: true,
                    propagate_default_route: propagate,
                    ..Default::default()
                },
            );
            id
        };
    let internet = device(&mut builder, "internet", None, false);
    let router = device(&mut builder, "router", Some("192.168.255.10/32"), true);
    let inner = device(&mut builder, "inner", Some("192.168.255.11/32"), false);
    let mut port = |device, ether: u16, ips: &[&str], tags: (bool, bool)| {
        let ips = ips.iter().map(|ip| address(&mut builder, ip)).collect();
        let (uplink, ospf) = tags;
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{ether}").into_boxed_str(),
                device,
                external: Some(PhysicalPortId::Ethernet(ether)),
                ips,
                internet: uplink,
                nat: uplink,
                enable_dhcp_client: uplink,
                use_ospf: ospf,
                ..Default::default()
            },
        );
        id
    };
    let internet_port = port(internet, 2, &[], (false, false));
    let uplink = port(router, 2, &[], (true, false));
    let transfer = port(router, 3, &["10.0.41.0/31"], (false, true));
    let inner_port = port(inner, 2, &["10.0.41.1/31"], (false, true));
    for (a, b) in [(internet_port, uplink), (transfer, inner_port)] {
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
        inner,
    }
}

/// Whether the dhcp-client mutation is about the harness's management port ether1 (its own
/// client there, which the generator would remove and so cut the test off).
fn on_management_port(mutation: &ResourceMutation) -> bool {
    let named = |value: &[u8]| value == b"ether1".as_slice();
    match &mutation.operation {
        ResourceMutationOperation::Add => !mutation
            .fields
            .iter()
            .any(|f| f.key == b"interface".as_slice() && !named(&f.value)),
        // by `.id`: the CHR's only client before the apply is the management port's
        ResourceMutationOperation::RemoveByKey(_) => true,
        ResourceMutationOperation::UpdateByKey(key) => named(&key.value),
        _ => true,
    }
}

/// Generates the node's config and applies it directly (the scenario's bootstrap), without
/// what a CHR doesn't take (`bandwidth`, `disable-running-check`) and the management client.
pub async fn apply_directly(
    node: &RunningNode,
    device: &backend::topology::access::device::DeviceAccess,
) {
    let client = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    let mut cfg = DeviceCfg::fetch_with_board_model(&client, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator)
        .expect("generate");
    // as `generate_for`, without `api_networks`
    cfg.setup_services(None);
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
    for mutation in sorted {
        let moves_management = mutation.resource == b"ip/dhcp-client".as_slice()
            && matches!(&mutation.operation, ResourceMutationOperation::UpdateByKey(key) if key.key == b".id".as_slice());
        if mutation.resource == b"ip/dhcp-client".as_slice()
            && !moves_management
            && on_management_port(mutation)
        {
            continue;
        }
        let mutation = if moves_management {
            // the generator takes the management port's client for the uplink (an entry it
            // doesn't want, moved): that would cut the harness off, so a new one instead
            ResourceMutation {
                operation: ResourceMutationOperation::Add,
                ..mutation.clone()
            }
        } else if mutation.resource == b"interface/ethernet".as_slice() {
            ResourceMutation {
                fields: mutation
                    .fields
                    .iter()
                    .filter(|f| {
                        ![b"bandwidth".as_slice(), b"disable-running-check"].contains(&f.key)
                    })
                    .cloned()
                    .collect(),
                ..mutation.clone()
            }
        } else {
            mutation.clone()
        };
        eprintln!(
            "[{}] {} {:?} {:?}",
            node.name,
            String::from_utf8_lossy(mutation.resource),
            mutation.operation,
            mutation.fields
        );
        apply_mutation(&client, &mutation)
            .await
            .unwrap_or_else(|e| panic!("[{}] apply: {e:#}", node.name));
    }
}

/// The IPv4 routes to `dst` in the node, as gateway and flags. The harness's management port
/// has a DHCP default of its own (distance 1), which stays active: the checks look for the
/// route by its gateway.
async fn default_routes(device: &Session, dst: &str) -> Vec<String> {
    let dst = dst.parse().expect("a network");
    collect_resource::<IpRoute>(device)
        .await
        .expect("routes")
        .iter()
        .filter(|route| route.cfg.dst_address == dst)
        .map(|route| {
            let flag = |on: Option<bool>, name: &'static str| match on {
                Some(true) => name,
                _ => "",
            };
            format!(
                "{} {} {} {} {}",
                String::from_utf8_lossy(&route.cfg.gateway.encode_ros()),
                flag(route.status.active, "active"),
                flag(route.status.dhcp, "dhcp"),
                flag(route.status.ospf, "ospf"),
                flag(route.status._static, "static"),
            )
        })
        .collect()
}

#[tokio::test]
#[ignore]
async fn the_default_route_of_an_internet_uplink_goes_out_over_ospf() {
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
        inner,
    } = build();
    let boards = HashMap::from([
        (internet, DeviceType::RB750Gr3),
        (router, DeviceType::RB750Gr3),
        (inner, DeviceType::RB750Gr3),
    ]);
    let work_dir = Path::new("/tmp").join("chr-network-default-route-test");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    let router_access = holder.devices_by_id(router).await.unwrap();
    let inner_access = holder.devices_by_id(inner).await.unwrap();

    // by hand: the provider hands out an address and its gateway by DHCP
    let internet_api = nodes[&internet]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("internet api");
    let address = IpAddressCfg::new("100.64.0.1/29".parse().unwrap(), "ether2".into());
    add(&internet_api, &address).await.expect("address");
    let mut pool = IpPoolCfg::new("isp".into());
    pool.ranges = ["100.64.0.2-100.64.0.6".into()].into();
    add(&internet_api, &pool).await.expect("pool");
    let mut network = IpDhcpServerNetworkCfg::new("100.64.0.0/29".parse().unwrap(), false, 29);
    network.gateway = ["100.64.0.1".parse().unwrap()].into();
    add(&internet_api, &network).await.expect("dhcp network");
    let server = IpDhcpServerCfg::new(
        "isp".into(),
        IpDhcpServerBootpLeaseTime::Forever,
        IpDhcpServerBootpSupport::Static,
        false,
        "ether2".into(),
        "isp".into(),
        IpDhcpServerUseRadius::No,
    );
    add(&internet_api, &server).await.expect("dhcp server");

    apply_directly(&nodes[&router], &router_access).await;
    apply_directly(&nodes[&inner], &inner_access).await;

    let router_api = nodes[&router]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("router api");
    let inner_api = nodes[&inner]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("inner api");
    wait_for_ospf_full(&inner_api, "default-v2", Duration::from_secs(90))
        .await
        .expect("ospf between router and inner");
    let clients = collect_resource::<IpDhcpClientState>(&router_api).await;
    eprintln!("router dhcp-clients: {clients:#?}");
    eprintln!(
        "router ospf: {:#?}",
        collect_resource::<RoutingOspfInstanceCfg>(&router_api).await
    );
    let router_has = wait_until(
        || async {
            Ok(default_routes(&router_api, "0.0.0.0/0")
                .await
                .iter()
                .any(|r| r.starts_with("100.64.0.1 ") && r.contains("dhcp")))
        },
        Duration::from_secs(60),
    )
    .await;
    eprintln!(
        "router default: {:?}",
        default_routes(&router_api, "0.0.0.0/0").await
    );
    router_has.expect("the router never got its default route from DHCP");
    let inner_has = wait_until(
        || async {
            Ok(default_routes(&inner_api, "0.0.0.0/0")
                .await
                .iter()
                .any(|r| r.starts_with("10.0.41.0%") && r.contains("ospf")))
        },
        Duration::from_secs(60),
    )
    .await;
    eprintln!(
        "inner default: {:?}",
        default_routes(&inner_api, "0.0.0.0/0").await
    );
    eprintln!(
        "router lsa: {:#?}",
        collect_resource::<RoutingOspfLsaState>(&router_api).await
    );
    inner_has.expect("inner never learned the default route over OSPF");
}
