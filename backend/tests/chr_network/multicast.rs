//! Scope: CHR tests of IPv6 behind bridges that snoop IGMP/MLD, as a WLAN's clients have it;
//! nothing else.
//!
//! `router -- cap -- client`: the router has the clients' VLAN on an interface with an IPv6
//! /64 it announces for SLAAC (`dhcp`), the cap carries the VLAN over a VXLAN to the router
//! (as a CAP's `bridge-caps`) to its access port, and the client, a CHR set up by hand,
//! configures its address from the router advertisements. Both bridges snoop, as the
//! generator has every bridge with a VXLAN; the router's is the querier and a permanent
//! multicast router (it has an address in the VLAN, `L2Setup::queries_multicast`). The cap's
//! access port stands in for its radios: the bridge forwards multicast by the same table.
//!
//! How long the client takes to its address and to reaching the router after it comes back
//! (as a phone does into the WLAN, it then asks for an advertisement, RS to `ff02::2`):
//! answered at once, it is a few seconds; waiting for the next unsolicited one, RouterOS sends
//! one every 200–600 s. CHR 7.24.6, 3 rounds each, the groups learned at the boot gone: without
//! a querier about 6 s, without snooping about 8 s, with the router's bridge the querier alone
//! not within 11 min (it registers its own groups in VLAN 1 only, not in VLAN 10 of its VLAN
//! interface), and with it a permanent multicast router as well 6 s again.

use crate::chr_network::default_route::apply_directly;
use crate::chr_network::node::{answers_ping, set_keyed, set_single};
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::vm::RunningNode;
use crate::chr_network::wait::wait_for_ospf_full;
use backend::device::access::Session;
use backend::device::ros::cfg::DeviceCfg;
use backend::device::ros::l2::KeepNameGenerator;
use backend::topology::access::device::DeviceAccess;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, Interface, IpAddressData, PhysicalPortId, TopologyHolder,
    VlanData, VlanGroupData, VxlanData,
};
use mikrotik_model::hwconfig::DeviceType;
use mikrotik_model::model::{
    InterfaceBridgeByName, InterfaceBridgeMulticastRouter, InterfaceEthernetByDefaultName,
    Ipv6Address, Ipv6SettingsAcceptRouterAdvertisements, Ipv6SettingsCfg,
};
use mikrotik_model::resource::{KeyedResource, collect_resource};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::time::{Duration, Instant};

/// The clients' network and the router's address in it.
const CLIENTS: &str = "2001:db8:61::/64";
const ROUTER_IN_CLIENTS: &str = "2001:db8:61::1";

