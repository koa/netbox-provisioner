//! Scope: CHR test of OSPF credentials moving segment by segment (A10); nothing else.
//!
//! `a -- b -- c`, each link a `/31` with `ospf`, applied without credentials first. Then the
//! devices are applied in pairs as a batch plans them: each generates and reads, the decision
//! (`ospf_auth::decide`) goes over the pair, both apply at once. Only the segment with both
//! ends in the pair moves; the adjacencies stay up and a ping from `a` to `c`'s loopback goes
//! through the whole time.

use crate::chr_network::default_route::{apply_directly, apply_generated};
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::wait_until;
use backend::config::OspfAuth;
use backend::device::access::Session;
use backend::device::ospf_auth::{
    Decision, MemberRead, decide, read_neighbors, target_credentials,
};
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::device::ros::ospf::OspfCredentials;
use backend::device::ros::warning::{OspfAuthDoubt, OspfEndsOutside};
use backend::topology::access::device::DeviceAccess;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, IpAddressData, PhysicalPortId, TopologyHolder,
};
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::{
    OspfNeighborState, RoutingOspfInterfaceTemplate, RoutingOspfNeighborState,
};
use mikrotik_model::resource::collect_resource;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::Duration;
use tokio::time::Instant;

const A_ID: &str = "192.168.255.41";
const B_ID: &str = "192.168.255.42";
const C_ID: &str = "192.168.255.43";
const AB: &str = "10.0.43.0/31";
const BC: &str = "10.0.43.2/31";
const AREAS: [&str; 2] = ["backbone-v2", "backbone-v3"];
const INSTANCES: [&str; 2] = ["default-v2", "default-v3"];

/// A topology and its three devices.
struct Chain {
    builder: TopologyBuilder,
    devices: [DeviceId; 3],
}

/// `a -- b -- c`: b has `ether2` to a and `ether3` to c.
fn chain() -> Chain {
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
    let mut devices = Vec::new();
    for (name, loopback) in [("a", A_ID), ("b", B_ID), ("c", C_ID)] {
        let id = builder.devices.next_id();
        let loopback = address(&mut builder, &format!("{loopback}/32"));
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                primary_ip_v4: Some(loopback),
                loopback_ip: Some(loopback),
                has_routeros: true,
                ..Default::default()
            },
        );
        devices.push(id);
    }
    let mut port = |device, ether: u16, ips: [&str; 2]| {
        let ips = ips.iter().map(|ip| address(&mut builder, ip)).collect();
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{ether}").into_boxed_str(),
                device,
                external: Some(PhysicalPortId::Ethernet(ether)),
                ips,
                use_ospf: true,
                ..Default::default()
            },
        );
        id
    };
    let links = [
        (
            port(devices[0], 2, ["10.0.43.0/31", "2001:db8:43::/127"]),
            port(devices[1], 2, ["10.0.43.1/31", "2001:db8:43::1/127"]),
        ),
        (
            port(devices[1], 3, ["10.0.43.2/31", "2001:db8:43::2/127"]),
            port(devices[2], 2, ["10.0.43.3/31", "2001:db8:43::3/127"]),
        ),
    ];
    for (a, b) in links {
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
    Chain {
        builder,
        devices: [devices[0], devices[1], devices[2]],
    }
}

/// A device of the scenario: its node, API session and NetBox device.
struct End<'a> {
    node: &'a RunningNode,
    api: Session,
    device: DeviceAccess,
}

/// What an end generates and reads for the decision.
struct Read {
    cfg: DeviceCfg,
    member: MemberRead,
}

/// What `end` generates and reads for the decision, as `AccessibleDevice::read_plan` does.
async fn read(end: &End<'_>) -> Read {
    let mut cfg = DeviceCfg::fetch_with_board_model(&end.api, b"RB750Gr3", &end.device)
        .await
        .expect("fetch");
    cfg.generate_from(&end.device, KeepNameGenerator)
        .expect("generate");
    let member = MemberRead {
        device: end.device.clone(),
        ports: cfg.ospf_ports(),
        neighbors: read_neighbors(&end.api).await.expect("neighbors"),
    };
    Read { cfg, member }
}

