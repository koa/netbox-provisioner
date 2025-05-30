use crate::device::ros::l2::KeepNameGenerator;
use crate::device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget};
use crate::topology::PhysicalPortId;
use crate::{
    device::ros::l2::L2Setup,
    topology::{
        access::DeviceAccess, test::TopologyBuilder, Device, Interface, TopologyHolder, VlanData,
        VlanGroupData,
    },
};
use ipnet::IpNet;
use mikrotik_model::generator::Generator;
use mikrotik_model::model::{InterfaceEthernetByDefaultName, ReferenceType};
use mikrotik_model::resource::ResourceMutation;
use std::collections::HashMap;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr};

#[tokio::test]
async fn test_l2_one_vlan() {
    let device = create_device_with_ports(1, 1, 3).await;
    let setup = L2Setup::new(device, &mut KeepNameGenerator);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_no_vlan() {
    let device = create_device_with_ports(1, 0, 3).await;
    let setup = L2Setup::new(device, &mut KeepNameGenerator);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_multi_vlan() {
    let device = create_device_with_ports(1, 3, 3).await;
    let setup = L2Setup::new(device, &mut KeepNameGenerator);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_untagged_switch() {
    let device = create_device_with_ports(1, 5, 24).await;
    let setup = L2Setup::new(device, &mut KeepNameGenerator);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_multi_untagged_switch() -> Result<(), Box<dyn std::error::Error>> {
    let device = create_device_with_ports(5, 0, 24).await;
    let setup = L2Setup::new(device, &mut KeepNameGenerator);
    println!("Setup: {:#?}", setup);
    let (mut target_data, empty_current) = setup_testdata(b"CRS326-24G-2S+")?;
    target_data.setup_l2(&setup)?;
    let mutations = target_data.generate_mutations(&empty_current)?;
    let mutations = ResourceMutation::sort_mutations_with_provided_dependencies(
        mutations.as_ref(),
        [
            (ReferenceType::Interface, b"lo".into()),
            (ReferenceType::RoutingTable, b"main".into()),
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
    let mut target_data = BaseDeviceDataTarget::new(model)?;
    let empty_current = BaseDeviceDataCurrent {
        ospf_interface: Box::new([]),
        interface_list: Box::new([]),
        identity: Default::default(),
        bridge: Box::new([]),
        bridge_port: Box::new([]),
        ethernet: target_data
            .ethernet
            .iter()
            .map(|(default_name, e)| InterfaceEthernetByDefaultName {
                default_name: default_name.clone(),
                data: e.clone(),
            })
            .collect(),
        ipv_6_address: Box::new([]),
        ospf_instance: Box::new([]),
        vxlan: Box::new([]),
        ospf_area: Box::new([]),
        vxlan_vteps: Box::new([]),
        interface_list_member: Box::new([]),
        bridge_vlan: Box::new([]),
        ipv_4_address: Box::new([]),
        vlan: Box::new([]),
    };
    Ok((target_data, empty_current))
}

async fn create_device_with_ports(
    bridge_count: u16,
    vlan_count: u16,
    port_count: usize,
) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let d1 = builder.next_device_id();
    let mut bridges = Vec::new();
    for bridge_idx in 0..bridge_count.max(1) {
        let bridge_id = builder.next_interface_id();
        bridges.push(bridge_id);
        let ips: Box<[IpNet]> = if vlan_count > 0 {
            Box::from([])
        } else {
            Box::new([
                IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, bridge_idx as u8, 1)), 24).unwrap(),
            ])
        };
        builder.interfaces.insert(
            bridge_id,
            Interface {
                name: "bridge".into(),
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
        let vlan_group = builder.next_vlan_group_id();
        for vid in 0..vlan_count {
            let vlan_id = builder.next_vlan_id();
            builder.vlans.insert(
                vlan_id,
                VlanData {
                    name: format!("vlan-{}", vid + 1).into_boxed_str(),
                    vlan_id: vid + 1,
                    group: vlan_group,
                    terminations: Box::new([]),
                    vxlan: None,
                    wlans: Box::new([]),
                },
            );
            vlans.push(vlan_id);
            let vlan_if_id = builder.next_interface_id();
            vlan_ports.push(vlan_if_id);
            let bridge_id = bridges[vid as usize % bridges.len()];
            bridge_of_vlan
                .entry(Some(vlan_id))
                .or_default()
                .push(bridge_id);
            builder.interfaces.insert(
                vlan_if_id,
                Interface {
                    name: format!("vlan-{}", vid + 1).into_boxed_str(),
                    device: d1,
                    vlan: Some(vlan_id),
                    ips: Box::new([IpNet::new(
                        IpAddr::V4(Ipv4Addr::new(192, 168, (vid + 1) as u8, 1)),
                        24,
                    )
                    .unwrap()]),
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
        .map(|idx| (idx + 1, builder.next_interface_id()))
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
                name: format!("ether{if_idx}").into_boxed_str(),
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