struct Net {
    builder: TopologyBuilder,
    router: DeviceId,
    cap: DeviceId,
    client: DeviceId,
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
    let device = |builder: &mut TopologyBuilder, name: &str, loopback: Option<&str>| {
        let id = builder.devices.next_id();
        let loopback = loopback.map(|ip| address(builder, ip));
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                primary_ip_v4: loopback,
                loopback_ip: loopback,
                has_routeros: true,
                ..Default::default()
            },
        );
        id
    };
    let router = device(&mut builder, "router", Some("192.168.255.31/32"));
    let cap = device(&mut builder, "cap", Some("192.168.255.32/32"));
    let client = device(&mut builder, "client", None);

    let vxlan = builder.vxlans.next_id();
    let group = builder.vlan_groups.next_id();
    let vlan = builder.vlans.next_id();
    builder.vlans.insert(
        vlan,
        VlanData {
            name: "clients".into(),
            vlan_id: 10,
            group,
            terminations: Box::new([]),
            vxlan: Some(vxlan),
            wlans: Box::new([]),
        },
    );
    builder.vlan_groups.insert(
        group,
        VlanGroupData {
            name: "wlan".into(),
            vlans: Box::new([vlan]),
        },
    );
    builder.vxlans.insert(
        vxlan,
        VxlanData {
            name: "clients".into(),
            vni: 10,
            vlans: Box::new([vlan]),
        },
    );

    let interface = |builder: &mut TopologyBuilder, data: Interface| {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(id, data);
        id
    };
    let router_transfer = address(&mut builder, "10.0.60.0/31");
    let router_port = interface(
        &mut builder,
        Interface {
            name: "ether2".into(),
            device: router,
            external: Some(PhysicalPortId::Ethernet(2)),
            ips: Box::new([router_transfer]),
            use_ospf: true,
            ..Default::default()
        },
    );
    let router_clients = address(&mut builder, &format!("{ROUTER_IN_CLIENTS}/64"));
    interface(
        &mut builder,
        Interface {
            name: "vlan-clients".into(),
            device: router,
            vlan: Some(vlan),
            ips: Box::new([router_clients]),
            enable_dhcp_server: true,
            ..Default::default()
        },
    );
    let cap_transfer = address(&mut builder, "10.0.60.1/31");
    let cap_uplink = interface(
        &mut builder,
        Interface {
            name: "ether2".into(),
            device: cap,
            external: Some(PhysicalPortId::Ethernet(2)),
            ips: Box::new([cap_transfer]),
            use_ospf: true,
            ..Default::default()
        },
    );
    let cap_access = interface(
        &mut builder,
        Interface {
            name: "ether3".into(),
            device: cap,
            external: Some(PhysicalPortId::Ethernet(3)),
            vlan: Some(vlan),
            ..Default::default()
        },
    );
    let client_port = interface(
        &mut builder,
        Interface {
            name: "ether2".into(),
            device: client,
            external: Some(PhysicalPortId::Ethernet(2)),
            ..Default::default()
        },
    );
    for (a, b) in [(router_port, cap_uplink), (cap_access, client_port)] {
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
        router,
        cap,
        client,
    }
}

/// The client's address in the clients' network, by SLAAC (dynamic).
async fn slaac_address(client: &Session) -> Option<IpAddr> {
    let clients: ipnet::Ipv6Net = CLIENTS.parse().unwrap();
    collect_resource::<Ipv6Address>(client)
        .await
        .expect("ipv6 addresses")
        .iter()
        .find(|address| address.status.dynamic && clients.contains(&address.cfg.address.addr()))
        .map(|address| IpAddr::V6(address.cfg.address.addr()))
}

/// Turns the client's `ether2` off or on: the client leaving the WLAN and coming back.
async fn set_client_port_disabled(client: &Session, disabled: bool) {
    set_keyed(
        client,
        &"ether2".into(),
        |ethernet: &mut InterfaceEthernetByDefaultName| ethernet.data.disabled = disabled,
    )
    .await
    .expect("port switched");
}

/// Changes the bridge `bridge` of a device.
async fn set_bridge(node: &Session, change: impl FnOnce(&mut InterfaceBridgeByName)) {
    set_keyed(node, &"bridge".into(), change)
        .await
        .expect("bridge changed");
}

struct Booted {
    nodes: HashMap<DeviceId, RunningNode>,
    /// router and cap as in the topology
    devices: [DeviceAccess; 2],
    router: Session,
    cap: Session,
    client: Session,
}

/// The mutations the diff of `device` still has, as `path fields`: those that change fields,
/// but on `/interface/ethernet`, where the CHR's ports aren't the RB750Gr3's the target is for
/// (`l2mtu`, flow control).
async fn remaining_mutations(node: &RunningNode, device: &DeviceAccess) -> Vec<String> {
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
        .filter(|mutation| {
            !mutation.fields.is_empty() && mutation.resource != b"interface/ethernet".as_slice()
        })
        .map(|mutation| {
            let fields = mutation
                .fields
                .iter()
                .map(|field| {
                    format!(
                        "{}={}",
                        String::from_utf8_lossy(field.key),
                        String::from_utf8_lossy(&field.value)
                    )
                })
                .collect::<Vec<_>>();
            format!(
                "{} {}",
                String::from_utf8_lossy(mutation.resource),
                fields.join(" ")
            )
        })
        .collect()
}

