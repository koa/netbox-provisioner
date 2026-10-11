//! Runs every RouterOS device of the stored production NetBox response
//! (`docs/netbox-response-example.json`) through the whole generator: L2 planning,
//! mutation diff against a reset device and dependency sorting. Catches what the
//! hand-built scenarios miss - data shapes nobody thought of, like the `combo1`
//! transfer links or bridge-less VLAN interfaces on APs.
//!
//! NetBox doesn't tell the generator the board model, so each device gets exactly
//! the ports - ethernet and radio - it declares in NetBox. That checks the config
//! logic, not whether a board actually has those ports.
//!
//! The generated scripts land in `target/netbox-example/<device>.rsc` for review.

use crate::{
    device::ros::{
        BaseDeviceDataTarget, W60GDeviceDataCurrent, W60GDeviceDataTarget,
        WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
        hw_facts::{w60g_port, wireless_bridge_port},
        l2::{EndpointNameGenerator, L2Setup},
        test::empty_current,
    },
    netbox::fetch_topology::ResponseData,
    topology::{PhysicalPortId, access::device::DeviceAccess, fetch::build_topology_from},
};
use mikrotik_model::{
    generator::Generator,
    hwconfig::{ADVERTISE_1G, EthernetNamePattern, generate_ethernet},
    model::{InterfaceW60GByDefaultName, InterfaceWirelessByDefaultName, ReferenceType},
    resource::ResourceMutation,
};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tokio::time::Instant;

/// Devices known not to generate yet, with the start of the expected error. Remove
/// an entry once it's fixed - the test fails on entries that start working, so the
/// list can't silently go stale.
const KNOWN_FAILURES: &[(&str, &str)] = &[];

