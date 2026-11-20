//! Scope: Tests of where the generator puts each OSPF port's credentials
//! (`BaseDeviceDataTarget::place_ospf_credentials`, A10); not which segments switch
//! (`device::ospf_auth`).
use super::empty_current;
use crate::device::ros::BaseDeviceDataTarget;
use crate::device::ros::ospf::{
    OspfCredentials, OspfSwitch, own_template_comment, templates_added_first,
};
use ipnet::Ipv4Net;
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        RoutingOspfInterfaceTemplateAuth, RoutingOspfInterfaceTemplateById,
        RoutingOspfInterfaceTemplateCfg,
    },
    resource::ResourceMutationOperation,
};
use std::collections::BTreeMap;

const AREAS: [&str; 2] = ["backbone-v2", "backbone-v3"];

fn net(network: &str) -> Ipv4Net {
    network.parse().unwrap()
}

fn credentials(id: u8, key: &str) -> OspfCredentials {
    OspfCredentials {
        auth: Some(RoutingOspfInterfaceTemplateAuth::Sha256),
        id: Some(id),
        key: Some(key.into()),
    }
}

fn template(area: &str, ports: &[&str]) -> RoutingOspfInterfaceTemplateCfg {
    RoutingOspfInterfaceTemplateCfg {
        area: area.into(),
        interfaces: ports.iter().map(|&p| AsciiString::from(p)).collect(),
        use_bfd: Some(false),
        ..Default::default()
    }
}

/// A target with the shared templates `setup_ospf` gives `ether2` and `ether3`, and the
/// segment of each.
fn target() -> (BaseDeviceDataTarget, BTreeMap<AsciiString, Ipv4Net>) {
    let mut target = BaseDeviceDataTarget::new(b"RB750Gr3").unwrap();
    for area in AREAS {
        target
            .ospf_interface
            .insert((area.into(), None), template(area, &["ether2", "ether3"]));
    }
    let segments = [
        (AsciiString::from("ether2"), net("10.0.1.0/31")),
        (AsciiString::from("ether3"), net("10.0.1.2/31")),
    ]
    .into();
    (target, segments)
}

fn current_templates(
    templates: Vec<RoutingOspfInterfaceTemplateCfg>,
) -> Box<[RoutingOspfInterfaceTemplateById]> {
    templates
        .into_iter()
        .enumerate()
        .map(|(n, data)| RoutingOspfInterfaceTemplateById {
            id: format!("*{n}").as_str().into(),
            data,
        })
        .collect()
}

fn own(
    area: &str,
    port: &str,
    network: &str,
    with: &OspfCredentials,
) -> RoutingOspfInterfaceTemplateCfg {
    let mut template = template(area, &[port]);
    template.comment = Some(own_template_comment(net(network)));
    template.auth = with.auth;
    template.auth_id = with.id;
    template.auth_key = with.key.clone();
    template
}

/// The ports and credentials of the target's templates of `area`, by comment.
fn placed(
    target: &BaseDeviceDataTarget,
    area: &str,
) -> Vec<(Option<String>, Vec<String>, OspfCredentials)> {
    target
        .ospf_interface
        .iter()
        .filter(|((a, _), _)| *a.0 == *area.as_bytes())
        .map(|((_, comment), template)| {
            (
                comment.as_ref().map(ToString::to_string),
                template
                    .interfaces
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                OspfCredentials::of(template),
            )
        })
        .collect()
}

/// Without credentials on the device and nothing to switch, the shared templates stay as
/// `setup_ospf` made them, and the diff changes no field of them.
#[test]
fn nothing_to_switch_changes_nothing() {
    let (mut target, segments) = target();
    let mut current = empty_current(&target);
    current.ospf_interface = current_templates(
        AREAS
            .map(|area| template(area, &["ether2", "ether3"]))
            .into(),
    );
    target.place_ospf_credentials(&current, &segments, &OspfSwitch::new());
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [(
                None,
                vec!["ether2".into(), "ether3".into()],
                OspfCredentials::default()
            )]
        );
    }
    let changes = target.generate_mutations(&current).unwrap();
    assert!(
        !changes
            .iter()
            .any(|m| *m.resource == *b"routing/ospf/interface-template" && !m.fields.is_empty()),
        "{changes:?}"
    );
}

