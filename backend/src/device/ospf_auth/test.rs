//! Scope: Tests of `device::ospf_auth`; nothing else.
use super::{MemberRead, decide, segment_key, target_credentials};
use crate::config::OspfAuth;
use crate::device::ros::ospf::{OspfCredentials, OspfPort};
use crate::device::ros::warning::{
    OspfAuthDoubt, OspfCredentialsDiffer, OspfEndNotOspf, OspfEndsOutside, OspfForeignNeighbor,
    OspfNeighborMissing,
};
use crate::topology::builder::TopologyBuilder;
use crate::topology::{Device, DeviceId, Interface, IpAddressData, IpAddressId, Topology};
use ipnet::Ipv4Net;
use mikrotik_model::model::{OspfNeighborState, RoutingOspfNeighborState};
use mikrotik_model::value::Id;
use std::net::Ipv4Addr;
use std::sync::Arc;

const A_ID: &str = "192.168.255.1";
const B_ID: &str = "192.168.255.2";

fn settings() -> OspfAuth {
    OspfAuth::new(1, "secret")
}

fn link() -> Ipv4Net {
    "10.0.1.0/31".parse().unwrap()
}

struct Net {
    topology: Arc<Topology>,
    a: DeviceId,
    b: DeviceId,
}

fn address(builder: &mut TopologyBuilder, ip: &str) -> IpAddressId {
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
}

fn device(builder: &mut TopologyBuilder, name: &str, loopback: Option<&str>) -> DeviceId {
    let id = builder.devices.next_id();
    let loopback = loopback.map(|ip| address(builder, &format!("{ip}/32")));
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            primary_ip_v4: loopback,
            loopback_ip: loopback,
            has_routeros: loopback.is_some(),
            ..Default::default()
        },
    );
    id
}

fn port(builder: &mut TopologyBuilder, device: DeviceId, ip: &str, ospf: bool) {
    let ip = address(builder, ip);
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: "ether2".into(),
            device,
            ips: [ip].into(),
            use_ospf: ospf,
            ..Default::default()
        },
    );
}

/// `a -- b` over `10.0.1.0/31` with `ospf`; `b` is left out of NetBox's link with `lonely`,
/// and a client without RouterOS has an address in it with `client`.
fn build(lonely: bool, client: bool) -> Net {
    let mut builder = TopologyBuilder::default();
    let a = device(&mut builder, "a", Some(A_ID));
    let b = device(&mut builder, "b", Some(B_ID));
    port(&mut builder, a, "10.0.1.0/31", true);
    if !lonely {
        port(&mut builder, b, "10.0.1.1/31", true);
    }
    if client {
        let client = device(&mut builder, "client", None);
        port(&mut builder, client, "10.0.1.1/31", false);
    }
    Net {
        topology: Arc::new(builder.build()),
        a,
        b,
    }
}

fn neighbor(instance: &str, router_id: &str, state: OspfNeighborState) -> RoutingOspfNeighborState {
    RoutingOspfNeighborState {
        id: Id(1),
        instance: instance.into(),
        area: "backbone".into(),
        interface: Some("e02".into()),
        address: "10.0.1.1".parse().unwrap(),
        priority: None,
        router_id: router_id.parse().unwrap(),
        dr: None,
        bdr: None,
        state,
        state_changes: None,
        adjacency: None,
        timeout: None,
        dynamic: None,
        _virtual: None,
        comment: None,
        ls_requests: None,
        ls_retransmits: None,
        db_summaries: None,
        about: None,
    }
}

/// Full in v2 and v3 with `router_id`.
fn full(router_id: &str) -> Vec<RoutingOspfNeighborState> {
    ["default-v2", "default-v3"]
        .map(|instance| neighbor(instance, router_id, OspfNeighborState::Full))
        .into()
}

fn member(
    net: &Net,
    device: DeviceId,
    credentials: OspfCredentials,
    neighbors: Vec<RoutingOspfNeighborState>,
) -> MemberRead {
    MemberRead {
        device: net.topology.get_device_by_id(&device).unwrap(),
        ports: [OspfPort {
            port: "e02".into(),
            network: link(),
            v2: credentials.clone(),
            v3: credentials,
        }]
        .into(),
        neighbors: neighbors.into(),
    }
}

/// The doubt the decision has for `device`'s one segment, none if it switches.
fn doubt(
    decisions: &std::collections::BTreeMap<DeviceId, super::Decision>,
    device: DeviceId,
) -> Option<OspfAuthDoubt> {
    let decision = &decisions[&device];
    assert_eq!(
        decision.kept.len() + decision.switch.len(),
        1,
        "{decision:?}"
    );
    decision.kept.first().map(|kept| kept.doubt.clone())
}

/// Both ends in the apply, each seeing the other: the segment goes to the provisioner's
/// credentials on both, and each expects the other back.
#[test]
fn both_ends_seeing_each_other_switch() {
    let net = build(false, false);
    let members = [
        member(&net, net.a, OspfCredentials::default(), full(B_ID)),
        member(&net, net.b, OspfCredentials::default(), full(A_ID)),
    ];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    let target = target_credentials(&settings(), link());
    for (device, other) in [(net.a, B_ID), (net.b, A_ID)] {
        let decision = &decisions[&device];
        assert_eq!(decision.switch.get(&link()), Some(&target));
        assert!(decision.kept.is_empty());
        assert_eq!(decision.expected.len(), 1);
        assert_eq!(
            decision.expected[0].router_ids.iter().collect::<Vec<_>>(),
            [&other.parse::<Ipv4Addr>().unwrap()]
        );
    }
}

