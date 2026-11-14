//! Scope: CHR tests of the router advertisements the apply sets up (`/ipv6/nd`); nothing else.
//!
//! A router with two client networks: RouterOS's own entry takes the first interface, the
//! second is added. The added one advertises the DNS servers too, by a `set` after its `add`
//! (`after-add`, `ResourceMutationOperation::AddThenSet`): with `advertise-dns=yes` in the
//! `add` RouterOS noted "automatic dns option advertising is not started, re-apply dns config"
//! (CHR 7.24.5, srv-router-berg 7.24.6) and sent no DNS. Over the API (`apply_directly`) and
//! in a script, as the provisioner applies.

use crate::chr_network::default_route::apply_directly;
use crate::chr_network::scenario::boot_scenario;
use backend::device::access::Session;
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::access::device::DeviceAccess;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{Device, Interface, IpAddressData, PhysicalPortId, TopologyHolder};
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::{Ipv6Nd, Ipv6NdByInterface};
use mikrotik_model::resource::{
    ResourceMutation, apply_mutation, apply_mutations_via_script, collect_resource,
};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;

const DNS: &str = "2001:db8::53";

/// `(interface, advertise-dns, RouterOS's note)` of each entry.
async fn entries(node: &Session) -> Vec<(String, Option<bool>, Option<String>)> {
    let mut entries = collect_resource::<Ipv6Nd>(node)
        .await
        .expect("nd read")
        .iter()
        .map(|nd| {
            (
                nd.cfg.interface.to_string(),
                nd.cfg.advertise_dns,
                nd.status.about.as_ref().map(ToString::to_string),
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

/// The diff's mutations of `/ipv6/nd` for `device` that change something.
async fn nd_mutations(node: &Session, device: &DeviceAccess) -> Vec<ResourceMutation<'static>> {
    let mut cfg = DeviceCfg::fetch_with_board_model(node, b"RB750Gr3", device)
        .await
        .expect("fetch");
    cfg.generate_from(device, KeepNameGenerator)
        .expect("generate");
    cfg.setup_services(None);
    cfg.generate_mutations()
        .expect("mutations")
        .iter()
        .filter(|mutation| mutation.resource == b"ipv6/nd".as_slice())
        // a `set` without changes is no mutation (the script leaves it out too)
        .filter(|mutation| !mutation.fields.is_empty())
        .map(|mutation| mutation.clone().into_owned())
        .collect()
}

#[tokio::test]
#[ignore]
async fn an_added_entry_advertises_the_dns_servers() {
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
            dns_servers: Box::new([DNS.parse::<IpAddr>().unwrap()]),
            ..Default::default()
        },
    );
    for (port, network) in [(2, "2001:db8:71::1/64"), (3, "2001:db8:72::1/64")] {
        let ip = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            ip,
            IpAddressData {
                ip: network.parse().unwrap(),
                interface: None,
                prefix: None,
            },
        );
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{port}").into(),
                device: router,
                external: Some(PhysicalPortId::Ethernet(port)),
                ips: Box::new([ip]),
                enable_dhcp_server: true,
                ..Default::default()
            },
        );
    }
    let boards = HashMap::from([(router, DeviceType::RB750Gr3)]);
    let work_dir = Path::new("/tmp").join("chr-network-router-advertisements");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    let device = holder.devices_by_id(router).await.unwrap();
    let node = &nodes[&router];
    let api = node.connect_api(crate::TEST_PASSWORD).await.expect("api");
    let advertising = || {
        vec![
            ("ether2".to_string(), Some(true), None),
            ("ether3".to_string(), Some(true), None),
        ]
    };

    // over the API
    apply_directly(node, &device).await;
    assert_eq!(entries(&api).await, advertising(), "over the API");
    assert!(nd_mutations(&api, &device).await.is_empty());

    // in a script: the added entry gone, the diff adds it again
    apply_mutation(
        &api,
        &ResourceMutation::remove::<Ipv6NdByInterface>(&"ether3".into()),
    )
    .await
    .expect("entry removed");
    let mutations = nd_mutations(&api, &device).await;
    assert_eq!(mutations.len(), 1, "{mutations:?}");
    apply_mutations_via_script(&api, &mutations, "router-advertisements-test")
        .await
        .expect("script applied");
    assert_eq!(entries(&api).await, advertising(), "in a script");
    assert!(nd_mutations(&api, &device).await.is_empty());
}
