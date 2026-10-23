use super::*;
use crate::topology::{
    Cable, CablePort, Device, DeviceId, Interface, InterfaceId, IpAddressData, IpPrefixData,
    PhysicalPortId, PortType, PrefixPool, Radio, RadioLink, RadioRole, Topology, VlanData,
    VlanGroupData, VlanGroupId, VlanId, WirelessLinkEnd, builder::TopologyBuilder,
};
use std::{str::FromStr, sync::Arc};

/// r1 and r2 with a direct link (e01), r1 with an uplink (e02), a radio link from r1 e03 over
/// the bridge devices b1 and b2 to r2 e02, a customer bridge on r1 (port e04, VLAN 10 with an
/// address) and a free port e05; r2 e03 has VLAN 10 without an address.
pub(crate) struct Net {
    pub(crate) builder: TopologyBuilder,
    pub(crate) r1: DeviceId,
    pub(crate) r2: DeviceId,
    pub(crate) b1: DeviceId,
    pub(crate) b2: DeviceId,
    pub(crate) link: InterfaceId,
}

impl Net {
    pub(crate) fn new() -> Self {
        let mut builder = TopologyBuilder::default();
        pool(&mut builder, "172.16.1.0/24", PrefixPool::Loopback);
        pool(&mut builder, "172.17.1.0/24", PrefixPool::Transfer);
        let r1 = device(&mut builder, "r1", None, Some("172.16.1.1/32"));
        let r2 = device(&mut builder, "r2", None, Some("172.16.1.2/32"));
        let b1 = device(&mut builder, "b1", Some(DeviceRole::WlanBridge), None);
        let b2 = device(&mut builder, "b2", Some(DeviceRole::WlanBridge), None);

        let link = port(&mut builder, r1, "ether1", Some("172.17.1.0/31"), true);
        let far = port(&mut builder, r2, "ether1", Some("172.17.1.1/31"), true);
        cable(&mut builder, link, far);
        // each port cabled to a router names it, as „Verbindung“ labels them
        builder.interfaces.get_mut(&link).unwrap().label = "r2".into();
        builder.interfaces.get_mut(&far).unwrap().label = "r1".into();

        let uplink = port(&mut builder, r1, "ether2", None, false);
        builder.interfaces.get_mut(&uplink).unwrap().enable_dhcp_client = true;

        let radio_port = port(&mut builder, r1, "ether3", Some("172.17.1.9/29"), true);
        let far_radio_port = port(&mut builder, r2, "ether2", Some("172.17.1.10/29"), true);
        let (b1_ether, b1_radio) = bridge_device(&mut builder, b1, RadioRole::Ap);
        let (b2_ether, b2_radio) = bridge_device(&mut builder, b2, RadioRole::Station);
        cable(&mut builder, radio_port, b1_ether);
        cable(&mut builder, far_radio_port, b2_ether);
        for (port, label) in [(radio_port, "b1"), (far_radio_port, "b2"), (b1_ether, "r1"), (b2_ether, "r2")] {
            builder.interfaces.get_mut(&port).unwrap().label = label.into();
        }
        for (radio, peer) in [(b1_radio, b2_radio), (b2_radio, b1_radio)] {
            let link = WirelessLinkEnd {
                id: 1,
                peer: Some(peer),
                link: RadioLink {
                    ssid: "link-1".into(),
                    encrypted: true,
                    psk: Some("secret".into()),
                    country: None,
                    frequency_mode: None,
                },
            };
            let radio = builder.interfaces.get_mut(&radio).unwrap();
            radio.radio.as_mut().unwrap().link = Some(link.link.clone());
            radio.wireless_link = Some(link);
        }

        let group = VlanGroupId(1);
        builder.vlan_groups.insert(
            group,
            VlanGroupData {
                vlans: Box::new([VlanId(10)]),
            },
        );
        let vlan = VlanId(10);
        // the group's VNI joins r1's and r2's VLAN 10 into one network
        let vxlan = builder.vxlans.next_id();
        builder.vxlans.insert(
            vxlan,
            crate::topology::VxlanData {
                name: "clients".into(),
                vni: 10,
                vlans: Box::new([vlan]),
            },
        );
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
        let customer = virtual_interface(&mut builder, r1, "bridge-clients", PortType::Bridge);
        let address = address(&mut builder, "10.10.0.1/24");
        let data = builder.interfaces.get_mut(&customer).unwrap();
        data.ips = Box::new([address]);
        data.vlan = Some(vlan);
        data.enable_dhcp_server = true;
        let member = port(&mut builder, r1, "ether4", None, false);
        builder.interfaces.get_mut(&member).unwrap().bridge = Some(customer);

        port(&mut builder, r1, "ether5", None, false);
        let elsewhere = port(&mut builder, r2, "ether3", None, false);
        builder.interfaces.get_mut(&elsewhere).unwrap().vlan = Some(vlan);
        Net {
            builder,
            r1,
            r2,
            b1,
            b2,
            link,
        }
    }
    fn view(self, device: DeviceId) -> InterfaceView {
        let topology = Arc::new(self.builder.build());
        topology.get_device_by_id(&device).unwrap().interface_view()
    }
}

