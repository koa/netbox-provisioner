//! Scope: CHR tests of a radio link's shared OSPF segment; nothing else.
//!
//! A radio link's segment: the routers at both ends and the two bridge devices of the link
//! share one network, all four speaking OSPF on it. The bridge devices get priority 0 from the
//! generator, so neither is ever DR or BDR, also not after the link was cut and came back: the
//! routers are.
//!
//! The segment is a fifth CHR as a switch (a bridge over four cables): the harness's cables are
//! point to point, and QEMU's multicast sockets would need the host's real network.

use crate::chr_network::default_route::apply_directly;
use crate::chr_network::node::{add, set_keyed};
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::wait_until;
use backend::device::access::Session;
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, DeviceRole, Interface, IpAddressData, PhysicalPortId,
    TopologyHolder,
};
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::{
    InterfaceBridgeCfg, InterfaceBridgePortCfg, InterfaceEthernetByDefaultName, OspfInterfaceState,
    OspfNeighborState, RoutingOspfInterfaceState, RoutingOspfInterfaceTemplate,
    RoutingOspfInterfaceTemplateById, RoutingOspfNeighborState,
};
use mikrotik_model::resource::collect_resource;
use std::collections::{BTreeSet, HashMap};
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

struct Segment {
    builder: TopologyBuilder,
    switch: DeviceId,
    /// the near router, the two bridge devices, the far router
    speakers: [DeviceId; 4],
}

fn build() -> Segment {
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
    // first, so it listens on all its ports
    let switch = builder.devices.next_id();
    builder.devices.insert(
        switch,
        Device {
            name: "segment".into(),
            has_routeros: true,
            ..Default::default()
        },
    );
    let mut speakers = Vec::new();
    for (index, (name, bridge)) in [
        ("near", false),
        ("bridge-a", true),
        ("bridge-b", true),
        ("far", false),
    ]
    .into_iter()
    .enumerate()
    {
        let loopback = address(&mut builder, &format!("192.168.255.{}/32", 21 + index));
        let device = builder.devices.next_id();
        builder.devices.insert(
            device,
            Device {
                name: name.into(),
                primary_ip_v4: Some(loopback),
                loopback_ip: Some(loopback),
                has_routeros: true,
                role: bridge.then_some(DeviceRole::WlanBridge),
                ..Default::default()
            },
        );
        let ip = address(&mut builder, &format!("10.0.50.{}/29", 1 + index));
        let port = builder.interfaces.next_id();
        builder.interfaces.insert(
            port,
            Interface {
                name: "ether2".into(),
                device,
                external: Some(PhysicalPortId::Ethernet(2)),
                ips: Box::new([ip]),
                use_ospf: true,
                ..Default::default()
            },
        );
        let switch_port = builder.interfaces.next_id();
        let ether = 2 + index as u16;
        builder.interfaces.insert(
            switch_port,
            Interface {
                name: format!("ether{ether}").into_boxed_str(),
                device: switch,
                external: Some(PhysicalPortId::Ethernet(ether)),
                ..Default::default()
            },
        );
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(switch_port)]),
                port_b: Box::new([CablePort::Interface(port)]),
                kind: None,
            },
        );
        speakers.push(device);
    }
    Segment {
        builder,
        switch,
        speakers: speakers.try_into().unwrap(),
    }
}

/// The node's OSPFv2 interface on `ether2`: its state and the DR it knows there.
#[derive(Debug, Default)]
struct Role {
    state: Option<OspfInterfaceState>,
    /// shown with the roles while the test waits
    #[allow(dead_code)]
    dr: Option<Ipv4Addr>,
}

impl Role {
    fn is(&self, state: OspfInterfaceState) -> bool {
        self.state.as_ref() == Some(&state)
    }
}

