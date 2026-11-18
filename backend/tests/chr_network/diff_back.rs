//! Scope: CHR test of the script back of a diff (`device::ros::diff_view`); nothing else.
//!
//! A router in line with NetBox gets changes by hand: another identity, an address NetBox
//! doesn't have, and NetBox's address taken away. The apply's script undoes them (a set, a
//! removal, an add); the script back, run after it, brings every one of them back, so the
//! diff is the one from before again.

use crate::chr_network::default_route::apply_directly;
use crate::chr_network::node::{add_address, set_identity};
use crate::chr_network::scenario::boot_scenario;
use backend::device::access::Session;
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::diff_view::revert_script;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::access::device::DeviceAccess;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{Device, Interface, IpAddressData, PhysicalPortId, TopologyHolder};
use mikrotik_model::generator::Generator;
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::IpAddressByAddress;
use mikrotik_model::resource::{ResourceMutation, apply_mutation, run_script};
use std::collections::HashMap;
use std::path::Path;

/// The tables the hand changes are in: the rest of the diff (the management port's DHCP
/// client) stays out of the scripts.
const TABLES: [&[u8]; 2] = [b"system/identity", b"ip/address"];

/// The script of the diff in [`TABLES`] and the script back.
async fn scripts(node: &Session, device: &DeviceAccess) -> (String, String) {
    let mut cfg = DeviceCfg::fetch_with_board_model(node, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator)
        .expect("generate");
    cfg.setup_services(None);
    let mutations: Vec<ResourceMutation> = cfg
        .sorted_mutations()
        .expect("mutations")
        .iter()
        .filter(|mutation| TABLES.contains(&mutation.resource))
        .cloned()
        .collect();
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in &mutations {
        generator.append_mutation(mutation).expect("script");
    }
    let back = revert_script(&mutations).expect("script back");
    (script, back.into())
}

#[tokio::test]
#[ignore]
async fn the_script_back_undoes_an_apply() {
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
    let mut builder = TopologyBuilder::default();
    let router = builder.devices.next_id();
    builder.devices.insert(
        router,
        Device {
            name: "router".into(),
            has_routeros: true,
            ..Default::default()
        },
    );
    let ip = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        ip,
        IpAddressData {
            ip: "192.168.77.1/24".parse().unwrap(),
            interface: None,
            prefix: None,
        },
    );
    let port = builder.interfaces.next_id();
    builder.interfaces.insert(
        port,
        Interface {
            name: "ether2".into(),
            device: router,
            external: Some(PhysicalPortId::Ethernet(2)),
            ips: Box::new([ip]),
            ..Default::default()
        },
    );
    let boards = HashMap::from([(router, DeviceType::RB750Gr3)]);
    let work_dir = Path::new("/tmp").join("chr-network-diff-back");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    let device = holder.devices_by_id(router).await.unwrap();
    let node = &nodes[&router];
    let api = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    apply_directly(node, &device).await;
    assert_eq!(
        scripts(&api, &device).await.0,
        "",
        "in line after the apply"
    );

    // by hand
    set_identity(&api, "by-hand").await;
    add_address(&api, "192.168.78.1/24", "e03").await;
    apply_mutation(
        &api,
        &ResourceMutation::remove::<IpAddressByAddress>(&"192.168.77.1/24".parse().unwrap()),
    )
    .await
    .expect("netbox's address removed");
    let (script, back) = scripts(&api, &device).await;
    for change in ["by-hand", "192.168.78.1/24", "192.168.77.1/24"] {
        assert!(
            back.contains(change),
            "{change} in the script back:\n{back}"
        );
    }

    run_script(&api, &script, "diff-back-apply")
        .await
        .expect("script applied");
    assert_eq!(
        scripts(&api, &device).await.0,
        "",
        "in line after the script"
    );
    run_script(&api, &back, "diff-back-revert")
        .await
        .unwrap_or_else(|error| panic!("script back failed: {error}\n{back}"));
    assert_eq!(
        scripts(&api, &device).await.0,
        script,
        "the hand changes back after the script back"
    );
}
