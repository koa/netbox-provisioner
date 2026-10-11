use crate::{
    device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, test::empty_current},
    topology::{
        Cable, CablePort, Device, DeviceId, Interface, InterfaceId, IpAddressData, IpAddressId,
        IpPrefixData, IpRangeData, PhysicalPortId, TopologyHolder, VlanData, VlanGroupData,
        VlanGroupId, VlanId, VxlanData, WlanAuth, WlanData, WlanGroupData, WlanOpenSettings,
        access::device::DeviceAccess, test::TopologyBuilder,
    },
};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString, generator::Generator, model::ReferenceType, resource::ResourceMutation,
};
use std::{
    collections::HashMap,
    error::Error,
    net::{IpAddr, Ipv4Addr},
};

use super::{EndpointNameGenerator, KeepNameGenerator, L2Port, L2Setup, SimpleBridgePort};

#[tokio::test]
async fn test_l2_one_vlan() {
    let device = create_device_with_ports(1, 1, 3).await;
    let setup = L2Setup::new(&device, KeepNameGenerator).expect("no ambiguous vlans");

    assert_eq!(setup.single_ports.len(), 0);
    assert_eq!(setup.simple_bridges.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 1);

    let bridge = &setup.tagged_bridges[0];
    assert_eq!(bridge.name, AsciiString::from(b"bridge"));
    assert_eq!(bridge.ports.len(), 1);
    let entry = &bridge.ports[&1u16];
    assert_eq!(entry.if_name, AsciiString::from(b"vlan-1"));
    assert_eq!(count_ethernet(&entry.ports), 3);
    assert_eq!(count_ips(&entry.ports), 1);
}

#[tokio::test]
async fn test_l2_no_vlan() -> Result<(), Box<dyn Error>> {
    let device = create_device_with_ports(1, 0, 3).await;
    let setup = L2Setup::new(&device, KeepNameGenerator)?;

    assert_eq!(setup.single_ports.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 0);
    assert_eq!(setup.simple_bridges.len(), 1);
    let bridge = &setup.simple_bridges[0];
    assert_eq!(bridge.name, AsciiString::from(b"bridge"));
    let ethernet_count = bridge
        .ports
        .iter()
        .filter(|p| matches!(p, SimpleBridgePort::Ethernet { .. }))
        .count();
    let ip_count = bridge
        .ports
        .iter()
        .filter(|p| matches!(p, SimpleBridgePort::Ip(_)))
        .count();
    assert_eq!(ethernet_count, 3);
    assert_eq!(ip_count, 1);

    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.generate_from(&device, KeepNameGenerator)?;
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

#[tokio::test]
async fn test_l2_multi_vlan() {
    let device = create_device_with_ports(1, 3, 3).await;
    let setup = L2Setup::new(&device, KeepNameGenerator).expect("no ambiguous vlans");

    assert_eq!(setup.single_ports.len(), 0);
    assert_eq!(setup.simple_bridges.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 1);

    let bridge = &setup.tagged_bridges[0];
    assert_eq!(bridge.ports.len(), 3);
    for vlan_id in 1u16..=3 {
        let entry = bridge
            .ports
            .get(&vlan_id)
            .unwrap_or_else(|| panic!("missing vlan {vlan_id}"));
        assert_eq!(entry.if_name, AsciiString::from(format!("vlan-{vlan_id}")));
        assert_eq!(count_ethernet(&entry.ports), 1);
        assert_eq!(count_ips(&entry.ports), 1);
    }
}

#[tokio::test]
async fn test_l2_untagged_switch() {
    let device = create_device_with_ports(1, 5, 24).await;
    let setup = L2Setup::new(&device, KeepNameGenerator).expect("no ambiguous vlans");

    assert_eq!(setup.single_ports.len(), 0);
    assert_eq!(setup.simple_bridges.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 1);

    let bridge = &setup.tagged_bridges[0];
    assert_eq!(bridge.ports.len(), 5);
    let total_ethernet: usize = bridge
        .ports
        .values()
        .map(|e| count_ethernet(&e.ports))
        .sum();
    assert_eq!(total_ethernet, 24);
}

#[tokio::test]
async fn test_l2_multi_untagged_switch() -> Result<(), Box<dyn Error>> {
    let device = create_device_with_ports(5, 0, 24).await;
    let setup = L2Setup::new(&device, KeepNameGenerator)?;

    assert_eq!(setup.single_ports.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 0);
    assert_eq!(setup.simple_bridges.len(), 5);
    let total_ethernet: usize = setup
        .simple_bridges
        .iter()
        .map(|b| {
            b.ports
                .iter()
                .filter(|p| matches!(p, SimpleBridgePort::Ethernet { .. }))
                .count()
        })
        .sum();
    assert_eq!(total_ethernet, 24);

    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.setup_l2(&device, &setup)?;
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

/// Szenario "Kernrouter", nachgebaut nach dem echten Referenz-Export
/// `examples/01-core-router-ospf/config.rsc`: mehrere direkt geroutete,
/// gelabelte Kundenlinks (kein Bridge, kein VLAN - `SingleInterfaceEntry`),
/// Loopback + dual-stack OSPF. Nutzt - anders als die anderen Szenario-Tests hier -
/// bewusst `EndpointNameGenerator` statt `KeepNameGenerator`, weil genau dieser
/// Pfad (Label -> Interface-Name) der real produktiv genutzte ist und den im
/// Vergleich mit dem Referenz-Export gefundenen Namens-Bug (`EndpointNameGenerator`
/// ignorierte `label` komplett, siehe `test_endpoint_name_generator_uses_own_label`)
/// end-to-end abdeckt. Zeigt die generierten `/interface/ethernet`, `/ip/address`
/// und `/routing/ospf/*`-Befehle.
#[tokio::test]
async fn test_scenario_core_router() -> Result<(), Box<dyn Error>> {
    let mut builder = TopologyBuilder::default();
    let d1 = builder.devices.next_id();

    let loopback_id = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        loopback_id,
        IpAddressData {
            ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 255, 1)), 32).unwrap(),
            interface: None,
            prefix: None,
        },
    );

    for (idx, label, net) in [
        (2u16, "Hegglin", (172, 17, 0, 49)),
        (3, "Walter", (172, 17, 0, 57)),
        (4, "Burri", (172, 17, 0, 41)),
        (5, "AP Schopf", (172, 17, 1, 65)),
    ] {
        let if_id = builder.interfaces.next_id();
        let ip_id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            ip_id,
            IpAddressData {
                ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(net.0, net.1, net.2, net.3)), 29).unwrap(),
                interface: None,
                prefix: None,
            },
        );
        builder.interfaces.insert(
            if_id,
            Interface {
                name: format!("ether{idx}").into_boxed_str(),
                label: label.into(),
                device: d1,
                external: Some(PhysicalPortId::Ethernet(idx)),
                ips: Box::new([ip_id]),
                use_ospf: true,
                ..Default::default()
            },
        );
    }

    builder.devices.insert(
        d1,
        Device {
            name: "router-schopf".into(),
            primary_ip_v4: Some(loopback_id),
            loopback_ip: Some(loopback_id),
            ..Default::default()
        },
    );

    let device = Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found");

    let setup = L2Setup::new(&device, EndpointNameGenerator)?;
    assert_eq!(setup.single_ports.len(), 4);
    assert_eq!(setup.simple_bridges.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 0);
    let mut names: Vec<_> = setup
        .single_ports
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["e02-hegglin", "e03-walter", "e04-burri", "e05-ap-schopf"]
    );

    let (mut target_data, empty_current) = setup_testdata(b"CCR1009-7G-1C-1S+")?;
    target_data.generate_from(&device, EndpointNameGenerator)?;
    println!(
        "--- Szenario: Kernrouter nach examples/01-core-router-ospf (gelabelte Kundenlinks, OSPF, Loopback) ---"
    );
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

