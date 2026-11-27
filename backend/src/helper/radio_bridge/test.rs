//! Scope: Tests of `radio_bridge`; nothing else.
use super::*;
use crate::topology::test::example_topology;

fn find(topology: &Arc<Topology>, name: &str) -> DeviceAccess {
    topology
        .list_devices()
        .find(|d| d.name() == name)
        .unwrap_or_else(|| panic!("{name}"))
}

/// The export with `change` made to each of its interfaces and without the IP addresses
/// `removed`: the state a test plans from, since NetBox has the links whole by now.
fn changed(
    topology: &Arc<Topology>,
    removed: &[&str],
    change: impl Fn(&mut serde_json::Value),
) -> Arc<Topology> {
    Arc::new(
        topology
            .rebuilt_with(0, |json| {
                if let Some(addresses) = json["ip_address_list"].as_array_mut() {
                    addresses.retain(|address| !removed.iter().any(|id| address["id"] == *id));
                }
                for device in json["device_list"].as_array_mut().into_iter().flatten() {
                    for interface in device["interfaces"].as_array_mut().into_iter().flatten() {
                        if let Some(addresses) = interface["ip_addresses"].as_array_mut() {
                            addresses
                                .retain(|address| !removed.iter().any(|id| address["id"] == *id));
                        }
                        change(interface);
                    }
                }
            })
            .expect("the export's source")
            .expect("rebuilt"),
    )
}

fn input(ap: &DeviceAccess, station: &DeviceAccess) -> RadioBridgeInput {
    RadioBridgeInput {
        ap: ap.id().0,
        station: station.id().0,
        ap_router_port: None,
        station_router_port: None,
        ap_address: None,
        station_address: None,
        ssid: None,
    }
}