fn device(
    builder: &mut TopologyBuilder,
    name: &str,
    role: Option<DeviceRole>,
    loopback: Option<&str>,
) -> DeviceId {
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: true,
            role,
            ..Device::default()
        },
    );
    if let Some(loopback) = loopback {
        let address = address(builder, loopback);
        let lo = virtual_interface(builder, id, "lo", PortType::Loopback);
        let data = builder.interfaces.get_mut(&lo).unwrap();
        data.external = Some(PhysicalPortId::Loopback);
        data.ips = Box::new([address]);
        let device = builder.devices.get_mut(&id).unwrap();
        device.primary_ip_v4 = Some(address);
        device.loopback_ip = Some(address);
    }
    id
}

/// `ether1` and `wlan1` in a bridge with `ospf`, the radio with `role`.
fn bridge_device(
    builder: &mut TopologyBuilder,
    device: DeviceId,
    role: RadioRole,
) -> (InterfaceId, InterfaceId) {
    let bridge = virtual_interface(builder, device, "bridge", PortType::Bridge);
    builder.interfaces.get_mut(&bridge).unwrap().use_ospf = true;
    let ether = port(builder, device, "ether1", None, false);
    let radio = port(builder, device, "wlan1", None, false);
    let data = builder.interfaces.get_mut(&radio).unwrap();
    data.port_type = Some(PortType::Wireless);
    data.radio = Some(Radio {
        role,
        frequency: None,
        channel_width: None,
        tx_power: None,
        link: None,
    });
    for port in [ether, radio] {
        builder.interfaces.get_mut(&port).unwrap().bridge = Some(bridge);
    }
    (ether, radio)
}

fn virtual_interface(
    builder: &mut TopologyBuilder,
    device: DeviceId,
    name: &str,
    port_type: PortType,
) -> InterfaceId {
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            port_type: Some(port_type),
            ..Interface::default()
        },
    );
    id
}

fn port(
    builder: &mut TopologyBuilder,
    device: DeviceId,
    name: &str,
    ip: Option<&str>,
    ospf: bool,
) -> InterfaceId {
    let ips = ip.map(|ip| address(builder, ip)).into_iter().collect();
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
            ips,
            use_ospf: ospf,
            ..Interface::default()
        },
    );
    id
}

fn cable(builder: &mut TopologyBuilder, a: InterfaceId, b: InterfaceId) {
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

pub(crate) fn address(builder: &mut TopologyBuilder, ip: &str) -> crate::topology::IpAddressId {
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
}

fn pool(builder: &mut TopologyBuilder, prefix: &str, pool: PrefixPool) {
    let id = builder.ip_prefixes.next_id();
    builder.ip_prefixes.insert(
        id,
        IpPrefixData {
            prefix: prefix.parse().unwrap(),
            addresses: Box::new([]),
            children: Box::new([]),
            parent: None,
            ranges: Box::new([]),
            pool: Some(pool),
            site: None,
        },
    );
}

fn tasks(view: &InterfaceView) -> Vec<(&str, InterfaceTask)> {
    view.rows
        .iter()
        .map(|row| (&*row.interface.name, row.task))
        .collect()
}

fn row<'v>(view: &'v InterfaceView, name: &str) -> &'v InterfaceRow {
    view.rows
        .iter()
        .find(|row| &*row.interface.name == name)
        .unwrap()
}