/// Szenario "Switch mit mehreren Haushalts-VLANs": ein Uplink mit zwei tagged VLANs,
/// je ein untagged Access-Port pro VLAN, OSPF auf der Bridge (gilt dann für alle ihre
/// VLAN-Subinterfaces). Zeigt die generierten `/interface/bridge`,
/// `/interface/bridge/port`, `/interface/bridge/vlan`, `/interface/vlan` und
/// `/routing/ospf/*`-Befehle.
#[tokio::test]
async fn test_scenario_switch_multi_vlan() -> Result<(), Box<dyn Error>> {
    let mut builder = TopologyBuilder::default();
    let d1 = builder.devices.next_id();

    let bridge_id = builder.interfaces.next_id();
    builder.interfaces.insert(
        bridge_id,
        Interface {
            name: "bridge-sw".into(),
            device: d1,
            use_ospf: true,
            ..Default::default()
        },
    );

    let vlan_group = builder.vlan_groups.next_id();
    let mut vlan_ids = Vec::new();
    for vid in [10u16, 20] {
        let vlan_id = builder.vlans.next_id();
        builder.vlans.insert(
            vlan_id,
            VlanData {
                name: format!("haushalt-{vid}").into_boxed_str(),
                vlan_id: vid,
                group: vlan_group,
                terminations: Box::new([]),
                vxlan: None,
                wlans: Box::new([]),
            },
        );
        vlan_ids.push(vlan_id);

        let gateway_ip = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            gateway_ip,
            IpAddressData {
                ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, vid as u8, 1)), 24).unwrap(),
                interface: None,
                prefix: None,
            },
        );
        let vlan_if_id = builder.interfaces.next_id();
        builder.interfaces.insert(
            vlan_if_id,
            Interface {
                name: format!("haushalt-{vid}").into_boxed_str(),
                device: d1,
                vlan: Some(vlan_id),
                bridge: Some(bridge_id),
                ips: Box::new([gateway_ip]),
                ..Default::default()
            },
        );

        let access_port_id = builder.interfaces.next_id();
        builder.interfaces.insert(
            access_port_id,
            Interface {
                name: format!("ether{}", vid / 10 + 1).into_boxed_str(),
                device: d1,
                external: Some(PhysicalPortId::Ethernet(vid / 10 + 1)),
                bridge: Some(bridge_id),
                vlan: Some(vlan_id),
                ..Default::default()
            },
        );
    }
    builder.vlan_groups.insert(
        vlan_group,
        VlanGroupData {
            vlans: vlan_ids.clone().into_boxed_slice(),
        },
    );

    let uplink_id = builder.interfaces.next_id();
    builder.interfaces.insert(
        uplink_id,
        Interface {
            name: "ether1".into(),
            label: "Uplink".into(),
            device: d1,
            external: Some(PhysicalPortId::Ethernet(1)),
            bridge: Some(bridge_id),
            tagged_vlans: vlan_ids.clone().into_boxed_slice(),
            ..Default::default()
        },
    );

    builder.devices.insert(
        d1,
        Device {
            name: "switch-1".into(),
            ..Default::default()
        },
    );

    let device = Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found");

    let setup = L2Setup::new(&device, KeepNameGenerator)?;
    assert_eq!(setup.tagged_bridges.len(), 1);
    assert_eq!(setup.tagged_bridges[0].ports.len(), 2);

    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.generate_from(&device, KeepNameGenerator)?;
    println!(
        "--- Szenario: Switch mit 2 Haushalts-VLANs (tagged Uplink, untagged Access, OSPF auf der Bridge) ---"
    );
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

