use super::*;
use crate::topology::{
    VlanData, VlanGroupData, VlanGroupId, VxlanData, VxlanId, interfaces::test::Net,
};

fn steps(plan: &NetboxPlan) -> Vec<String> {
    plan.steps
        .iter()
        .map(|step| match step {
            PlanStep::UpdatePortVlan(u) => format!(
                "vlan {} {} {:?} -> {:?}, tagged {:?}",
                u.interface.device.name,
                u.interface.name,
                u.vlan_before.as_ref().map(|v| v.vid),
                u.vlan.as_ref().map(|v| v.vid),
                u.tagged_before.iter().map(|v| v.vid).collect::<Vec<_>>(),
            ),
            other => format!("{other:?}"),
        })
        .collect()
}

/// A VLAN `id` with VID `vid` in a group of its own named `group`, with a VNI if `vxlan`.
fn group_with(net: &mut Net, group: u32, name: &str, vlan: u32, vid: u16, vxlan: bool) -> VlanId {
    let (group, id) = (VlanGroupId(group), VlanId(vlan));
    net.builder.vlan_groups.insert(
        group,
        VlanGroupData {
            name: name.into(),
            vlans: Box::new([id]),
        },
    );
    let vxlan = vxlan.then(|| {
        let vxlan = VxlanId(group.0);
        net.builder.vxlans.insert(
            vxlan,
            VxlanData {
                name: name.into(),
                vni: group.0,
                vlans: Box::new([id]),
            },
        );
        vxlan
    });
    net.builder.vlans.insert(
        id,
        VlanData {
            name: name.into(),
            vlan_id: vid,
            group,
            terminations: Box::new([]),
            vxlan,
            wlans: Box::new([]),
        },
    );
    id
}

/// A VLAN `id` with VID `vid` in the group `clients` (VNI 10) of the fixture.
fn in_clients(net: &mut Net, vlan: u32, vid: u16) -> VlanId {
    let id = VlanId(vlan);
    net.builder.vlans.insert(
        id,
        VlanData {
            name: format!("clients-{vid}").into(),
            vlan_id: vid,
            group: VlanGroupId(1),
            terminations: Box::new([]),
            vxlan: net.builder.vlans[&VlanId(10)].vxlan,
            wlans: Box::new([]),
        },
    );
    id
}

/// The guests' VLAN on r1's free port.
#[test]
fn puts_the_vlan_on_a_free_port() {
    let mut net = Net::new();
    let r1 = net.r1;
    let (_, vlan) = net.guests();
    let ether5 = net.interface(r1, "ether5");
    let topology = Arc::new(net.builder.build());
    let input = PortVlanInput {
        device: r1.0,
        port: ether5.0,
        vlan: vlan.0,
    };
    assert_eq!(
        steps(&plan(&topology, &input).expect("plan")),
        ["vlan r1 ether5 None -> Some(20), tagged []"]
    );
}

/// The ports: the free one; not the link, the uplink or the bridge's port. The VLANs by group:
/// the one with a VNI and the device's own local one, not a local one of another device. The
/// group to start with is the one of the device's lowest VLAN.
#[test]
fn offers_free_ports_and_the_vlans_that_reach_them() {
    let mut net = Net::new();
    let (r1, r2) = (net.r1, net.r2);
    net.guests();
    let local = group_with(&mut net, 3, "local", 30, 30, false);
    let r2_port = net.interface(r2, "ether3");
    net.builder.interfaces.get_mut(&r2_port).unwrap().vlan = Some(local);
    let topology = Arc::new(net.builder.build());
    let options = options(&topology.get_device_by_id(&r1).unwrap());
    assert_eq!(
        options.ports.iter().map(|p| &*p.name).collect::<Vec<_>>(),
        ["ether5"]
    );
    assert_eq!(
        options
            .groups
            .iter()
            .map(|g| (&*g.name, g.vxlan, g.vlans.iter().map(|v| v.vid).collect::<Vec<_>>()))
            .collect::<Vec<_>>(),
        [("clients", true, vec![10]), ("guests", false, vec![20])]
    );
    assert_eq!(options.group, Some(1));
}

/// A VLAN of a WLAN the device has decides the group to start with.
#[test]
fn a_wlan_decides_the_group_to_start_with() {
    let mut net = Net::new();
    let r1 = net.r1;
    let (_, guests) = net.guests();
    let wlan = crate::topology::WlanId(1);
    net.builder.vlans.get_mut(&guests).unwrap().wlans = Box::new([wlan]);
    let topology = Arc::new(net.builder.build());
    let options = options(&topology.get_device_by_id(&r1).unwrap());
    assert_eq!(options.group, Some(2));
}