#[test]
fn each_interface_has_its_task() {
    let net = Net::new();
    let r1 = net.r1;
    let view = net.view(r1);
    assert_eq!(
        tasks(&view),
        [
            ("bridge-clients", InterfaceTask::Customer),
            ("ether1", InterfaceTask::Direct),
            ("ether2", InterfaceTask::Uplink),
            ("ether3", InterfaceTask::RadioLink),
            ("ether5", InterfaceTask::Unknown),
            ("lo", InterfaceTask::Loopback),
        ]
    );
    let customer = row(&view, "bridge-clients");
    assert_eq!(
        customer.ports.iter().map(|p| &*p.name).collect::<Vec<_>>(),
        ["ether4"]
    );
    assert_eq!(customer.untagged_vlan.as_ref().map(|v| v.vid), Some(10));
    assert!(row(&view, "ether5").free);
    assert!(!row(&view, "ether2").free);
    assert_eq!(
        row(&view, "ether1")
            .far_ends
            .iter()
            .map(|f| (&*f.device.name, &*f.name))
            .collect::<Vec<_>>(),
        [("r2", "ether1")]
    );
}

#[test]
fn a_radio_link_shows_the_way_to_the_other_router() {
    let net = Net::new();
    let r1 = net.r1;
    let path = row(&net.view(r1), "ether3").radio_path.clone().unwrap();
    let name = |i: &Option<InterfaceRef>| {
        i.as_ref()
            .map(|i| format!("{} {}", i.device.name, i.name))
    };
    assert_eq!(name(&path.router_port).as_deref(), Some("r1 ether3"));
    assert_eq!(name(&path.bridge_port).as_deref(), Some("b1 ether1"));
    assert_eq!(name(&path.radio).as_deref(), Some("b1 wlan1"));
    assert_eq!(path.ssid.as_deref(), Some("link-1"));
    assert_eq!(name(&path.peer_radio).as_deref(), Some("b2 wlan1"));
    assert_eq!(name(&path.peer_bridge_port).as_deref(), Some("b2 ether1"));
    assert_eq!(name(&path.peer_router_port).as_deref(), Some("r2 ether2"));
}

#[test]
fn a_bridge_device_shows_the_link_from_its_side() {
    let net = Net::new();
    let b1 = net.b1;
    let view = net.view(b1);
    assert_eq!(tasks(&view), [("bridge", InterfaceTask::RadioLink)]);
    let path = view.rows[0].radio_path.clone().unwrap();
    assert_eq!(path.router_port.map(|p| p.device.name).as_deref(), Some("r1"));
    assert_eq!(
        path.peer_router_port.map(|p| p.device.name).as_deref(),
        Some("r2")
    );
}

#[test]
fn a_customer_port_without_address_names_the_gateway() {
    let net = Net::new();
    let r2 = net.r2;
    let view = net.view(r2);
    let elsewhere = row(&view, "ether3");
    assert_eq!(elsewhere.task, InterfaceTask::Customer);
    assert_eq!(
        elsewhere.gateways.iter().map(|d| &*d.name).collect::<Vec<_>>(),
        ["r1"]
    );
}

#[test]
fn findings_go_to_their_rows() {
    let mut net = Net::new();
    let link = net.link;
    net.builder.interfaces.get_mut(&link).unwrap().mtu = Some(2000);
    let r1 = net.r1;
    let r2 = net.r2;
    // r2 without a primary IPv4: a finding about the device
    net.builder.devices.get_mut(&r2).unwrap().primary_ip_v4 = None;
    let topology = Arc::new(net.builder.build());
    let view = topology.get_device_by_id(&r1).unwrap().interface_view();
    assert!(matches!(
        &row(&view, "ether1").findings[..],
        [Finding { kind: FindingKind::MtuMismatch(_), .. }]
    ));
    assert!(view.device_findings.is_empty(), "{:#?}", view.device_findings);
    let view = topology.get_device_by_id(&r2).unwrap().interface_view();
    assert!(matches!(
        &row(&view, "ether1").findings[..],
        [Finding { kind: FindingKind::MtuMismatch(_), .. }]
    ));
    assert!(matches!(
        &view.device_findings[..],
        [Finding { kind: FindingKind::NoPrimaryIpv4(_), .. }]
    ));
}