/// Szenario "DHCP-Haushalt": ein Haushalts-VLAN mit einem `IpRangeData` (`is_dhcp`),
/// DHCP-Server auf der Bridge aktiviert. Zeigt, dass `setup_ip_addresses` den
/// `GapFinder`-Pool-Bereich jetzt End-to-End über die echte Pipeline erzeugt (bisher
/// nur isoliert in `device/ros/test.rs` getestet), inkl. `/ip/pool`,
/// `/ip/dhcp-server` und `/ip/dhcp-server/network`.
#[tokio::test]
async fn test_scenario_dhcp_household() -> Result<(), Box<dyn Error>> {
    let mut builder = TopologyBuilder::default();
    let d1 = builder.devices.next_id();

    let bridge_id = builder.interfaces.next_id();
    builder.interfaces.insert(
        bridge_id,
        Interface {
            name: "bridge-sw".into(),
            device: d1,
            enable_dhcp_server: true,
            ..Default::default()
        },
    );

    let vlan_group = builder.vlan_groups.next_id();
    let vlan_id = builder.vlans.next_id();
    builder.vlans.insert(
        vlan_id,
        VlanData {
            name: "haushalt-50".into(),
            vlan_id: 50,
            group: vlan_group,
            terminations: Box::new([]),
            vxlan: None,
            wlans: Box::new([]),
        },
    );
    builder.vlan_groups.insert(
        vlan_group,
        VlanGroupData {
            vlans: Box::new([vlan_id]),
        },
    );

    let prefix_id = builder.ip_prefixes.next_id();
    builder.ip_prefixes.insert(
        prefix_id,
        IpPrefixData {
            prefix: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 50, 0)), 24).unwrap(),
            addresses: Box::new([]),
            children: Box::new([]),
            parent: None,
            ranges: Box::new([]),
        },
    );
    let range_id = builder.ip_ranges.next_id();
    builder.ip_ranges.insert(
        range_id,
        IpRangeData {
            is_dhcp: true,
            net: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 50, 0)), 24).unwrap(),
            start: IpAddr::V4(Ipv4Addr::new(192, 168, 50, 100)),
            end: IpAddr::V4(Ipv4Addr::new(192, 168, 50, 200)),
            prefix: Some(prefix_id),
        },
    );

    let gateway_ip = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        gateway_ip,
        IpAddressData {
            ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 50, 1)), 24).unwrap(),
            interface: None,
            prefix: Some(prefix_id),
        },
    );
    let vlan_if_id = builder.interfaces.next_id();
    builder.interfaces.insert(
        vlan_if_id,
        Interface {
            name: "haushalt-50".into(),
            device: d1,
            vlan: Some(vlan_id),
            bridge: Some(bridge_id),
            ips: Box::new([gateway_ip]),
            ..Default::default()
        },
    );

    let access_port_id = builder.interfaces.next_id();
    builder.interfaces.insert(
        access_port_id,
        Interface {
            name: "ether4".into(),
            device: d1,
            external: Some(PhysicalPortId::Ethernet(4)),
            bridge: Some(bridge_id),
            vlan: Some(vlan_id),
            ..Default::default()
        },
    );

    builder.devices.insert(
        d1,
        Device {
            name: "switch-2".into(),
            ..Default::default()
        },
    );

    let device = Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found");

    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.generate_from(&device, KeepNameGenerator)?;
    println!(
        "--- Szenario: DHCP-Haushalts-VLAN (Pool/Range automatisch aus IpPrefixData/IpRangeData abgeleitet) ---"
    );
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

/// Ids of the two-device topology built by `build_tunnel_topology`.
#[derive(Clone, Copy)]
struct TunnelIds {
    switch: DeviceId,
    ap: DeviceId,
    vlan: VlanId,
    vlan_group: VlanGroupId,
    switch_bridge: InterfaceId,
    switch_port: InterfaceId,
    ap_port: InterfaceId,
}

/// A switch `switch-haus` whose household ports `ether2`/`ether3` (on `bridge-sw`)
/// and a second device `ap-wohnung` whose `ether2` both carry the household VLAN
/// `wohnung-koa` (VLAN 1) of a VLAN group with VNI 1 - so the VLAN is used on two
/// devices and gets tunneled. `customize` adjusts the topology before it's built.
async fn build_tunnel_topology(
    customize: impl FnOnce(&mut TopologyBuilder, TunnelIds),
) -> (DeviceAccess, DeviceAccess) {
    let mut builder = TopologyBuilder::default();
    let switch = builder.devices.next_id();
    let ap = builder.devices.next_id();
    let vxlan_id = builder.vxlans.next_id();

    let switch_ip = add_ip(&mut builder, [172, 16, 1, 70], 32);
    let ap_ip = add_ip(&mut builder, [172, 16, 1, 61], 32);

    let vlan_group = builder.vlan_groups.next_id();
    let vlan = builder.vlans.next_id();
    builder.vlans.insert(
        vlan,
        VlanData {
            name: "wohnung-koa".into(),
            vlan_id: 1,
            group: vlan_group,
            terminations: Box::new([]),
            vxlan: Some(vxlan_id),
            wlans: Box::new([]),
        },
    );
    builder.vlan_groups.insert(
        vlan_group,
        VlanGroupData {
            vlans: Box::new([vlan]),
        },
    );
    builder.vxlans.insert(
        vxlan_id,
        VxlanData {
            name: "client-berg".into(),
            vni: 1,
            vlans: Box::new([vlan]),
        },
    );

    let bridge = builder.interfaces.next_id();
    builder.interfaces.insert(
        bridge,
        Interface {
            name: "bridge-sw".into(),
            device: switch,
            ..Default::default()
        },
    );
    let uplink = builder.interfaces.next_id();
    builder.interfaces.insert(
        uplink,
        Interface {
            name: "ether1".into(),
            device: switch,
            external: Some(PhysicalPortId::Ethernet(1)),
            use_ospf: true,
            ..Default::default()
        },
    );
    let mut switch_port = None;
    for idx in [2u16, 3] {
        let port = builder.interfaces.next_id();
        switch_port.get_or_insert(port);
        builder.interfaces.insert(
            port,
            Interface {
                name: format!("ether{idx}").into_boxed_str(),
                device: switch,
                external: Some(PhysicalPortId::Ethernet(idx)),
                bridge: Some(bridge),
                vlan: Some(vlan),
                ..Default::default()
            },
        );
    }
    let ap_port = builder.interfaces.next_id();
    builder.interfaces.insert(
        ap_port,
        Interface {
            name: "ether2".into(),
            device: ap,
            external: Some(PhysicalPortId::Ethernet(2)),
            vlan: Some(vlan),
            ..Default::default()
        },
    );
    builder.devices.insert(
        switch,
        Device {
            name: "switch-haus".into(),
            primary_ip_v4: Some(switch_ip),
            ..Default::default()
        },
    );
    builder.devices.insert(
        ap,
        Device {
            name: "ap-wohnung".into(),
            primary_ip_v4: Some(ap_ip),
            ..Default::default()
        },
    );
    customize(
        &mut builder,
        TunnelIds {
            switch,
            ap,
            vlan,
            vlan_group,
            switch_bridge: bridge,
            switch_port: switch_port.unwrap(),
            ap_port,
        },
    );

    let holder = Into::<TopologyHolder>::into(builder);
    (
        holder.devices_by_id(switch).await.expect("switch"),
        holder.devices_by_id(ap).await.expect("ap"),
    )
}