/// A switched segment's port gets a template of its own with the credentials, in v2 and v3;
/// the other port stays in the shared one.
#[test]
fn a_switched_segment_gets_its_own_template() {
    let (mut target, segments) = target();
    let current = empty_current(&target);
    let key = credentials(1, "k1");
    let switch = OspfSwitch::from([(net("10.0.1.2/31"), key.clone())]);
    target.place_ospf_credentials(&current, &segments, &switch);
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [
                (None, vec!["ether2".into()], OspfCredentials::default()),
                (
                    Some("netbox-provisioner: ospf 10.0.1.2/31".into()),
                    vec!["ether3".into()],
                    key.clone()
                ),
            ],
            "{area}"
        );
    }
}

/// Not switched, a segment keeps the credentials of its own template on the device, also
/// where its port has another name there now; its template follows the port.
#[test]
fn a_segment_not_switched_keeps_its_credentials() {
    let (mut target, segments) = target();
    let mut current = empty_current(&target);
    let old = credentials(1, "old");
    current.ospf_interface = current_templates(
        AREAS
            .iter()
            .flat_map(|&area| {
                [
                    template(area, &["ether2"]),
                    own(area, "e03-old-name", "10.0.1.2/31", &old),
                ]
            })
            .collect(),
    );
    target.place_ospf_credentials(&current, &segments, &OspfSwitch::new());
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [
                (None, vec!["ether2".into()], OspfCredentials::default()),
                (
                    Some("netbox-provisioner: ospf 10.0.1.2/31".into()),
                    vec!["ether3".into()],
                    old.clone()
                ),
            ],
            "{area}"
        );
    }
}

/// A segment switched to no credentials goes back into the shared template; its own one is
/// no target anymore, so the diff removes it.
#[test]
fn a_segment_switched_off_goes_back_into_the_shared_template() {
    let (mut target, segments) = target();
    let mut current = empty_current(&target);
    current.ospf_interface = current_templates(
        AREAS
            .iter()
            .flat_map(|&area| {
                [
                    template(area, &["ether2"]),
                    own(area, "ether3", "10.0.1.2/31", &credentials(1, "old")),
                ]
            })
            .collect(),
    );
    let switch = OspfSwitch::from([(net("10.0.1.2/31"), OspfCredentials::default())]);
    target.place_ospf_credentials(&current, &segments, &switch);
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [(
                None,
                vec!["ether2".into(), "ether3".into()],
                OspfCredentials::default()
            )]
        );
    }
}

/// Credentials set by hand on the shared template stay there, and a port not switched stays
/// in it with them.
#[test]
fn the_shared_template_keeps_its_credentials() {
    let (mut target, segments) = target();
    let mut current = empty_current(&target);
    let by_hand = OspfCredentials {
        auth: Some(RoutingOspfInterfaceTemplateAuth::Md5),
        id: Some(7),
        key: Some("by-hand".into()),
    };
    current.ospf_interface = current_templates(
        AREAS
            .iter()
            .map(|&area| {
                let mut shared = template(area, &["ether2", "ether3"]);
                shared.auth = by_hand.auth;
                shared.auth_id = by_hand.id;
                shared.auth_key = by_hand.key.clone();
                shared
            })
            .collect(),
    );
    target.place_ospf_credentials(&current, &segments, &OspfSwitch::new());
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [(
                None,
                vec!["ether2".into(), "ether3".into()],
                by_hand.clone()
            )]
        );
    }
}

/// Placed again with a decision after the generate, the result is the decision's alone.
#[test]
fn placing_again_follows_the_last_decision() {
    let (mut target, segments) = target();
    let current = empty_current(&target);
    let first = OspfSwitch::from([(net("10.0.1.2/31"), credentials(1, "k1"))]);
    target.place_ospf_credentials(&current, &segments, &first);
    let second = OspfSwitch::from([(net("10.0.1.0/31"), credentials(2, "k2"))]);
    target.place_ospf_credentials(&current, &segments, &second);
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [
                (None, vec!["ether3".into()], OspfCredentials::default()),
                (
                    Some("netbox-provisioner: ospf 10.0.1.0/31".into()),
                    vec!["ether2".into()],
                    credentials(2, "k2")
                ),
            ],
            "{area}"
        );
    }
}