/// Every interface of the stored export in one row, as one or as a bridge port; the rows land
/// in `target/netbox-example/interfaces.txt` for review.
#[test]
fn interfaces_of_netbox_example() {
    let topology: Arc<Topology> = crate::topology::test::example_topology();
    let mut devices = topology
        .list_devices()
        .filter(DeviceAccess::has_routeros)
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| a.name().cmp(b.name()));
    let mut out = String::new();
    for device in devices {
        let view = device.interface_view();
        let ports = view.rows.iter().map(|r| 1 + r.ports.len()).sum::<usize>();
        assert_eq!(ports, device.interfaces().len(), "{}", device.name());
        out.push_str(&format!("{}\n", device.name()));
        for row in &view.rows {
            let ports = row.ports.iter().map(|p| &*p.name).collect::<Vec<_>>();
            let addresses = row.addresses.iter().map(|a| &*a.address).collect::<Vec<_>>();
            let far_ends = row
                .far_ends
                .iter()
                .map(|f| format!("{} {}", f.device.name, f.name))
                .collect::<Vec<_>>();
            out.push_str(&format!(
                "  {:?} {} [{}]{} {}{} -> {} findings: {}\n",
                row.task,
                row.interface.name,
                ports.join(", "),
                if row.free { " free" } else { "" },
                addresses.join(" "),
                row.untagged_vlan
                    .as_ref()
                    .map(|v| format!(" vlan {}", v.vid))
                    .unwrap_or_default(),
                far_ends.join(", "),
                row.findings.len()
            ));
        }
    }
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/netbox-example");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/interfaces.txt"), out).unwrap();
}

fn helpers(row: &InterfaceRow) -> Vec<(ActionKind, ActionHelper)> {
    row.actions.iter().map(|a| (a.kind, a.helper)).collect()
}

#[test]
fn rows_offer_the_helpers_that_fit() {
    let mut net = Net::new();
    let link = net.link;
    net.builder.interfaces.get_mut(&link).unwrap().mtu = Some(2000);
    let (r1, r2, b1) = (net.r1, net.r2, net.b1);
    // ether5 of r1 gets a label: no longer free, but still without a task
    let ether5 = net
        .builder
        .interfaces
        .iter()
        .find(|(_, i)| i.device == r1 && &*i.name == "ether5")
        .map(|(id, _)| *id)
        .unwrap();
    net.builder.interfaces.get_mut(&ether5).unwrap().label = "spare".into();
    let topology = Arc::new(net.builder.build());
    let view = topology.get_device_by_id(&r1).unwrap().interface_view();
    let direct = row(&view, "ether1");
    assert_eq!(
        helpers(direct),
        [
            (ActionKind::Suggestion, ActionHelper::Connection),
            (ActionKind::Remove, ActionHelper::Disconnect),
        ]
    );
    assert_eq!(direct.actions[0].interface, Some(link.0));
    assert_eq!(
        direct.actions[0].peer.as_ref().map(|p| (&*p.device.name, &*p.name)),
        Some(("r2", "ether1"))
    );
    assert_eq!(
        helpers(row(&view, "ether5")),
        [
            (ActionKind::AssignRole, ActionHelper::CustomerNet),
            (ActionKind::AssignRole, ActionHelper::Connection),
        ]
    );
    // the router's end of a complete radio link without findings needs nothing
    assert!(row(&view, "ether3").actions.is_empty());
    assert!(row(&view, "ether2").actions.is_empty());
    // a customer bridge with IPv4 only: the helper can bring IPv6
    assert_eq!(
        row(&view, "bridge-clients").actions[0].vlan,
        Some(10)
    );
    assert!(view.loopback_actions.is_empty());

    // r2's VLAN port has its L3 on r1
    let view = topology.get_device_by_id(&r2).unwrap().interface_view();
    assert!(row(&view, "ether3").actions.is_empty());

    // the bridge device: no loopback yet
    let view = topology.get_device_by_id(&b1).unwrap().interface_view();
    assert_eq!(
        view.loopback_actions.iter().map(|a| a.helper).collect::<Vec<_>>(),
        [ActionHelper::Loopback]
    );
}

/// With a finding on the router's end of a radio link, „Funkbrücke“ opens from the router port
/// for the bridge device cabled to it.
#[test]
fn a_radio_link_offers_funkbruecke_from_the_router_port() {
    let mut net = Net::new();
    let r1 = net.r1;
    let ether3 = net
        .builder
        .interfaces
        .iter()
        .find(|(_, i)| i.device == r1 && &*i.name == "ether3")
        .map(|(id, _)| *id)
        .unwrap();
    net.builder.interfaces.get_mut(&ether3).unwrap().mtu = Some(1600);
    let topology = Arc::new(net.builder.build());
    let view = topology.get_device_by_id(&r1).unwrap().interface_view();
    let radio = row(&view, "ether3");
    assert_eq!(helpers(radio), [(ActionKind::Suggestion, ActionHelper::RadioBridge)]);
    assert_eq!(radio.actions[0].interface, Some(ether3.0));
    assert_eq!(
        radio.actions[0].peer.as_ref().map(|p| (&*p.device.name, &*p.name)),
        Some(("b1", "ether1"))
    );
}