fn add_ip(builder: &mut TopologyBuilder, address: [u8; 4], prefix_len: u8) -> IpAddressId {
    let ip_id = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        ip_id,
        IpAddressData {
            ip: IpNet::new(IpAddr::V4(Ipv4Addr::from(address)), prefix_len).unwrap(),
            interface: None,
            prefix: None,
        },
    );
    ip_id
}

/// A virtual interface `name` on `device` holding `address` in `vlan`.
fn add_vlan_interface(
    builder: &mut TopologyBuilder,
    device: DeviceId,
    vlan: VlanId,
    name: &str,
    address: [u8; 4],
    dhcp_server: bool,
) {
    let ip = add_ip(builder, address, 24);
    let interface = builder.interfaces.next_id();
    builder.interfaces.insert(
        interface,
        Interface {
            name: name.into(),
            device,
            vlan: Some(vlan),
            ips: Box::new([ip]),
            enable_dhcp_server: dhcp_server,
            ..Default::default()
        },
    );
}

fn has_vxlan(setup: &L2Setup<KeepNameGenerator>) -> bool {
    setup
        .tagged_bridges
        .iter()
        .flat_map(|b| b.ports.values())
        .flat_map(|entry| entry.ports.iter())
        .any(|p| matches!(p, L2Port::VxLan { .. }))
}