async fn role(device: &Session) -> Role {
    collect_resource::<RoutingOspfInterfaceState>(device)
        .await
        .expect("ospf interfaces")
        .into_iter()
        .find(|interface| {
            *interface.interface.0 == *b"ether2"
                && interface
                    .address
                    .as_ref()
                    .is_some_and(|address| address.to_string().starts_with("10.0.50."))
        })
        .map(|interface| Role {
            state: Some(interface.state),
            dr: interface.dr,
        })
        .unwrap_or_default()
}

/// Turns the Ethernet port with the factory name `port` off or on.
async fn set_port_disabled(device: &Session, port: &str, disabled: bool) {
    set_keyed(
        device,
        &port.into(),
        |ethernet: &mut InterfaceEthernetByDefaultName| ethernet.data.disabled = disabled,
    )
    .await
    .expect("port switched");
}

/// The segment booted, the switch bridging its four ports and every speaker applied: the nodes
/// (they stop when dropped), the topology and the speakers' APIs in the order of
/// [`Segment::speakers`].
async fn boot(
    test: &str,
) -> (
    HashMap<DeviceId, RunningNode>,
    TopologyHolder,
    [DeviceId; 4],
    Vec<Session>,
) {
    let golden = crate::chr_network::golden_image::ensure_golden_image(
        &crate::cache_dir(),
        crate::TEST_PASSWORD,
    )
    .await
    .expect("golden image build failed");
    let Segment {
        builder,
        switch,
        speakers,
    } = build();
    let boards: HashMap<_, _> = [switch]
        .into_iter()
        .chain(speakers)
        .map(|device| (device, DeviceType::RB750Gr3))
        .collect();
    let work_dir = Path::new("/tmp").join(test);
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();

    // by hand: the switch bridges its four ports
    let switch_api = nodes[&switch]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("switch api");
    let bridge = InterfaceBridgeCfg {
        name: "segment".into(),
        ..InterfaceBridgeCfg::default()
    };
    add(&switch_api, &bridge).await.expect("bridge");
    for ether in 2..=5 {
        let port = InterfaceBridgePortCfg {
            bridge: "segment".into(),
            interface: format!("ether{ether}").as_str().into(),
            ..InterfaceBridgePortCfg::default()
        };
        add(&switch_api, &port).await.expect("bridge port");
    }
    for speaker in speakers {
        let access = holder.devices_by_id(speaker).await.unwrap();
        apply_directly(&nodes[&speaker], &access).await;
    }
    let mut apis = Vec::new();
    for speaker in speakers {
        apis.push(
            nodes[&speaker]
                .connect_api(crate::TEST_PASSWORD)
                .await
                .expect("api"),
        );
    }
    (nodes, holder, speakers, apis)
}

#[tokio::test]
#[ignore]
async fn bridge_devices_are_never_dr_or_bdr_of_their_segment() {
    if crate::chr_network::qemu::nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let (_nodes, _, _, apis) = boot("chr-network-ospf-segment-test").await;
    let [_, bridge_a, bridge_b, _] = &apis[..] else {
        unreachable!()
    };
    let roles = || async {
        let mut roles = Vec::new();
        for api in &apis {
            roles.push(role(api).await);
        }
        eprintln!("roles (near, bridge-a, bridge-b, far): {roles:?}");
        roles
    };
    let routers_elected = |roles: &[Role]| {
        let elected =
            |role: &Role| role.is(OspfInterfaceState::Dr) || role.is(OspfInterfaceState::Bdr);
        elected(&roles[0])
            && elected(&roles[3])
            && roles[1].is(OspfInterfaceState::DrOther)
            && roles[2].is(OspfInterfaceState::DrOther)
    };

    wait_until(
        || async { Ok(routers_elected(&roles().await)) },
        Duration::from_secs(180),
    )
    .await
    .expect("the routers are DR and BDR, the bridge devices neither");

    // the DR goes away: the other router takes over, and no bridge device steps up as BDR
    let dr = if roles().await[0].is(OspfInterfaceState::Dr) {
        0
    } else {
        3
    };
    let other = 3 - dr;
    set_port_disabled(&apis[dr], "ether2", true).await;
    wait_until(
        || async { Ok(roles().await[other].is(OspfInterfaceState::Dr)) },
        Duration::from_secs(120),
    )
    .await
    .expect("the other router becomes DR");
    // longer than the dead interval (40 s) and a wait after it
    tokio::time::sleep(Duration::from_secs(60)).await;
    for api in [bridge_a, bridge_b] {
        let role = role(api).await;
        assert!(
            role.is(OspfInterfaceState::DrOther),
            "a bridge device is no BDR with one router left: {role:?}"
        );
    }

    // and back: the routers again, whichever is DR
    set_port_disabled(&apis[dr], "ether2", false).await;
    wait_until(
        || async { Ok(routers_elected(&roles().await)) },
        Duration::from_secs(180),
    )
    .await
    .expect("the routers are DR and BDR again");
}

