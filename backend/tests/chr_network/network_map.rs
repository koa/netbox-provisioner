//! The network map's edges from a real link-state database (`backend::network_state`): a chain
//! `r1 -- r2 -- r3`, the LSDB read from r1 as the map reads it from one router. Both edges are
//! up in v2 and v3; with r3's port down the edge r2–r3 is down and r1–r2 stays up (r3's LSAs
//! stay in r1's LSDB until they age out, which must not count); back up, both are up again.

use crate::chr_network::default_route::apply_directly;
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::wait::{wait_for_ospf_full, wait_until};
use backend::network_state::edges::{Edge, EdgeState, Lsdb, edges};
use backend::network_state::lsa::Lsa;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, InterfaceId, IpAddressData, PhysicalPortId,
    TopologyHolder,
};
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::hwconfig::DeviceType;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio_stream::StreamExt;

struct Chain {
    builder: TopologyBuilder,
    routers: [DeviceId; 3],
    /// the ports of the edge r2–r3
    far_edge: (InterfaceId, InterfaceId),
}

fn build() -> Chain {
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
    let mut routers = Vec::new();
    for index in 0..3u8 {
        let loopback = address(&mut builder, &format!("192.168.255.{}/32", 31 + index));
        let device = builder.devices.next_id();
        builder.devices.insert(
            device,
            Device {
                name: format!("r{}", index + 1).into_boxed_str(),
                primary_ip_v4: Some(loopback),
                loopback_ip: Some(loopback),
                has_routeros: true,
                ..Default::default()
            },
        );
        routers.push(device);
    }
    let port = |builder: &mut TopologyBuilder, device: DeviceId, ether: u16, ip: &str| {
        let ip = address(builder, ip);
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{ether}").into_boxed_str(),
                device,
                external: Some(PhysicalPortId::Ethernet(ether)),
                ips: Box::new([ip]),
                use_ospf: true,
                ..Default::default()
            },
        );
        id
    };
    let r1 = port(&mut builder, routers[0], 2, "10.0.61.1/30");
    let r2a = port(&mut builder, routers[1], 2, "10.0.61.2/30");
    let r2b = port(&mut builder, routers[1], 3, "10.0.62.1/30");
    let r3 = port(&mut builder, routers[2], 2, "10.0.62.2/30");
    for (a, b) in [(r1, r2a), (r2b, r3)] {
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(a)]),
                port_b: Box::new([CablePort::Interface(b)]),
                kind: None,
            },
        );
    }
    Chain {
        builder,
        routers: routers.try_into().unwrap(),
        far_edge: (r2b, r3),
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
                attributes.iter().fold(cmd, |cmd, (key, value)| {
                    cmd.attribute(key.as_bytes(), value.as_bytes())
                })
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

async fn lsdb(device: &MikrotikDevice<SimpleResult>) -> Lsdb {
    run(device, "/routing/ospf/lsa/print", &[])
        .await
        .into_iter()
        .filter_map(|row| Lsa::from_row(|key| row.get(key).cloned().flatten().map(String::from)))
        .collect()
}

/// The state of the edge r1–r2 and of r2–r3.
fn states(edges: &[Edge], far_edge: (InterfaceId, InterfaceId)) -> (EdgeState, EdgeState) {
    let is_far = |edge: &&Edge| (edge.a, edge.b) == far_edge || (edge.b, edge.a) == far_edge;
    let far = edges.iter().find(is_far).expect("the edge r2–r3").state;
    let near = edges
        .iter()
        .find(|edge| !is_far(edge))
        .expect("the edge r1–r2")
        .state;
    (near, far)
}

#[tokio::test]
#[ignore]
async fn the_lsdb_gives_the_state_of_each_edge() {
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
    let Chain {
        builder,
        routers,
        far_edge,
    } = build();
    let boards: HashMap<_, _> = routers.iter().map(|r| (*r, DeviceType::RB750Gr3)).collect();
    let work_dir = Path::new("/tmp").join("chr-network-map-test");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    let mut devices = Vec::new();
    for router in routers {
        let access = holder.devices_by_id(router).await.unwrap();
        apply_directly(&nodes[&router], &access).await;
        devices.push(access);
    }
    let r1 = nodes[&routers[0]]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("r1 api");
    let r2 = nodes[&routers[1]]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("r2 api");
    let r3 = nodes[&routers[2]]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("r3 api");
    for instance in ["default-v2", "default-v3"] {
        wait_for_ospf_full(&r2, instance, Duration::from_secs(180))
            .await
            .unwrap_or_else(|e| panic!("{instance}: {e}"));
    }
    let wait_for = |want: (EdgeState, EdgeState), what: &'static str| {
        let (r1, devices) = (&r1, &devices);
        async move {
            wait_until(
                || async {
                    let got = states(&edges(devices, &lsdb(r1).await), far_edge);
                    eprintln!("{what}: {got:?}");
                    Ok(got == want)
                },
                Duration::from_secs(180),
            )
            .await
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        }
    };
    wait_for((EdgeState::Up, EdgeState::Up), "both up").await;

    run(&r3, "/interface/ethernet/disable", &[("numbers", "ether2")]).await;
    wait_until(
        || async {
            let (near, far) = states(&edges(&devices, &lsdb(&r1).await), far_edge);
            eprintln!("r3 cut off: {near:?} {far:?}");
            Ok(near == EdgeState::Up && matches!(far, EdgeState::Down(_)))
        },
        Duration::from_secs(180),
    )
    .await
    .expect("r2–r3 down, r1–r2 up");

    run(&r3, "/interface/ethernet/enable", &[("numbers", "ether2")]).await;
    wait_for((EdgeState::Up, EdgeState::Up), "both up again").await;
}