/// Szenario "Port-Gruppe im VXLAN": ein Drucker an `ether2`/`ether3` einer Wohnung
/// soll im selben L2 wie ein Port eines anderen Geräts landen. Beide Seiten tragen
/// nur das VLAN; weil es auf zwei Geräten vorkommt und seine VLAN-Gruppe eine VNI
/// hat, wird es getunnelt. Die Ports bleiben untagged im VLAN, das VXLAN-Interface
/// wird getaggtes Mitglied (das VXLAN ist ein VLAN-Trunk der Gruppe).
#[tokio::test]
async fn test_scenario_switch_vxlan_port_group() -> Result<(), Box<dyn Error>> {
    let (device, _) = build_tunnel_topology(|_, _| {}).await;

    let setup = L2Setup::new(&device, KeepNameGenerator)?;
    assert_eq!(
        setup.single_ports.len(),
        1,
        "uplink stays a standalone port"
    );
    assert_eq!(setup.tagged_bridges.len(), 1);
    let entry = &setup.tagged_bridges[0].ports[&1];
    assert_eq!(count_ethernet(&entry.ports), 2);
    assert!(entry.ports.iter().any(
        |p| matches!(p, L2Port::VxLan { name, .. } if name == &AsciiString::from("client-berg"))
    ));

    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.generate_from(&device, KeepNameGenerator)?;

    let vxlan_name = AsciiString::from("client-berg");
    let own_ip = IpAddr::V4(Ipv4Addr::new(172, 16, 1, 70));
    let remote_ip = IpAddr::V4(Ipv4Addr::new(172, 16, 1, 61));
    let vxlan = &target_data.vxlan[&vxlan_name].0;
    assert_eq!(vxlan.vni, 1);
    assert_eq!(vxlan.local_address, Some(own_ip));
    assert!(
        target_data
            .vxlan_vteps
            .contains_key(&(vxlan_name.clone(), remote_ip))
    );
    assert!(
        !target_data
            .vxlan_vteps
            .contains_key(&(vxlan_name.clone(), own_ip))
    );
    assert!(
        target_data
            .bridge_vlan
            .keys()
            .any(|(_, tagged, untagged, _)| {
                tagged.contains(&vxlan_name)
                    && untagged.contains(&AsciiString::from("ether2"))
                    && untagged.contains(&AsciiString::from("ether3"))
            })
    );

    println!("--- Szenario: Switch-Port-Gruppe einer Wohnung im Haushalts-VXLAN ---");
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

/// The household VLAN set on the bridge `bridge-sw` instead of its ports: the
/// ports without a VLAN of their own are in the bridge's VLAN, so the VXLAN joins
/// them in that bridge rather than landing alone on the default bridge.
#[tokio::test]
async fn test_vlan_on_bridge_applies_to_its_ports() -> Result<(), Box<dyn Error>> {
    let (device, _) = build_tunnel_topology(|builder, ids| {
        for interface in builder.interfaces.values_mut() {
            if interface.device == ids.switch && interface.bridge == Some(ids.switch_bridge) {
                interface.vlan = None;
            }
        }
        builder.interfaces.get_mut(&ids.switch_bridge).unwrap().vlan = Some(ids.vlan);
    })
    .await;

    let setup = L2Setup::new(&device, KeepNameGenerator)?;
    assert_eq!(setup.tagged_bridges.len(), 1, "no separate default bridge");
    let entry = &setup.tagged_bridges[0].ports[&1];
    assert_eq!(count_ethernet(&entry.ports), 2);
    assert!(
        entry
            .ports
            .iter()
            .any(|p| matches!(p, L2Port::VxLan { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn test_vlan_on_a_single_device_stays_local() -> Result<(), Box<dyn Error>> {
    let (device, _) = build_tunnel_topology(|builder, ids| {
        builder.interfaces.get_mut(&ids.ap_port).unwrap().vlan = None;
    })
    .await;
    assert!(!has_vxlan(&L2Setup::new(&device, KeepNameGenerator)?));
    Ok(())
}

#[tokio::test]
async fn test_vlan_group_without_vni_stays_local() -> Result<(), Box<dyn Error>> {
    let (device, _) = build_tunnel_topology(|builder, ids| {
        builder.vlans.get_mut(&ids.vlan).unwrap().vxlan = None;
    })
    .await;
    assert!(!has_vxlan(&L2Setup::new(&device, KeepNameGenerator)?));
    Ok(())
}

/// A VLAN cabled between the two devices in addition to the tunnel would loop.
#[tokio::test]
async fn test_vlan_on_cable_and_tunnel_is_rejected() {
    let (device, ap) = build_tunnel_topology(|builder, ids| {
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(ids.switch_port)]),
                port_b: Box::new([CablePort::Interface(ids.ap_port)]),
            },
        );
    })
    .await;
    for device in [device, ap] {
        assert!(matches!(
            L2Setup::new(&device, KeepNameGenerator),
            Err(SetupError::VlanOnCableAndTunnel { .. })
        ));
    }
}

/// Like `1og-sw1`'s `switch-lan` next to the controller's network of the same
/// VLAN: two networks would end up in one L2 domain.
#[tokio::test]
async fn test_two_networks_in_one_l2_domain_are_rejected() {
    let (device, ap) = build_tunnel_topology(|builder, ids| {
        add_vlan_interface(builder, ids.switch, ids.vlan, "lan", [10, 192, 3, 1], false);
        add_vlan_interface(builder, ids.ap, ids.vlan, "lan", [10, 192, 2, 1], false);
    })
    .await;
    for device in [device, ap] {
        let result = L2Setup::new(&device, KeepNameGenerator);
        let Err(SetupError::MultipleNetworksOnVlan { networks, .. }) = result else {
            panic!("expected MultipleNetworksOnVlan, got {result:?}");
        };
        assert_eq!(
            networks,
            [
                "10.192.2.0/24 (ap-wohnung:lan)",
                "10.192.3.0/24 (switch-haus:lan)"
            ]
        );
    }
}

/// Two addresses of the same network on both ends (gateway plus a switch's own
/// address) are one network, not two.
#[tokio::test]
async fn test_same_network_on_two_devices_is_fine() -> Result<(), Box<dyn Error>> {
    let (device, _) = build_tunnel_topology(|builder, ids| {
        add_vlan_interface(builder, ids.switch, ids.vlan, "lan", [10, 192, 3, 2], false);
        add_vlan_interface(builder, ids.ap, ids.vlan, "lan", [10, 192, 3, 1], true);
    })
    .await;
    assert!(has_vxlan(&L2Setup::new(&device, KeepNameGenerator)?));
    Ok(())
}

#[tokio::test]
async fn test_two_dhcp_servers_in_one_l2_domain_are_rejected() {
    let (device, _) = build_tunnel_topology(|builder, ids| {
        add_vlan_interface(builder, ids.switch, ids.vlan, "lan", [10, 192, 3, 2], true);
        add_vlan_interface(builder, ids.ap, ids.vlan, "lan", [10, 192, 3, 1], true);
    })
    .await;
    assert!(matches!(
        L2Setup::new(&device, KeepNameGenerator),
        Err(SetupError::MultipleDhcpServersOnVlan { servers, .. })
            if servers.len() == 2
    ));
}

/// Two tunneled VLANs of the same group on two different bridges would make the
/// group's VXLAN a member of both bridges, which RouterOS refuses.
#[tokio::test]
async fn test_vxlan_on_two_bridges_is_rejected() {
    let (device, _) = build_tunnel_topology(|builder, ids| {
        let second_vlan = builder.vlans.next_id();
        builder.vlans.insert(
            second_vlan,
            VlanData {
                name: "wohnung-gast".into(),
                vlan_id: 2,
                group: ids.vlan_group,
                terminations: Box::new([]),
                vxlan: builder.vlans.get(&ids.vlan).unwrap().vxlan,
                wlans: Box::new([]),
            },
        );
        let vxlan = builder.vlans.get(&ids.vlan).unwrap().vxlan.unwrap();
        builder.vxlans.get_mut(&vxlan).unwrap().vlans = Box::new([ids.vlan, second_vlan]);
        let bridge = builder.interfaces.next_id();
        builder.interfaces.insert(
            bridge,
            Interface {
                name: "bridge-gast".into(),
                device: ids.switch,
                ..Default::default()
            },
        );
        for (device, idx, bridge) in [(ids.switch, 4, Some(bridge)), (ids.ap, 3, None)] {
            let port = builder.interfaces.next_id();
            builder.interfaces.insert(
                port,
                Interface {
                    name: format!("ether{idx}").into_boxed_str(),
                    device,
                    external: Some(PhysicalPortId::Ethernet(idx)),
                    bridge,
                    vlan: Some(second_vlan),
                    ..Default::default()
                },
            );
        }
    })
    .await;
    let result = L2Setup::new(&device, KeepNameGenerator);
    assert!(
        matches!(&result, Err(SetupError::PortInMultipleBridges(name)) if name == &AsciiString::from("client-berg")),
        "{result:?}"
    );
}

/// Builds a WLAN AP whose `ether2`/`ether3` carry the VXLAN-transported household
/// VLAN untagged without naming a bridge (like `ap-mimi-buero` in production),
/// plus a VLAN interface `haushalt` (no bridge either) holding the VLAN's gateway
/// address and the `dhcp` tag. With `explicit_bridge`, `ether1` is additionally a
/// member of a modeled bridge `bridge-sw` in the same VLAN.
async fn create_default_bridge_device(explicit_bridge: bool) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let device = builder.devices.next_id();
    let controller = builder.devices.next_id();
    let vxlan_id = builder.vxlans.next_id();
    let wlan_group = builder.wlan_groups.next_id();
    let wlan = builder.wlans.next_id();

    let controller_ip = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        controller_ip,
        IpAddressData {
            ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(172, 16, 1, 1)), 32).unwrap(),
            interface: None,
            prefix: None,
        },
    );

    let own_ip = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        own_ip,
        IpAddressData {
            ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(172, 16, 1, 70)), 32).unwrap(),
            interface: None,
            prefix: None,
        },
    );
    let prefix = builder.ip_prefixes.next_id();
    builder.ip_prefixes.insert(
        prefix,
        IpPrefixData {
            prefix: IpNet::new(IpAddr::V4(Ipv4Addr::new(10, 192, 3, 0)), 24).unwrap(),
            addresses: Box::new([]),
            children: Box::new([]),
            parent: None,
            ranges: Box::new([]),
        },
    );
    let gateway_ip = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        gateway_ip,
        IpAddressData {
            ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(10, 192, 3, 1)), 24).unwrap(),
            interface: None,
            prefix: Some(prefix),
        },
    );

    let vlan_group = builder.vlan_groups.next_id();
    let vlan = builder.vlans.next_id();
    builder.vlans.insert(
        vlan,
        VlanData {
            name: "wlan-mimi".into(),
            vlan_id: 4,
            group: vlan_group,
            terminations: Box::new([]),
            vxlan: Some(vxlan_id),
            wlans: Box::new([wlan]),
        },
    );
    builder.vlan_groups.insert(
        vlan_group,
        VlanGroupData {
            vlans: Box::new([vlan]),
        },
    );
    builder.wlans.insert(
        wlan,
        WlanData {
            ssid: "mimi".into(),
            vlan: Some(vlan),
            wlan_auth: WlanAuth::Open(WlanOpenSettings { use_owe: true }),
            wlan_group,
        },
    );
    builder.wlan_groups.insert(
        wlan_group,
        WlanGroupData {
            name: "berg".into(),
            mgmt_vlan: None,
            controller,
            aps: Box::new([device]),
            wlans: Box::new([wlan]),
        },
    );
    builder.vxlans.insert(
        vxlan_id,
        VxlanData {
            name: "client-berg".into(),
            vni: 1,
            vlans: Box::new([vlan]),
        },
    );

    if explicit_bridge {
        let bridge = builder.interfaces.next_id();
        builder.interfaces.insert(
            bridge,
            Interface {
                name: "bridge-sw".into(),
                device,
                ..Default::default()
            },
        );
        let port = builder.interfaces.next_id();
        builder.interfaces.insert(
            port,
            Interface {
                name: "ether1".into(),
                device,
                external: Some(PhysicalPortId::Ethernet(1)),
                bridge: Some(bridge),
                vlan: Some(vlan),
                ..Default::default()
            },
        );
    }
    for idx in [2u16, 3] {
        let port = builder.interfaces.next_id();
        builder.interfaces.insert(
            port,
            Interface {
                name: format!("ether{idx}").into_boxed_str(),
                device,
                external: Some(PhysicalPortId::Ethernet(idx)),
                vlan: Some(vlan),
                ..Default::default()
            },
        );
    }
    let vlan_if = builder.interfaces.next_id();
    builder.interfaces.insert(
        vlan_if,
        Interface {
            name: "haushalt".into(),
            device,
            vlan: Some(vlan),
            ips: Box::new([gateway_ip]),
            enable_dhcp_server: true,
            ..Default::default()
        },
    );
    builder.devices.insert(
        device,
        Device {
            name: "ap-wohnung".into(),
            primary_ip_v4: Some(own_ip),
            wlan_ap_of: Some(wlan_group),
            ..Default::default()
        },
    );
    builder.devices.insert(
        controller,
        Device {
            name: "srv-router".into(),
            primary_ip_v4: Some(controller_ip),
            wlan_controller_of: Some(wlan_group),
            ..Default::default()
        },
    );

    Into::<TopologyHolder>::into(builder)
        .devices_by_id(device)
        .await
        .expect("device not found")
}