/// The network booted, router and cap applied, the client taking router advertisements.
async fn boot(test: &str) -> Booted {
    let golden = crate::chr_network::golden_image::ensure_golden_image(
        &crate::cache_dir(),
        crate::TEST_PASSWORD,
    )
    .await
    .expect("golden image build failed");
    let Net {
        builder,
        router,
        cap,
        client,
    } = build();
    let boards = HashMap::from([
        (router, DeviceType::RB750Gr3),
        (cap, DeviceType::RB750Gr3),
        (client, DeviceType::RB750Gr3),
    ]);
    let work_dir = Path::new("/tmp").join(test);
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();
    let devices = [
        holder.devices_by_id(router).await.unwrap(),
        holder.devices_by_id(cap).await.unwrap(),
    ];
    for device in &devices {
        apply_directly(&nodes[&device.id()], device).await;
    }
    let router_api = nodes[&router]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("router api");
    let cap_api = nodes[&cap]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("cap api");
    wait_for_ospf_full(&cap_api, "default-v2", Duration::from_secs(120))
        .await
        .expect("ospf between router and cap");

    // by hand: the client is a host, configuring its address from the advertisements
    let client_api = nodes[&client]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("client api");
    set_single(&client_api, |settings: &mut Ipv6SettingsCfg| {
        settings.forward = Some(false);
        settings.accept_router_advertisements = Some(Ipv6SettingsAcceptRouterAdvertisements::Yes);
    })
    .await
    .expect("client takes router advertisements");
    Booted {
        nodes,
        devices,
        router: router_api,
        cap: cap_api,
        client: client_api,
    }
}

/// The client leaves and comes back: how long until it has its address again and router and
/// client reach each other over IPv6 (`None`: not within `limit`).
async fn time_to_address(booted: &Booted, limit: Duration) -> Option<Duration> {
    set_client_port_disabled(&booted.client, true).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    while slaac_address(&booted.client).await.is_some() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    set_client_port_disabled(&booted.client, false).await;
    let start = Instant::now();
    while start.elapsed() < limit {
        if let Some(address) = slaac_address(&booted.client).await {
            let router_address: IpAddr = ROUTER_IN_CLIENTS.parse().unwrap();
            if answers_ping(&booted.client, router_address, 3).await
                && answers_ping(&booted.router, address, 3).await
            {
                return Some(start.elapsed());
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    None
}

/// The apply leaves no diff, the router's bridge is the querier, the cap's not. Then explores
/// the time to the address after the client comes back, three times for each setting; the
/// first round waits past `membership-interval` (4m20s) since the boot, so the groups the
/// bridges learned at the start are gone.
#[tokio::test]
#[ignore]
async fn clients_behind_snooping_bridges_get_their_ipv6() {
    if crate::chr_network::qemu::nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let booted = boot("chr-network-multicast").await;
    for device in &booted.devices {
        assert_eq!(
            remaining_mutations(&booted.nodes[&device.id()], device).await,
            Vec::<String>::new(),
            "{} applied",
            device.name()
        );
    }
    for (node, querier) in [(&booted.router, true), (&booted.cap, false)] {
        let bridge = InterfaceBridgeByName::fetch(node, &"bridge".into())
            .await
            .expect("bridge read")
            .expect("bridge there");
        assert!(bridge.0.igmp_snooping);
        assert_eq!(bridge.0.multicast_querier, querier);
        assert_eq!(
            bridge.0.multicast_router == InterfaceBridgeMulticastRouter::Permanent,
            querier
        );
    }
    let limit = Duration::from_secs(3 * 60);
    let rounds = |setting: &'static str| {
        let booted = &booted;
        async move {
            for round in 1..=3 {
                let time = time_to_address(booted, limit).await;
                eprintln!("{setting}, round {round}: {time:?}");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
    };
    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
    rounds("the router's bridge querier and permanent multicast router (as generated)").await;

    set_bridge(&booted.router, |bridge| {
        bridge.0.multicast_router = InterfaceBridgeMulticastRouter::TemporaryQuery
    })
    .await;
    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
    rounds("the router's bridge querier only").await;

    set_bridge(&booted.router, |bridge| bridge.0.multicast_querier = false).await;
    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
    rounds("neither").await;
}