/// A VLAN with the VID of one the device has is left out, and refused.
#[test]
fn a_vid_the_device_has_is_refused() {
    let mut net = Net::new();
    let r1 = net.r1;
    net.guests();
    // VID 20 again, reachable over its VNI
    let other = group_with(&mut net, 4, "other", 40, 20, true);
    let ether5 = net.interface(r1, "ether5");
    let topology = Arc::new(net.builder.build());
    let options = options(&topology.get_device_by_id(&r1).unwrap());
    assert!(!options.groups.iter().any(|g| &*g.name == "other"), "{options:?}");
    let input = PortVlanInput {
        device: r1.0,
        port: ether5.0,
        vlan: other.0,
    };
    match plan(&topology, &input) {
        Err(Error::VlanIdTaken { vid: 20, vlan, group }) => {
            assert_eq!((&*vlan, &*group), ("guests", "guests"))
        }
        other => panic!("{:?}", other.map(|plan| plan.steps)),
    }
}

/// The VID on the port alone counts as free: the port gets the other VLAN instead.
#[test]
fn the_ports_own_vid_doesnt_count() {
    let mut net = Net::new();
    let r1 = net.r1;
    let local = group_with(&mut net, 3, "local", 30, 30, false);
    let other = group_with(&mut net, 4, "other", 40, 30, true);
    let ether5 = net.interface(r1, "ether5");
    net.builder.interfaces.get_mut(&ether5).unwrap().vlan = Some(local);
    let topology = Arc::new(net.builder.build());
    let input = PortVlanInput {
        device: r1.0,
        port: ether5.0,
        vlan: other.0,
    };
    assert_eq!(
        steps(&plan(&topology, &input).expect("plan")),
        ["vlan r1 ether5 Some(30) -> Some(30), tagged []"]
    );
}

/// The group `clients` is tunneled into r1's modeled bridge `bridge-clients` (VLAN 10): another
/// tunneled VLAN of it on a free port would be in the default bridge, its VXLAN in two. VLAN 10
/// itself goes into `bridge-clients` with the port and is fine.
#[test]
fn a_vxlan_stays_in_its_bridge() {
    let mut net = Net::new();
    let (r1, r2) = (net.r1, net.r2);
    let eleven = in_clients(&mut net, 11, 11);
    // on r2 too: tunneled once r1 has it
    let r2_port = net.interface(r2, "ether3");
    net.builder.interfaces.get_mut(&r2_port).unwrap().tagged_vlans = Box::new([eleven]);
    let ether5 = net.interface(r1, "ether5");
    let topology = Arc::new(net.builder.build());
    let options = options(&topology.get_device_by_id(&r1).unwrap());
    let clients = options.groups.iter().find(|g| &*g.name == "clients").unwrap();
    assert_eq!(clients.vlans.iter().map(|v| v.vid).collect::<Vec<_>>(), [10]);
    let input = |vlan: VlanId| PortVlanInput {
        device: r1.0,
        port: ether5.0,
        vlan: vlan.0,
    };
    match plan(&topology, &input(eleven)) {
        Err(Error::VxlanInBridge { group, bridge }) => {
            assert_eq!((&*group, bridge.as_deref()), ("clients", Some("bridge-clients")))
        }
        other => panic!("{:?}", other.map(|plan| plan.steps)),
    }
    assert!(plan(&topology, &input(VlanId(10))).is_ok());
}

/// A bridge's port, an uplink, a port that has the VLAN already and a VLAN that can't reach
/// the port are refused.
#[test]
fn refuses_what_wouldnt_work() {
    let mut net = Net::new();
    let r1 = net.r1;
    let (_, vlan) = net.guests();
    let ether4 = net.interface(r1, "ether4");
    let ether5 = net.interface(r1, "ether5");
    let uplink = net.interface(r1, "ether2");
    net.builder.interfaces.get_mut(&ether5).unwrap().vlan = Some(vlan);
    let topology = Arc::new(net.builder.build());
    let input = |port: InterfaceId, vlan: u32| PortVlanInput {
        device: r1.0,
        port: port.0,
        vlan,
    };
    assert!(matches!(
        plan(&topology, &input(ether4, vlan.0)),
        Err(Error::PortInBridge { .. })
    ));
    assert!(matches!(
        plan(&topology, &input(uplink, vlan.0)),
        Err(Error::PortUplink(_))
    ));
    assert!(matches!(
        plan(&topology, &input(ether5, vlan.0)),
        Err(Error::PortHasVlan { vlan: 20, .. })
    ));
    assert!(matches!(
        plan(&topology, &input(ether5, 99)),
        Err(Error::UnknownVlan(99))
    ));
}