/// Szenario "Standard-Bridge" auf einem AP: Ports, die ein VLAN tragen, aber in
/// NetBox keine Bridge nennen, landen zusammen mit dem VXLAN und dem CAP-Datapath
/// dieses VLANs auf `bridge-caps`, der Standard-Bridge eines APs; die Tags des
/// VLAN-Interfaces (`dhcp`) gelten für das VLAN, obwohl keine Bridge modelliert
/// ist. Das VXLAN ist pro VLAN getaggt (nicht als `1-4094`-Bereich, den RouterOS
/// neben den Access-Port-Einträgen mit "vlan already added" ablehnt).
#[tokio::test]
async fn test_scenario_default_bridge_without_netbox_bridge() -> Result<(), Box<dyn Error>> {
    let device = create_default_bridge_device(false).await;

    let setup = L2Setup::new(&device, KeepNameGenerator)?;
    assert_eq!(setup.single_ports.len(), 0);
    assert_eq!(setup.simple_bridges.len(), 0);
    assert_eq!(setup.tagged_bridges.len(), 1);
    let bridge = &setup.tagged_bridges[0];
    assert_eq!(bridge.name, AsciiString::from(b"bridge-caps"));
    let entry = &bridge.ports[&4];
    assert_eq!(count_ethernet(&entry.ports), 2);
    assert_eq!(count_ips(&entry.ports), 1);
    assert!(
        entry
            .ports
            .iter()
            .any(|p| matches!(p, L2Port::VxLan { .. }))
    );
    let logical = setup
        .logical_interfaces()
        .find(|l| l.if_name == &entry.if_name)
        .expect("vlan entry is a logical interface");
    assert!(
        logical.enable_dhcp_server,
        "dhcp tag of the VLAN interface applies"
    );

    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.generate_from(&device, KeepNameGenerator)?;
    let vxlan_name = AsciiString::from("client-berg");
    let caps_bridge = AsciiString::from("bridge-caps");
    assert_eq!(
        target_data.bridge.len(),
        1,
        "only bridge-caps, no second bridge"
    );
    assert!(target_data.bridge.contains_key(&caps_bridge));
    assert!(
        target_data
            .vxlan_vteps
            .contains_key(&(vxlan_name.clone(), IpAddr::V4(Ipv4Addr::new(172, 16, 1, 1))))
    );
    assert!(
        target_data
            .bridge_vlan
            .keys()
            .all(|(bridge, ..)| bridge == &caps_bridge)
    );
    assert!(
        target_data
            .bridge_vlan
            .keys()
            .any(|(_, tagged, untagged, _)| {
                tagged.contains(&vxlan_name)
                    && untagged.contains(&AsciiString::from("ether2"))
                    && untagged.contains(&AsciiString::from("ether3"))
            })
    );
    assert!(
        target_data
            .dhcp_v_4_server
            .values()
            .any(|s| s.0.interface == AsciiString::from("vlan-4"))
    );
    println!("--- Szenario: AP-Ports ohne NetBox-Bridge auf der Standard-Bridge (bridge-caps) ---");
    dump_mutations(&target_data, &empty_current)?;
    Ok(())
}