/// A port moving into a template of its own is added there before it leaves the shared one,
/// and goes back into the shared one before its own goes.
#[test]
fn a_port_is_in_a_template_all_the_time() {
    let order = |switch: &OspfSwitch, current: &[RoutingOspfInterfaceTemplateCfg]| {
        let (mut target, segments) = target();
        let mut now = empty_current(&target);
        now.ospf_interface = current_templates(current.to_vec());
        target.place_ospf_credentials(&now, &segments, switch);
        let cfg_mutations = target.generate_mutations(&now).unwrap();
        let templates = cfg_mutations
            .into_iter()
            .filter(|m| *m.resource == *b"routing/ospf/interface-template")
            .filter(|m| {
                !m.fields.is_empty()
                    || !matches!(m.operation, ResourceMutationOperation::UpdateByKey(_))
            })
            // as the sort may give them: changes first
            .rev()
            .collect::<Vec<_>>();
        templates_added_first(templates)
            .iter()
            .map(|m| match m.operation {
                ResourceMutationOperation::Add => "add",
                ResourceMutationOperation::RemoveByKey(_) => "remove",
                _ => "set",
            })
            .collect::<Vec<_>>()
    };
    let shared = AREAS.map(|area| template(area, &["ether2", "ether3"]));
    let moved = OspfSwitch::from([(net("10.0.1.2/31"), credentials(1, "k1"))]);
    assert_eq!(order(&moved, &shared), ["add", "add", "set", "set"]);
    let own = AREAS
        .iter()
        .flat_map(|&area| {
            [
                template(area, &["ether2"]),
                own(area, "ether3", "10.0.1.2/31", &credentials(1, "k1")),
            ]
        })
        .collect::<Vec<_>>();
    let back = OspfSwitch::from([(net("10.0.1.2/31"), OspfCredentials::default())]);
    let back_order = order(&back, &own);
    let set = back_order.iter().rposition(|m| *m == "set").unwrap();
    let remove = back_order.iter().position(|m| *m == "remove").unwrap();
    assert!(set < remove, "{back_order:?}");
}

/// With every port in a template of its own, the shared one goes, and comes back once a port
/// is in it again.
#[test]
fn a_shared_template_without_ports_goes() {
    let (mut target, segments) = target();
    let current = empty_current(&target);
    let all = OspfSwitch::from([
        (net("10.0.1.0/31"), credentials(1, "k1")),
        (net("10.0.1.2/31"), credentials(1, "k2")),
    ]);
    target.place_ospf_credentials(&current, &segments, &all);
    for area in AREAS {
        let comments = placed(&target, area)
            .into_iter()
            .map(|(comment, _, _)| comment)
            .collect::<Vec<_>>();
        assert_eq!(
            comments,
            [
                Some("netbox-provisioner: ospf 10.0.1.0/31".into()),
                Some("netbox-provisioner: ospf 10.0.1.2/31".into())
            ],
            "{area}"
        );
    }
    let one = OspfSwitch::from([(net("10.0.1.2/31"), credentials(1, "k2"))]);
    target.place_ospf_credentials(&current, &segments, &one);
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [
                (None, vec!["ether2".into()], OspfCredentials::default()),
                (
                    Some("netbox-provisioner: ospf 10.0.1.2/31".into()),
                    vec!["ether3".into()],
                    credentials(1, "k2")
                ),
            ],
            "{area}"
        );
    }
}

/// A link renumbered in NetBox (a resident moving its device, „Kabel übernehmen“) keeps the
/// credentials its port has on the device, in its old segment's template: in doubt they stay.
#[test]
fn a_renumbered_port_keeps_its_credentials() {
    let (mut target, segments) = target();
    let mut current = empty_current(&target);
    let old = credentials(1, "old");
    current.ospf_interface = current_templates(
        AREAS
            .iter()
            .flat_map(|&area| {
                [
                    template(area, &["ether2"]),
                    own(area, "ether3", "10.0.9.0/30", &old),
                ]
            })
            .collect(),
    );
    target.place_ospf_credentials(&current, &segments, &OspfSwitch::new());
    for area in AREAS {
        assert_eq!(
            placed(&target, area),
            [
                (None, vec!["ether2".into()], OspfCredentials::default()),
                (
                    Some("netbox-provisioner: ospf 10.0.1.2/31".into()),
                    vec!["ether3".into()],
                    old.clone()
                ),
            ],
            "{area}"
        );
    }
}
