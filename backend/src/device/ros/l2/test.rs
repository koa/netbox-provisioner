use crate::{
    device::ros::l2::L2Setup,
    topology::{
        CablePort, Device, Interface, TopologyHolder, VlanData, VlanGroupData,
        access::DeviceAccess, test::TopologyBuilder,
    },
};
use ipnet::IpNet;
use std::net::{IpAddr, Ipv4Addr};

#[tokio::test]
async fn test_l2_one_vlan() {
    let device = create_device_with_ports(1, 3).await;
    let setup = L2Setup::new(device);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_no_vlan() {
    let device = create_device_with_ports(0, 3).await;
    let setup = L2Setup::new(device);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_multi_vlan() {
    let device = create_device_with_ports(3, 3).await;
    let setup = L2Setup::new(device);
    println!("Setup: {:#?}", setup);
}
#[tokio::test]
async fn test_l2_untagged_switch() {
    let device = create_device_with_ports(5, 24).await;
    let setup = L2Setup::new(device);
    println!("Setup: {:#?}", setup);
}
async fn create_device_with_ports(vlan_count: u16, port_count: usize) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let d1 = builder.next_device_id();
    let bridge_id = builder.next_interface_id();
    builder.interfaces.insert(
        bridge_id,
        Interface {
            name: "bridge".into(),
            label: "Bridge ".into(),
            device: d1,
            ips: Box::new([IpNet::new(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1)), 24).unwrap()]),
            ..Default::default()
        },
    );
    let ports = (0..port_count)
        .map(|_| builder.next_interface_id())
        .collect::<Vec<_>>();
    builder.devices.insert(
        d1,
        Device {
            name: "d1".into(),
            ports: ports
                .iter()
                .copied()
                .chain(Some(bridge_id))
                .map(CablePort::Interface)
                .collect(),
            ..Default::default()
        },
    );
    let mut vlans = Vec::new();
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
        }
        builder.vlan_groups.insert(
            vlan_group,
            VlanGroupData {
                vlans: vlans.clone().into_boxed_slice(),
            },
        );
    }
    for (if_idx, ifid) in ports.into_iter().enumerate() {
        let vlan = if vlans.is_empty() {
            None
        } else {
            vlans.get(if_idx % vlans.len()).copied()
        };
        builder.interfaces.insert(
            ifid,
            Interface {
                name: format!("if{}", ifid.0).into_boxed_str(),
                label: format!("Interface {}", ifid.0).into_boxed_str(),
                device: d1,
                bridge: Some(bridge_id),
                vlan,
                ..Default::default()
            },
        );
    }

    Into::<TopologyHolder>::into(builder)
        .devices_by_id(d1)
        .await
        .expect("device not found")
}