#[tokio::test]
async fn test_default_bridge_merges_into_explicit_bridge_of_same_vlan() -> Result<(), Box<dyn Error>>
{
    let device = create_default_bridge_device(true).await;

    let setup = L2Setup::new(&device, KeepNameGenerator)?;
    assert_eq!(setup.tagged_bridges.len(), 1, "no separate default bridge");
    let entry = &setup.tagged_bridges[0].ports[&4];
    assert_eq!(count_ethernet(&entry.ports), 3);
    assert!(
        entry
            .ports
            .iter()
            .any(|p| matches!(p, L2Port::VxLan { .. }))
    );
    Ok(())
}

fn count_ethernet(ports: &std::collections::BTreeSet<L2Port>) -> usize {
    ports
        .iter()
        .filter(|p| {
            matches!(
                p,
                L2Port::TaggedEthernet { .. } | L2Port::UntaggedEthernet { .. }
            )
        })
        .count()
}

fn count_ips(ports: &std::collections::BTreeSet<L2Port>) -> usize {
    ports
        .iter()
        .filter(|p| matches!(p, L2Port::Ip { .. }))
        .count()
}

fn dump_mutations(
    target_data: &BaseDeviceDataTarget,
    empty_current: &BaseDeviceDataCurrent,
) -> Result<(), Box<dyn Error>> {
    let mutations = target_data.generate_mutations(empty_current)?;
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
    .unwrap();
    let mut cfg = String::new();
    let mut generator = Generator::new(&mut cfg);
    for mutation in mutations {
        generator.append_mutation(mutation)?;
    }
    println!("{}", cfg.into_boxed_str());
    Ok(())
}

fn setup_testdata(
    model: &[u8],
) -> Result<(BaseDeviceDataTarget, BaseDeviceDataCurrent), Box<dyn Error>> {
    let target_data = BaseDeviceDataTarget::new(model)?;
    let empty_current = empty_current(&target_data);
    Ok((target_data, empty_current))
}

async fn create_device_with_ports(
    bridge_count: u16,
    vlan_count: u16,
    port_count: usize,
) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let d1 = builder.devices.next_id();
    let mut bridges = Vec::new();
    for bridge_idx in 0..bridge_count.max(1) {
        let bridge_id = builder.interfaces.next_id();
        bridges.push(bridge_id);
        let ips: Box<[IpAddressId]> = if vlan_count > 0 {
            Box::from([])
        } else {
            let ip_id = builder.ip_addresses.next_id();
            builder.ip_addresses.insert(
                ip_id,
                IpAddressData {
                    ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, bridge_idx as u8, 1)), 24)
                        .unwrap(),
                    interface: None,
                    prefix: None,
                },
            );
            Box::new([ip_id])
        };
        builder.interfaces.insert(
            bridge_id,
            Interface {
                name: format!("bridge-{bridge_idx}").into(),
                label: "Bridge ".into(),
                device: d1,
                ips,
                ..Default::default()
            },
        );
    }
    let mut vlans = Vec::new();
    let mut vlan_ports = Vec::new();
    let mut bridge_of_vlan = HashMap::<_, Vec<_>>::new();
    if vlan_count > 0 {
        let vlan_group = builder.vlan_groups.next_id();
        for vid in 0..vlan_count {
            let vlan_id = builder.vlans.next_id();
            let string = format!("vlan-{}", vid + 1);
            builder.vlans.insert(
                vlan_id,
                VlanData {
                    name: string.clone().into_boxed_str(),
                    vlan_id: vid + 1,
                    group: vlan_group,
                    terminations: Box::new([]),
                    vxlan: None,
                    wlans: Box::new([]),
                },
            );
            vlans.push(vlan_id);
            let vlan_if_id = builder.interfaces.next_id();
            vlan_ports.push(vlan_if_id);
            let bridge_id = bridges[vid as usize % bridges.len()];
            bridge_of_vlan
                .entry(Some(vlan_id))
                .or_default()
                .push(bridge_id);
            let ip_id = builder.ip_addresses.next_id();
            builder.ip_addresses.insert(
                ip_id,
                IpAddressData {
                    ip: IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, (vid + 1) as u8, 1)), 24)
                        .unwrap(),
                    interface: None,
                    prefix: None,
                },
            );
            builder.interfaces.insert(
                vlan_if_id,
                Interface {
                    name: string.into_boxed_str(),
                    device: d1,
                    vlan: Some(vlan_id),
                    ips: Box::new([ip_id]),
                    bridge: Some(bridge_id),
                    ..Default::default()
                },
            );
        }
        builder.vlan_groups.insert(
            vlan_group,
            VlanGroupData {
                vlans: vlans.clone().into_boxed_slice(),
            },
        );
    } else {
        bridge_of_vlan.entry(None).or_default().extend(bridges);
    }
    builder.devices.insert(
        d1,
        Device {
            name: "d1".into(),
            ..Default::default()
        },
    );

    for (if_idx, ifid) in (0..port_count)
        .map(|idx| (idx + 1, builder.interfaces.next_id()))
        .collect::<Vec<_>>()
        .into_iter()
    {
        let vlan = if vlans.is_empty() {
            None
        } else {
            vlans.get(if_idx % vlans.len()).copied()
        };
        let bridges = bridge_of_vlan.get(&vlan).unwrap();
        let bridge_id = bridges[if_idx % bridges.len()];
        builder.interfaces.insert(
            ifid,
            Interface {
                name: format!("ether{if_idx:2}").into_boxed_str(),
                label: format!("Interface {if_idx}").into_boxed_str(),
                device: d1,
                bridge: Some(bridge_id),
                vlan,
                external: Some(PhysicalPortId::Ethernet(if_idx as u16)),
                ..Default::default()
            },
        );
    }

    Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found")
}

/// Real reference configs (`examples/01-core-router-ospf`, `examples/02-edge-router-bgp-vpn`)
/// name customer/household links like `e01-ap-keller` or `e05-bachmann-dachboden` -
/// a short-name prefix plus a human-entered label, with no separate NetBox device/
/// cable modeled for the far end. `EndpointNameGenerator::generate_interface_name`
/// used to ignore the interface's own `label` entirely and only ever derive a
/// descriptive suffix from cable-topology walking, so an uncabled-but-labeled
/// interface like this produced just the bare short name (`e01`), silently dropping
/// the documented intent.
#[tokio::test]
async fn test_endpoint_name_generator_uses_own_label() {
    use super::{EndpointNameGenerator, NameGenerator};
    let mut builder = TopologyBuilder::default();
    let d1 = builder.devices.next_id();
    let if_id = builder.interfaces.next_id();
    builder.interfaces.insert(
        if_id,
        Interface {
            name: "ether1".into(),
            label: "AP Keller".into(),
            device: d1,
            external: Some(PhysicalPortId::Ethernet(1)),
            ..Default::default()
        },
    );
    builder.devices.insert(
        d1,
        Device {
            name: "router-1".into(),
            ..Default::default()
        },
    );
    let device = Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found");
    let interface = device.interfaces().into_iter().next().unwrap();
    let name = EndpointNameGenerator.generate_interface_name(&interface);
    assert_eq!(name.as_ref(), "e01-ap-keller");
}