/// Without the setting nothing moves and nothing is said.
#[test]
fn without_the_setting_nothing_moves() {
    let net = build(false, false);
    let members = [member(&net, net.a, OspfCredentials::default(), full(B_ID))];
    let decisions = decide(None, &members.iter().collect::<Vec<_>>());
    assert_eq!(decisions[&net.a], super::Decision::default());
}

/// Alone, the device leaves the segment: its other end isn't in the apply.
#[test]
fn an_end_outside_the_apply_keeps_the_segment() {
    let net = build(false, false);
    let members = [member(&net, net.a, OspfCredentials::default(), full(B_ID))];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    assert_eq!(
        doubt(&decisions, net.a),
        Some(OspfAuthDoubt::EndsOutside(OspfEndsOutside {
            devices: ["b".into()].into()
        }))
    );
}

/// A neighbor NetBox doesn't have at the segment is doubt, even with every end in the apply.
#[test]
fn a_foreign_neighbor_keeps_the_segment() {
    let net = build(false, false);
    let mut seen_by_a = full(B_ID);
    seen_by_a.push(neighbor(
        "default-v2",
        "192.168.255.9",
        OspfNeighborState::Init,
    ));
    let members = [
        member(&net, net.a, OspfCredentials::default(), seen_by_a),
        member(&net, net.b, OspfCredentials::default(), full(A_ID)),
    ];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    let foreign = Some(OspfAuthDoubt::ForeignNeighbor(OspfForeignNeighbor {
        device: "a".into(),
        router_id: "192.168.255.9".into(),
    }));
    assert_eq!(doubt(&decisions, net.a), foreign);
    assert_eq!(doubt(&decisions, net.b), foreign);
}

/// An end not seen in v3, or not past `ExStart` in v2, may be down: the segment stays.
#[test]
fn a_missing_neighbor_keeps_the_segment() {
    let net = build(false, false);
    for seen_by_a in [
        vec![neighbor("default-v2", B_ID, OspfNeighborState::Full)],
        vec![
            neighbor("default-v2", B_ID, OspfNeighborState::ExStart),
            neighbor("default-v3", B_ID, OspfNeighborState::Full),
        ],
    ] {
        let members = [
            member(&net, net.a, OspfCredentials::default(), seen_by_a),
            member(&net, net.b, OspfCredentials::default(), full(A_ID)),
        ];
        let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
        assert_eq!(
            doubt(&decisions, net.a),
            Some(OspfAuthDoubt::NeighborMissing(OspfNeighborMissing {
                device: "a".into(),
                missing: "b".into(),
            }))
        );
    }
}

/// Ends with different credentials aren't in line with each other: the segment stays.
#[test]
fn ends_with_different_credentials_keep_the_segment() {
    let net = build(false, false);
    let other = OspfCredentials {
        id: Some(9),
        key: Some("other".into()),
        ..target_credentials(&settings(), link())
    };
    let members = [
        member(&net, net.a, OspfCredentials::default(), full(B_ID)),
        member(&net, net.b, other, full(A_ID)),
    ];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    assert_eq!(
        doubt(&decisions, net.a),
        Some(OspfAuthDoubt::CredentialsDiffer(OspfCredentialsDiffer {
            devices: ["a".into(), "b".into()].into()
        }))
    );
}

/// An address in the segment on a device without RouterOS is no end the provisioner sets up.
#[test]
fn an_address_on_another_device_keeps_the_segment() {
    let net = build(true, true);
    let members = [member(&net, net.a, OspfCredentials::default(), Vec::new())];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    assert_eq!(
        doubt(&decisions, net.a),
        Some(OspfAuthDoubt::EndNotOspf(OspfEndNotOspf {
            address: "10.0.1.1/31".into(),
            device: Some("client".into()),
        }))
    );
}

/// A segment only the device has in NetBox, without a neighbor there (a resident's port
/// waiting for his device), switches with the device alone.
#[test]
fn a_segment_of_one_end_switches_alone() {
    let net = build(true, false);
    let members = [member(&net, net.a, OspfCredentials::default(), Vec::new())];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    assert_eq!(doubt(&decisions, net.a), None);
    assert!(decisions[&net.a].expected[0].router_ids.is_empty());
}

/// A segment at the provisioner's credentials needs nothing, whoever is in the apply.
#[test]
fn a_segment_at_the_target_needs_nothing() {
    let net = build(false, false);
    let target = target_credentials(&settings(), link());
    let members = [member(&net, net.a, target, full(B_ID))];
    let decisions = decide(Some(&settings()), &members.iter().collect::<Vec<_>>());
    assert_eq!(decisions[&net.a], super::Decision::default());
}

/// The key is the segment's own: 64 hex characters, another for another network or secret.
#[test]
fn every_segment_has_a_key_of_its_own() {
    let key = segment_key("secret", link());
    assert_eq!(key.len(), 64);
    assert!(key.0.iter().all(u8::is_ascii_hexdigit));
    assert_eq!(key, segment_key("secret", link()));
    assert_ne!(key, segment_key("secret", "10.0.1.2/31".parse().unwrap()));
    assert_ne!(key, segment_key("other", link()));
}