#[tokio::test]
async fn generate_all_devices_of_netbox_example() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/netbox-response-example.json"
    ))
    .expect("example response");
    let mut raw: serde_json::Value = serde_json::from_str(&raw).expect("example response json");
    // Exports from before VLAN groups were queried with their custom fields lack
    // the field; treat those groups as having none (no VNI, nothing tunneled).
    for group in raw["data"]["vlan_group_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(group) = group.as_object_mut() {
            group
                .entry("custom_field_data")
                .or_insert_with(|| serde_json::json!({}));
        }
    }
    let response: graphql_client::Response<ResponseData> =
        serde_json::from_value(raw).expect("parse example response");
    let topology = Arc::new(build_topology_from(
        response.data.expect("response without data"),
        Instant::now(),
    ));
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/netbox-example");
    std::fs::create_dir_all(&out_dir).expect("output directory");

    let mut results = BTreeMap::new();
    for device in topology.list_devices().filter(|d| d.has_routeros()) {
        let name = device.name().to_string();
        let result = generate(&device);
        if let Ok(script) = &result {
            std::fs::write(out_dir.join(format!("{name}.rsc")), script).expect("write script");
        }
        results.insert(name, result);
    }
    assert!(results.len() > 30, "only {} devices found", results.len());

    let mut problems = Vec::new();
    for (name, result) in &results {
        let known = KNOWN_FAILURES.iter().find(|(n, _)| n == name);
        match (result, known) {
            (Ok(_), None) => {}
            (Ok(_), Some(_)) => {
                problems.push(format!("{name}: works now, drop it from KNOWN_FAILURES"))
            }
            (Err(e), Some((_, expected))) if e.starts_with(expected) => {}
            (Err(e), _) => problems.push(format!("{name}: {e}")),
        }
    }
    for (name, _) in KNOWN_FAILURES {
        if !results.contains_key(*name) {
            problems.push(format!("{name}: in KNOWN_FAILURES but not in the example"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

fn generate(device: &DeviceAccess) -> Result<String, String> {
    let mut target = BaseDeviceDataTarget::with_ethernet_ports(declared_ports(device));
    let current = empty_current(&target);
    let interfaces = device.interfaces();
    let radio_ports = interfaces.iter().filter_map(|i| i.external_port());
    let mut wireless = WirelessDeviceDataTarget {
        wireless_cap: Default::default(),
        wireless: radio_ports
            .clone()
            .filter_map(|port| match port {
                PhysicalPortId::Wlan(idx) => Some(wireless_bridge_port(idx.into(), 0)),
                _ => None,
            })
            .map(|port| (port.default_name, port.data))
            .collect(),
    };
    let mut w60g = W60GDeviceDataTarget {
        radio: radio_ports
            .filter_map(|port| match port {
                PhysicalPortId::W60g(idx) => Some(w60g_port(idx.into())),
                _ => None,
            })
            .map(|port| (port.default_name, port.data))
            .collect(),
    };
    // A reset device: the radios under their default names, as the board builds them.
    let wireless_current = WirelessDeviceDataCurrent {
        wireless_cap: Default::default(),
        wireless: wireless
            .wireless
            .iter()
            .map(|(default_name, data)| InterfaceWirelessByDefaultName {
                default_name: default_name.clone(),
                data: data.clone(),
            })
            .collect(),
    };
    let w60g_current = W60GDeviceDataCurrent {
        radio: w60g
            .radio
            .iter()
            .map(|(default_name, data)| InterfaceW60GByDefaultName {
                default_name: default_name.clone(),
                data: data.clone(),
            })
            .collect(),
    };
    target
        .generate_from(device, EndpointNameGenerator)
        .map_err(|e| format!("generate_from: {e:?}"))?;
    let l2 = L2Setup::new(device, EndpointNameGenerator).map_err(|e| format!("l2: {e:?}"))?;
    wireless
        .generate_from(device, &l2, Some(&wireless_current))
        .map_err(|e| format!("wireless: {e:?}"))?;
    w60g.generate_from(device, &l2, Some(&w60g_current))
        .map_err(|e| format!("w60g: {e:?}"))?;
    let mutations = [
        target.generate_mutations(&current),
        wireless.generate_mutations(&wireless_current),
        w60g.generate_mutations(&w60g_current),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| format!("mutations: {e:?}"))?
    .into_iter()
    .flat_map(Vec::from)
    .collect::<Vec<_>>();
    let mutations = ResourceMutation::sort_mutations_with_provided_dependencies(
        mutations.as_ref(),
        [
            (ReferenceType::Interface, b"lo".into()),
            (ReferenceType::RoutingTable, b"main".into()),
            (ReferenceType::FirewallChain, b"input".into()),
            (ReferenceType::FirewallChain, b"output".into()),
            (ReferenceType::FirewallChain, b"forward".into()),
        ],
    )
    .map_err(|e| format!("sort: {e:?}"))?;
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in mutations {
        generator
            .append_mutation(mutation)
            .map_err(|e| format!("script: {e}"))?;
    }
    Ok(script)
}

/// The ethernet-like ports the device declares in NetBox, as RouterOS would
/// create them.
fn declared_ports(
    device: &DeviceAccess,
) -> impl Iterator<Item = mikrotik_model::model::InterfaceEthernetByDefaultName> {
    device
        .interfaces()
        .iter()
        .filter_map(|i| i.external_port())
        .filter_map(|port| {
            let (pattern, idx) = match port {
                PhysicalPortId::Ethernet(n) => (EthernetNamePattern::Ether, n),
                PhysicalPortId::SfpSfpPlus(n) => (EthernetNamePattern::SfpSfpPlus, n),
                PhysicalPortId::Sfp28(n) => (EthernetNamePattern::Sfp28, n),
                PhysicalPortId::Sfp(n) => (EthernetNamePattern::Sfp, n),
                PhysicalPortId::Combo(n) => (EthernetNamePattern::Combo, n),
                PhysicalPortId::QsfpPlus(port, lane) => {
                    (EthernetNamePattern::QsfpPlus, (port - 1) * 4 + lane)
                }
                _ => return None,
            };
            Some(generate_ethernet(pattern, &ADVERTISE_1G, 1592, false)(
                idx as usize,
            ))
        })
        .collect::<Vec<_>>()
        .into_iter()
}