/// Applies `ends` together as a batch plans them: all read, the decision over all, then all
/// at once. Gives each one's decision, by name.
async fn apply_together(settings: &OspfAuth, ends: &[&End<'_>]) -> BTreeMap<Box<str>, Decision> {
    let mut read_all = Vec::new();
    for end in ends {
        read_all.push(read(end).await);
    }
    let mut decisions = decide(
        Some(settings),
        &read_all.iter().map(|read| &read.member).collect::<Vec<_>>(),
    );
    let mut by_name = BTreeMap::new();
    let mut applies = Vec::new();
    for (end, Read { mut cfg, member }) in ends.iter().zip(read_all) {
        let decision = decisions.remove(&member.device.id()).unwrap_or_default();
        cfg.take_ospf_decision(&decision);
        by_name.insert(Box::<str>::from(end.device.name()), decision);
        applies.push(apply_generated(end.node, &end.api, cfg));
    }
    async_graphql::futures_util::future::join_all(applies).await;
    by_name
}

/// A template on a device: its comment and credentials.
#[derive(Debug, PartialEq)]
struct Template {
    comment: Option<String>,
    credentials: OspfCredentials,
}

/// The comment and credentials of the template of `area` holding `port` on `api`.
async fn template_at(api: &Session, area: &str, port: &str) -> Template {
    let templates = collect_resource::<RoutingOspfInterfaceTemplate>(api)
        .await
        .expect("templates");
    let template = templates
        .iter()
        .find(|t| {
            *t.cfg.area.0 == *area.as_bytes()
                && t.cfg.interfaces.iter().any(|i| *i.0 == *port.as_bytes())
        })
        .unwrap_or_else(|| panic!("no template of {area} with {port}: {templates:?}"));
    Template {
        comment: template.cfg.comment.as_ref().map(ToString::to_string),
        credentials: OspfCredentials::of(&template.cfg),
    }
}

/// Asserts that no interface template of `end` is left without a port.
async fn no_empty_template(end: &End<'_>) {
    let templates = collect_resource::<RoutingOspfInterfaceTemplate>(&end.api)
        .await
        .expect("templates");
    assert!(
        templates.iter().all(|t| !t.cfg.interfaces.is_empty()),
        "{}: {templates:?}",
        end.device.name()
    );
}

/// Whether `api`'s device sees `router_id` `Full` in `instance`.
async fn full(api: &Session, instance: &str, router_id: &str) -> bool {
    let router_id: Ipv4Addr = router_id.parse().unwrap();
    collect_resource::<RoutingOspfNeighborState>(api)
        .await
        .expect("neighbors")
        .iter()
        .any(|n| {
            *n.instance.0 == *instance.as_bytes()
                && n.router_id == router_id
                && n.state == OspfNeighborState::Full
        })
}

/// What `during` gave, and what was down or lost while it ran.
struct Watched<T> {
    result: T,
    broken: Vec<String>,
}

/// While `during` runs and up to `watch`: b's four adjacencies (to a and c, v2 and v3) every
/// second and a ping from a to c's loopback every second. Gives what was down or lost, by
/// second.
async fn unbroken<T>(
    a: &Session,
    b: &Session,
    during: impl Future<Output = T>,
    watch: Duration,
) -> Watched<T> {
    let start = Instant::now();
    let adjacencies = async {
        let mut down = Vec::new();
        while start.elapsed() < watch {
            for instance in INSTANCES {
                for router_id in [A_ID, C_ID] {
                    if !full(b, instance, router_id).await {
                        let at = start.elapsed().as_secs();
                        down.push(format!("{at}s {instance} {router_id}"));
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        down
    };
    let pings = async {
        let c: IpAddr = C_ID.parse().unwrap();
        let mut lost = Vec::new();
        while start.elapsed() < watch {
            if !crate::chr_network::node::answers_ping(a, c, 1).await {
                lost.push(format!("{}s ping", start.elapsed().as_secs()));
            }
        }
        lost
    };
    let (result, down, lost) = tokio::join!(during, adjacencies, pings);
    Watched {
        result,
        broken: down.into_iter().chain(lost).collect(),
    }
}

/// The networks a decision moves.
fn moved(decision: &Decision) -> Vec<String> {
    decision.switch.keys().map(ToString::to_string).collect()
}

#[tokio::test]
#[ignore]
async fn ospf_credentials_move_with_both_ends_of_a_segment() {
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
        devices: [a, b, c],
    } = chain();
    let boards = HashMap::from([
        (a, DeviceType::RB750Gr3),
        (b, DeviceType::RB750Gr3),
        (c, DeviceType::RB750Gr3),
    ]);
    let work_dir = Path::new("/tmp").join("chr-network-ospf-auth");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    let mut ends = Vec::new();
    for id in [a, b, c] {
        let device = holder.devices_by_id(id).await.unwrap();
        apply_directly(&nodes[&id], &device).await;
        ends.push(End {
            node: &nodes[&id],
            api: nodes[&id]
                .connect_api(crate::TEST_PASSWORD)
                .await
                .expect("api"),
            device,
        });
    }
    let [a, b, c] = [&ends[0], &ends[1], &ends[2]];
    for instance in INSTANCES {
        for (end, router_id) in [(a, B_ID), (b, A_ID), (b, C_ID), (c, B_ID)] {
            wait_until(
                || async { Ok(full(&end.api, instance, router_id).await) },
                Duration::from_secs(120),
            )
            .await
            .unwrap_or_else(|_| {
                panic!("{}: {router_id} not full in {instance}", end.device.name())
            });
        }
    }
    // the routes to the loopbacks come a little after Full
    let c_loopback: IpAddr = C_ID.parse().unwrap();
    wait_until(
        || async { Ok(crate::chr_network::node::answers_ping(&a.api, c_loopback, 1).await) },
        Duration::from_secs(60),
    )
    .await
    .expect("a reaches c");
    let settings = OspfAuth::new(1, "chr-test-secret");
    let target = |network: &str| target_credentials(&settings, network.parse().unwrap());

    // a and b together: a-b moves, b-c stays, c being outside
    let Watched {
        result: decisions,
        broken,
    } = unbroken(
        &a.api,
        &b.api,
        apply_together(&settings, &[a, b]),
        Duration::from_secs(60),
    )
    .await;
    assert!(broken.is_empty(), "a and b: {broken:?}");
    assert_eq!(moved(&decisions["a"]), [AB]);
    assert!(decisions["a"].kept.is_empty(), "{:?}", decisions["a"]);
    assert_eq!(moved(&decisions["b"]), [AB]);
    assert_eq!(decisions["b"].kept.len(), 1, "{:?}", decisions["b"]);
    assert_eq!(&*decisions["b"].kept[0].port, "ether3");
    assert_eq!(
        decisions["b"].kept[0].doubt,
        OspfAuthDoubt::EndsOutside(OspfEndsOutside {
            devices: ["c".into()].into()
        })
    );
    let own = |network: &str| Some(format!("netbox-provisioner: ospf {network}"));
    for area in AREAS {
        for (end, port) in [(a, "ether2"), (b, "ether2")] {
            assert_eq!(
                template_at(&end.api, area, port).await,
                Template {
                    comment: own(AB),
                    credentials: target(AB),
                },
                "{} {area}",
                end.device.name()
            );
        }
        assert_eq!(
            template_at(&b.api, area, "ether3").await,
            Template {
                comment: None,
                credentials: OspfCredentials::default(),
            },
            "b {area}"
        );
    }

    no_empty_template(a).await;
    no_empty_template(b).await;

    // b alone now: a-b is at the provisioner's credentials, b-c stays; nothing in the diff
    let Read { mut cfg, member } = read(b).await;
    let alone = decide(Some(&settings), &[&member])
        .remove(&b.device.id())
        .unwrap_or_default();
    assert!(alone.switch.is_empty(), "{alone:?}");
    assert_eq!(alone.kept.len(), 1, "{alone:?}");
    cfg.take_ospf_decision(&alone);
    // as `apply_generated`
    cfg.setup_services(None);
    let templates = cfg
        .generate_mutations()
        .expect("mutations")
        .iter()
        .filter(|m| *m.resource == *b"routing/ospf/interface-template" && !m.fields.is_empty())
        .map(|m| format!("{:?} {:?}", m.operation, m.fields))
        .collect::<Vec<_>>();
    assert!(templates.is_empty(), "b alone: {templates:?}");

    // b and c together: b-c moves, a-b keeps its credentials
    let Watched {
        result: decisions,
        broken,
    } = unbroken(
        &a.api,
        &b.api,
        apply_together(&settings, &[b, c]),
        Duration::from_secs(60),
    )
    .await;
    assert!(broken.is_empty(), "b and c: {broken:?}");
    assert_eq!(moved(&decisions["b"]), [BC]);
    assert!(decisions["b"].kept.is_empty(), "{:?}", decisions["b"]);
    for area in AREAS {
        for (end, port, network) in [
            (a, "ether2", AB),
            (b, "ether2", AB),
            (b, "ether3", BC),
            (c, "ether2", BC),
        ] {
            assert_eq!(
                template_at(&end.api, area, port).await,
                Template {
                    comment: own(network),
                    credentials: target(network),
                },
                "{} {port} {area}",
                end.device.name()
            );
        }
    }

    for end in [a, b, c] {
        no_empty_template(end).await;
    }

    // a new version: a alone leaves a-b as it is, a and b together move it
    let next = OspfAuth::new(2, "chr-test-secret-2");
    let Watched {
        result: decisions,
        broken,
    } = unbroken(
        &a.api,
        &b.api,
        apply_together(&next, &[a]),
        Duration::from_secs(10),
    )
    .await;
    assert!(broken.is_empty(), "a alone: {broken:?}");
    assert!(decisions["a"].switch.is_empty(), "{:?}", decisions["a"]);
    assert_eq!(
        template_at(&a.api, AREAS[0], "ether2").await.credentials,
        target(AB)
    );
    let Watched {
        result: decisions,
        broken,
    } = unbroken(
        &a.api,
        &b.api,
        apply_together(&next, &[a, b]),
        Duration::from_secs(60),
    )
    .await;
    assert!(broken.is_empty(), "a and b, version 2: {broken:?}");
    assert_eq!(moved(&decisions["a"]), [AB]);
    let version_2 = target_credentials(&next, AB.parse().unwrap());
    assert_eq!(version_2.id, Some(2));
    for end in [a, b] {
        for area in AREAS {
            assert_eq!(
                template_at(&end.api, area, "ether2").await.credentials,
                version_2
            );
        }
    }
    // b-c stays at version 1: b has a segment of each version
    assert_eq!(
        template_at(&b.api, AREAS[0], "ether3").await.credentials,
        target(BC)
    );
}
