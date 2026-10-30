//! The default route of an internet uplink reaches the network over OSPF:
//! `internet -- router -- inner`. `internet` is the provider, set up by hand with a DHCP server;
//! `router` has its uplink tagged `internet`, `nat` and `dhcp-client` and the custom field
//! `propagate_default_route`, so it gets its default from DHCP (through its own perimeter
//! firewall) and originates it; `inner` learns it over OSPF.

use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::{wait_for_ospf_full, wait_until};
use backend::device::ros::graphql::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, IpAddressData, PhysicalPortId, TopologyHolder,
};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::ReferenceType;
use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation, apply_mutation};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio_stream::StreamExt;

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
    let device = |builder: &mut TopologyBuilder, name: &str, loopback: Option<&str>, propagate: bool| {
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
async fn apply_directly(node: &RunningNode, device: &backend::topology::access::device::DeviceAccess) {
    let client = mikrotik_model::MikrotikDevice::connect(
        ("127.0.0.1", node.api_port),
        b"admin",
        Some(encode_latin1_lossy(crate::TEST_PASSWORD)),
    )
    .await
    .expect("typed api");
    let mut cfg = DeviceCfg::fetch_with_board_model(&client, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator).expect("generate");
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
    let simple = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    for mutation in sorted {
        let moves_management = mutation.resource == b"ip/dhcp-client".as_slice()
            && matches!(&mutation.operation, ResourceMutationOperation::UpdateByKey(key) if key.key == b".id".as_slice());
        if mutation.resource == b"ip/dhcp-client".as_slice() && !moves_management && on_management_port(mutation) {
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
                    .filter(|f| ![b"bandwidth".as_slice(), b"disable-running-check"].contains(&f.key))
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
        apply_mutation(&simple, &mutation)
            .await
            .unwrap_or_else(|e| panic!("[{}] apply: {e:#}", node.name));
    }
}

/// The routes to `dst` in the node's `menu` (`ip` or `ipv6`), as gateway and flags. The
/// harness's management port has a DHCP default of its own (distance 1), which stays active: the
/// checks look for the route by its gateway.
async fn default_routes(device: &MikrotikDevice<SimpleResult>, menu: &str, dst: &str) -> Vec<String> {
    run(device, &format!("/{menu}/route/print"), &[])
        .await
        .into_iter()
        .filter(|row| row.get("dst-address").cloned().flatten().as_deref() == Some(dst))
        .map(|row| {
            let flag = |name: &'static str| {
                if row.get(name).cloned().flatten().as_deref() == Some("true") { name } else { "" }
            };
            format!(
                "{} {} {} {} {}",
                row.get("gateway").cloned().flatten().unwrap_or_default(),
                flag("active"),
                flag("dhcp"),
                flag("ospf"),
                flag("static"),
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
    let internet_api = nodes[&internet].connect_api(crate::TEST_PASSWORD).await.expect("internet api");
    for (command, attributes) in [
        ("/ip/address/add", &[("address", "100.64.0.1/29"), ("interface", "ether2")][..]),
        ("/ip/pool/add", &[("name", "isp"), ("ranges", "100.64.0.2-100.64.0.6")]),
        ("/ip/dhcp-server/network/add", &[("address", "100.64.0.0/29"), ("gateway", "100.64.0.1")]),
        ("/ip/dhcp-server/add", &[("name", "isp"), ("interface", "ether2"), ("address-pool", "isp")]),
    ] {
        run(&internet_api, command, attributes).await;
    }

    apply_directly(&nodes[&router], &router_access).await;
    apply_directly(&nodes[&inner], &inner_access).await;

    let router_api = nodes[&router].connect_api(crate::TEST_PASSWORD).await.expect("router api");
    let inner_api = nodes[&inner].connect_api(crate::TEST_PASSWORD).await.expect("inner api");
    wait_for_ospf_full(&inner_api, "default-v2", Duration::from_secs(90))
        .await
        .expect("ospf between router and inner");
    let clients = run(&router_api, "/ip/dhcp-client/print", &[]).await;
    eprintln!("router dhcp-clients: {clients:#?}");
    eprintln!("router ospf: {:#?}", run(&router_api, "/routing/ospf/instance/print", &[]).await);
    let router_has = wait_until(
        || async { Ok(default_routes(&router_api, "ip", "0.0.0.0/0").await.iter().any(|r| r.starts_with("100.64.0.1 ") && r.contains("dhcp"))) },
        Duration::from_secs(60),
    )
    .await;
    eprintln!("router default: {:?}", default_routes(&router_api, "ip", "0.0.0.0/0").await);
    router_has.expect("the router never got its default route from DHCP");
    let inner_has = wait_until(
        || async {
            Ok(default_routes(&inner_api, "ip", "0.0.0.0/0")
                .await
                .iter()
                .any(|r| r.starts_with("10.0.41.0%") && r.contains("ospf")))
        },
        Duration::from_secs(60),
    )
    .await;
    eprintln!("inner default: {:?}", default_routes(&inner_api, "ip", "0.0.0.0/0").await);
    eprintln!("router lsa: {:#?}", run(&router_api, "/routing/ospf/lsa/print", &[]).await);
    inner_has.expect("inner never learned the default route over OSPF");
}