/// schopf-burri – burri-schopf as in an export before the helper: cables, bridges,
/// radios, link and loopbacks are there, the bridges lack their addresses in
/// router-schopf's network and `ospf`, berghof-burri an IPv4 in it, the bridges' cables
/// their labels.
#[tokio::test]
async fn schopf_burri_gets_its_addresses() {
    // berghof-burri ether1's 172.17.0.42/29 and the bridges' .43 and .44
    let topology = changed(&example_topology(), &["554", "555", "556"], |interface| {
        match interface["id"].as_str() {
            // the bridges of schopf-burri and burri-schopf
            Some("304" | "342") => interface["tags"] = serde_json::json!([]),
            // the bridges' ether1
            Some("305" | "343") => interface["label"] = serde_json::json!(""),
            _ => {}
        }
    });
    let (ap, station) = (
        find(&topology, "schopf-burri"),
        find(&topology, "burri-schopf"),
    );
    let mut routes = Some(Routes::none("test"));
    let plan = plan_with(&topology, &input(&ap, &station), &mut routes)
        .await
        .expect("plan");
    let addresses = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::CreateAddress(create) => {
                Some((create.device.name.to_string(), create.address.to_string()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(addresses.len(), 3, "{:#?}", plan.steps);
    for (device, address) in &addresses {
        let address = address.parse::<IpNet>().expect("an address");
        assert_eq!(address.trunc().to_string(), "172.17.0.40/29", "{device}");
        assert_ne!(address.addr().to_string(), "172.17.0.41", "{device}");
    }
    let devices = addresses
        .iter()
        .map(|(d, _)| d.as_str())
        .collect::<Vec<_>>();
    assert_eq!(devices, ["berghof-burri", "schopf-burri", "burri-schopf"]);
    let tagged = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdateInterface(update) if *update.tags_added == [InterfaceTag::Ospf] => {
                Some(update.interface.device.name.to_string())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tagged,
        ["schopf-burri", "burri-schopf"],
        "the routers have ospf"
    );
    // each cable end names the device at the other one, as „Verbindung“ labels them
    let labels = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdateInterface(update) => Some((
                format!("{} {}", update.interface.device.name, update.interface.name),
                update.label.as_deref()?.to_string(),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&("schopf-burri ether1".into(), "router-schopf".into())),
        "{labels:?}"
    );
    assert!(
        labels.contains(&("burri-schopf ether1".into(), "berghof-burri".into())),
        "{labels:?}"
    );
    let after = plan.preview_topology().expect("preview");
    assert!(
        !after.findings().iter().any(|f| matches!(&f.kind,
                crate::topology::findings::FindingKind::LabelNotFarEnd(l)
                    if [&*l.interface.device.name, &*l.far_end.device.name]
                        .iter()
                        .any(|name| ["schopf-burri", "burri-schopf"].contains(name)))),
        "{:#?}",
        after.findings()
    );
    assert!(
        !plan.steps.iter().any(|step| matches!(
            step,
            PlanStep::CreateCable(_)
                | PlanStep::CreateInterface(_)
                | PlanStep::CreateWirelessLink(_)
        )),
        "{:#?}",
        plan.steps
    );
}

/// A 60 GHz radio named `wlan1` becomes `wlan60-1`, another radio `wlan1`.
#[test]
fn radios_get_their_routeros_name() {
    use crate::topology::{Device, Interface, builder::TopologyBuilder};
    use std::str::FromStr;
    let mut builder = TopologyBuilder::default();
    let device = builder.devices.next_id();
    builder.devices.insert(device, Device::default());
    let mut ids = Vec::new();
    for (name, sixty_ghz) in [
        ("wlan1", true),
        ("wlan60-1", true),
        ("radio", false),
        ("wlan1", false),
    ] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external: PhysicalPortId::from_str(name).ok(),
                port_type: Some(PortType::Wireless),
                sixty_ghz,
                ..Interface::default()
            },
        );
        ids.push(id);
    }
    let topology = Arc::new(builder.build());
    let names = ids
        .into_iter()
        .map(|id| routeros_name(&topology.get_interface_by_id(id).unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(names, [Some("wlan60-1"), None, Some("wlan1"), None]);
}

/// Radios without RF role get theirs, the link without SSID and PSK both, and the station
/// is cut off until both ends are applied.
#[test]
fn a_radio_without_role_is_corrected() {
    // the link as it was before the helper: no roles, no SSID, open
    let topology = changed(&example_topology(), &[], |interface| {
        if matches!(interface["id"].as_str(), Some("293" | "296")) {
            interface["rf_role"] = serde_json::json!("");
            let link = &mut interface["wireless_link"];
            link["ssid"] = serde_json::json!("");
            link["auth_type"] = serde_json::Value::Null;
            link["auth_psk"] = serde_json::json!("");
        }
    });
    let (ap, station) = (
        find(&topology, "hegglin-schopf"),
        find(&topology, "schopf-hegglin"),
    );
    // without loopbacks and segment: the export's devices lack `lo`, a new one asks NetBox
    let LinkPlan { plan, .. } = link_plan(&topology, &input(&ap, &station)).expect("plan");
    let ports = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdatePort(update) => Some(update),
            _ => None,
        })
        .filter(|update| update.rf_role.is_some())
        .collect::<Vec<_>>();
    assert_eq!(ports.len(), 2, "{:#?}", plan.steps);
    assert!(
        ports.iter().all(|update| update.name.is_none()),
        "named as RouterOS"
    );
    let link = plan.steps.iter().find_map(|step| match step {
        PlanStep::UpdateWirelessLink(update) => Some(update),
        _ => None,
    });
    let link = link.expect("the link gets SSID and PSK");
    assert_eq!(link.ssid.as_deref(), Some("hegglin-schopf"));
    assert!(link.psk_set);
    assert_eq!(
        plan.apply_together
            .iter()
            .map(|d| d.name.to_string())
            .collect::<Vec<_>>(),
        ["hegglin-schopf", "schopf-hegglin"]
    );
    assert!(plan.cut_off.is_empty());
}

/// The same device on both ends is refused.
#[tokio::test]
async fn one_device_is_no_link() {
    let topology = example_topology();
    let ap = find(&topology, "schopf-burri");
    let mut routes = Some(Routes::none("test"));
    assert!(matches!(
        plan_with(&topology, &input(&ap, &ap), &mut routes).await,
        Err(Error::SameDevice(_))
    ));
}

/// The bridges of the export are candidates, with the device at the other end of the link.
#[test]
fn bridges_are_candidates_with_their_peer() {
    let topology = example_topology();
    let candidates = candidates(&topology);
    let schopf = candidates
        .iter()
        .find(|c| &*c.device.name == "schopf-burri")
        .expect("schopf-burri");
    assert_eq!(schopf.peer.as_ref().map(|p| &*p.name), Some("burri-schopf"));
    assert_eq!(
        schopf.router_port.as_ref().map(|p| &*p.device.name),
        Some("router-schopf")
    );
}