/// Whether the node is adjacent (`Full`) with the router `router_id` in `instance`.
async fn full_with(device: &Session, instance: &str, router_id: Ipv4Addr) -> bool {
    collect_resource::<RoutingOspfNeighborState>(device)
        .await
        .expect("ospf neighbors")
        .iter()
        .any(|neighbor| {
            *neighbor.instance.0 == *instance.as_bytes()
                && neighbor.router_id == router_id
                && neighbor.state == OspfNeighborState::Full
        })
}

/// The fields the diff of the node sets on `/routing/ospf/interface-template` entries it keeps.
async fn template_updates(
    node: &RunningNode,
    device: &backend::topology::access::device::DeviceAccess,
) -> Vec<String> {
    let client = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    let mut cfg = DeviceCfg::fetch_with_board_model(&client, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator)
        .expect("generate");
    cfg.setup_services(None);
    cfg.generate_mutations()
        .expect("mutations")
        .iter()
        .filter(|m| m.resource.ends_with(b"routing/ospf/interface-template"))
        .flat_map(|m| m.fields.iter())
        .map(|field| String::from_utf8_lossy(field.key).into_owned())
        .collect()
}

/// A template with `networks=""` (berghof-haus-stall, 7.24.6) matches no network: RouterOS
/// makes no interface of it, the adjacency goes. The diff unsets it, and the apply brings the
/// adjacency back.
#[tokio::test]
#[ignore]
async fn an_empty_networks_is_unset_by_the_apply() {
    if crate::chr_network::qemu::nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let (nodes, holder, speakers, apis) = boot("chr-network-empty-networks").await;
    let near = &apis[0];
    let far: Ipv4Addr = "192.168.255.24".parse().unwrap();
    let near_node = &nodes[&speakers[0]];
    let near_access = holder.devices_by_id(speakers[0]).await.unwrap();
    let adjacent = || async { Ok(full_with(near, "default-v2", far).await) };
    wait_until(adjacent, Duration::from_secs(180))
        .await
        .expect("near and far adjacent in v2");
    assert!(template_updates(near_node, &near_access).await.is_empty());

    let id = collect_resource::<RoutingOspfInterfaceTemplate>(near)
        .await
        .expect("templates")
        .into_iter()
        .find(|template| *template.cfg.area.0 == *b"backbone-v2")
        .map(|template| template.status.id)
        .expect("the v2 template");
    set_keyed(
        near,
        &id,
        |template: &mut RoutingOspfInterfaceTemplateById| {
            template.data.networks = Some(BTreeSet::new())
        },
    )
    .await
    .expect("networks emptied");
    wait_until(
        || async { Ok(!full_with(near, "default-v2", far).await) },
        Duration::from_secs(60),
    )
    .await
    .expect("the adjacency gone");
    assert_eq!(
        template_updates(near_node, &near_access).await,
        ["!networks"],
        "the diff unsets the empty networks"
    );

    apply_directly(near_node, &near_access).await;
    wait_until(adjacent, Duration::from_secs(180))
        .await
        .expect("near and far adjacent in v2 again");
    assert!(
        template_updates(near_node, &near_access).await.is_empty(),
        "and the diff empty for it"
    );
}