/// Name `EndpointNameGenerator` gives an unlabeled `ether2` cabled to `ether1` of a
/// second device named `far_device`.
async fn name_towards(far_device: &str) -> String {
    use super::{EndpointNameGenerator, NameGenerator};
    use crate::topology::{Cable, CablePort};

    let mut builder = TopologyBuilder::default();
    let d1 = builder.devices.next_id();
    let d2 = builder.devices.next_id();
    let near_if = builder.interfaces.next_id();
    let far_if = builder.interfaces.next_id();
    builder.interfaces.insert(
        near_if,
        Interface {
            name: "ether2".into(),
            device: d1,
            external: Some(PhysicalPortId::Ethernet(2)),
            ..Default::default()
        },
    );
    builder.interfaces.insert(
        far_if,
        Interface {
            name: "ether1".into(),
            device: d2,
            external: Some(PhysicalPortId::Ethernet(1)),
            ..Default::default()
        },
    );
    let cable_id = builder.cables.next_id();
    builder.cables.insert(
        cable_id,
        Cable {
            port_a: Box::new([CablePort::Interface(near_if)]),
            port_b: Box::new([CablePort::Interface(far_if)]),
        },
    );
    builder.devices.insert(
        d1,
        Device {
            name: "router-1".into(),
            ..Default::default()
        },
    );
    builder.devices.insert(
        d2,
        Device {
            name: far_device.into(),
            ..Default::default()
        },
    );
    let device = Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found");
    let interface = device
        .interfaces()
        .into_iter()
        .find(|i| i.id() == near_if)
        .unwrap();
    EndpointNameGenerator
        .generate_interface_name(&interface)
        .into_owned()
}

/// A cabled infrastructure link (router-to-router/switch, typically left unlabeled
/// in NetBox since the cable topology already documents it) is named after the far
/// device, even though the far end is a physical port itself (used to give
/// `e02-e01`).
#[tokio::test]
async fn test_endpoint_name_generator_falls_back_to_cable_when_unlabeled() {
    assert_eq!(name_towards("Hegglin").await, "e02-hegglin");
    assert_eq!(name_towards("Büro / Küche").await, "e02-buero-kueche");
}

/// An unnamed far device (e.g. a patch panel without a name) falls back to the far
/// port instead of leaving a dangling `e02-`.
#[tokio::test]
async fn test_endpoint_name_generator_unnamed_far_device() {
    assert_eq!(name_towards("").await, "e02-e01");
}

#[test]
fn name_parts_are_ascii_kebab() {
    for (raw, expected) in [
        ("AP Keller", "ap-keller"),
        ("Büro / Küche E.5.1", "buero-kueche-e-5-1"),
        ("Größe", "groesse"),
        ("hegglin-LHG60G", "hegglin-lhg60g"),
        ("1og-sw2", "1og-sw2"),
        (" / ", ""),
    ] {
        assert_eq!(super::name_part(raw), expected, "{raw}");
    }
}

/// Transfer links on `combo1`/`sfp1`/QSFP+ breakout lanes are ports like any other:
/// the address stays on the (renamed) port. These names used to be unrecognized,
/// which moved the address onto an empty `bridge-combo1` and cut the link.
#[tokio::test]
async fn test_non_ether_ports_keep_their_address() -> Result<(), Box<dyn Error>> {
    for (model, port_name, port, address, expected_name) in [
        (
            b"CCR1009-7G-1C-1S+".as_slice(),
            "combo1",
            PhysicalPortId::Combo(1),
            [172, 17, 1, 13],
            "c01-keller",
        ),
        (
            b"RB962UiGS-5HacT2HnT",
            "sfp1",
            PhysicalPortId::Sfp(1),
            [10, 200, 1, 6],
            "s01-keller",
        ),
        (
            b"CRS354-48G-4S+2Q+",
            "qsfpplus1-2",
            PhysicalPortId::QsfpPlus(1, 2),
            [172, 17, 1, 34],
            "q01-2-keller",
        ),
    ] {
        let mut builder = TopologyBuilder::default();
        let d1 = builder.devices.next_id();
        let ip_id = builder.ip_addresses.next_id();
        let ip = IpNet::new(IpAddr::V4(Ipv4Addr::from(address)), 30).unwrap();
        builder.ip_addresses.insert(
            ip_id,
            IpAddressData {
                ip,
                interface: None,
                prefix: None,
            },
        );
        let if_id = builder.interfaces.next_id();
        builder.interfaces.insert(
            if_id,
            Interface {
                name: port_name.into(),
                label: "Keller".into(),
                device: d1,
                external: port_name.parse().ok(),
                ips: Box::new([ip_id]),
                use_ospf: true,
                ..Default::default()
            },
        );
        builder.devices.insert(
            d1,
            Device {
                name: "router".into(),
                ..Default::default()
            },
        );
        let device = Into::<TopologyHolder>::into(builder)
            .devices_by_id(d1)
            .await
            .expect("device not found");
        assert_eq!(device.interfaces()[0].external_port(), Some(port));

        let (mut target_data, _) = setup_testdata(model)?;
        target_data.generate_from(&device, EndpointNameGenerator)?;
        let expected_name = AsciiString::from(expected_name);
        assert!(target_data.bridge.is_empty(), "{port_name}: no bridge");
        assert_eq!(
            target_data.ethernet[&AsciiString::from(port_name)].name,
            expected_name
        );
        let IpNet::V4(v4) = ip else { unreachable!() };
        assert_eq!(target_data.ipv_4_address[&v4].0.interface, expected_name);
    }
    Ok(())
}
