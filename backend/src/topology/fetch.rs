use crate::{
    netbox::{
        NetboxError, fetch_topology,
        fetch_topology::FetchTopologyL2vpnListTerminationsAssignedObject,
    },
    topology::{
        CablePort, Device, DeviceId, Interface, InterfaceId, PhysicalPortId, PortType, Topology,
        VlanData, VlanGroupData, VlanGroupId, VlanId, VxlanData, VxlanId, WlanAuth, WlanData,
        WlanGroupData, WlanGroupId, WlanId, WlanOpenSettings, WlanWpaSettings,
        fetch::fetch_topology::CableConnectionTermination,
    },
};
use ipnet::IpNet;
use log::warn;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};
use tokio::time::Instant;

#[derive(Debug, Default)]
struct CableChain {
    left_ports: HashSet<CablePort>,
    right_ports: HashSet<CablePort>,
    cables: Vec<CablePathEntry>,
}

impl CableChain {
    fn try_merge(
        &mut self,
        other: CableChain,
        internal_connections: &HashMap<CablePort, HashSet<CablePort>>,
    ) -> CableChainMergeResult {
        if self
            .left_ports
            .iter()
            .flat_map(|p| internal_connections.get(p).into_iter().flatten())
            .any(|p| other.left_ports.contains(p))
        {
            self.append_left(other.swap(), internal_connections);
            CableChainMergeResult::Merged
        } else if self
            .left_ports
            .iter()
            .flat_map(|p| internal_connections.get(p).into_iter().flatten())
            .any(|p| other.right_ports.contains(p))
        {
            self.append_left(other, internal_connections);
            CableChainMergeResult::Merged
        } else if self
            .right_ports
            .iter()
            .flat_map(|p| internal_connections.get(p).into_iter().flatten())
            .any(|p| other.left_ports.contains(p))
        {
            self.append_right(other, internal_connections);
            CableChainMergeResult::Merged
        } else if self
            .right_ports
            .iter()
            .flat_map(|p| internal_connections.get(p).into_iter().flatten())
            .any(|p| other.right_ports.contains(p))
        {
            self.append_right(other.swap(), internal_connections);
            CableChainMergeResult::Merged
        } else {
            CableChainMergeResult::NotMerged(other)
        }
    }
    fn append_left(
        &mut self,
        other: CableChain,
        internal_connections: &HashMap<CablePort, HashSet<CablePort>>,
    ) {
        for replace_port in other
            .right_ports
            .iter()
            .flat_map(|p| internal_connections.get(p).into_iter().flatten())
        {
            self.left_ports.remove(replace_port);
        }
        for new_port in other.left_ports {
            self.left_ports.insert(new_port);
        }
        self.cables.extend(other.cables.into_iter());
    }
    fn append_right(
        &mut self,
        other: CableChain,
        internal_connections: &HashMap<CablePort, HashSet<CablePort>>,
    ) {
        for replace_port in other
            .left_ports
            .iter()
            .flat_map(|p| internal_connections.get(p).into_iter().flatten())
        {
            self.right_ports.remove(replace_port);
        }
        for new_port in other.right_ports {
            self.right_ports.insert(new_port);
        }
        self.cables.extend(other.cables.into_iter());
    }
    fn swap(self) -> Self {
        Self {
            left_ports: self.right_ports,
            right_ports: self.left_ports,
            cables: self
                .cables
                .into_iter()
                .map(|c| CablePathEntry::swap(c))
                .collect(),
        }
    }
}
enum CableChainMergeResult {
    Merged,
    NotMerged(CableChain),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CablePathEntry {
    id: u32,
    left_port: CablePort,
    right_port: CablePort,
}
impl CablePathEntry {
    fn swap(self) -> Self {
        Self {
            id: self.id,
            left_port: self.right_port,
            right_port: self.left_port,
        }
    }
}
struct InternalDeviceConnection {
    left: CablePort,
    right: CablePort,
}

impl InternalDeviceConnection {
    fn new(p0: CablePort, p1: CablePort) -> InternalDeviceConnection {
        if p0 < p1 {
            Self {
                left: p0,
                right: p1,
            }
        } else {
            Self {
                left: p1,
                right: p0,
            }
        }
    }
}

pub async fn build_topology() -> Result<Topology, NetboxError> {
    let fetch_time = Instant::now();
    let data = fetch_topology().await?;
    let mut internal_connections = HashMap::<_, HashSet<_>>::new();
    let mut cable_chains = Vec::<CableChain>::new();
    for (cable, internal_connections_of_cable) in data.cable_list.into_iter().filter_map(|cable| {
        let id = cable.id.parse().ok();
        if let Some(id) = id {
            let mut internal_connections = Vec::with_capacity(2);
            let mut left_ports = Vec::with_capacity(1);
            let mut right_ports = Vec::with_capacity(1);
            for (port, other_ports) in cable
                .a_terminations
                .into_iter()
                .filter_map(|termination| parse_termination(termination))
            {
                for other_port in other_ports {
                    internal_connections.push(InternalDeviceConnection::new(port, other_port));
                }
                left_ports.push(port);
            }
            for (port, other_ports) in cable
                .b_terminations
                .into_iter()
                .filter_map(|termination| parse_termination(termination))
            {
                for other_port in other_ports {
                    internal_connections.push(InternalDeviceConnection::new(port, other_port));
                }
                right_ports.push(port);
            }
            Some((
                left_ports
                    .iter()
                    .flat_map(|left| {
                        right_ports.iter().map(|right| CablePathEntry {
                            id,
                            left_port: *left,
                            right_port: *right,
                        })
                    })
                    .collect::<Box<[_]>>(),
                internal_connections,
            ))
        } else {
            None
        }
    }) {
        for ic in internal_connections_of_cable {
            internal_connections
                .entry(ic.left)
                .or_default()
                .insert(ic.right);
            internal_connections
                .entry(ic.right)
                .or_default()
                .insert(ic.left);
        }
        for cable_path in cable {
            let mut current_fragment = CableChain {
                left_ports: HashSet::from([cable_path.right_port]),
                right_ports: HashSet::from([cable_path.left_port]),
                cables: vec![cable_path],
            };
            let mut merged = true;
            while merged {
                merged = false;
                let mut new_chains = Vec::with_capacity(cable_chains.len() + 1);
                for mut existing_chain in cable_chains {
                    match existing_chain.try_merge(current_fragment, &internal_connections) {
                        CableChainMergeResult::Merged => {
                            merged = true;
                            current_fragment = existing_chain;
                        }
                        CableChainMergeResult::NotMerged(f) => {
                            new_chains.push(existing_chain);
                            current_fragment = f;
                        }
                    }
                }
                cable_chains = new_chains;
            }
            cable_chains.push(current_fragment);
        }
    }
    let mut cable_path_endpoints: HashMap<CablePort, HashSet<CablePort>> = HashMap::new();
    for chain in cable_chains.iter() {
        for left_port in chain.left_ports.iter() {
            cable_path_endpoints
                .entry(*left_port)
                .or_default()
                .extend(chain.right_ports.clone());
        }
        for right_port in chain.right_ports.iter() {
            cable_path_endpoints
                .entry(*right_port)
                .or_default()
                .extend(chain.left_ports.clone());
        }
        /*if chain.cables.len() > 1 {
            let cables = chain.cables.iter().map(|c| c.id).collect::<Box<[_]>>();
            info!("Cable chain: {cables:?}");
        }*/
    }
    let mut credentials_by_tenants = HashMap::<&str, &str>::new();
    for tenant in &data.tenant_list {
        if let Some(cred) = tenant.custom_field_data.mikrotik_credentials.as_ref() {
            credentials_by_tenants.insert(tenant.id.as_ref(), cred);
        }
    }

    let mut interfaces = HashMap::new();
    let mut devices = HashMap::new();
    let mut vxlans = HashMap::new();
    let mut wlan_groups = HashMap::new();
    let mut controllers = HashMap::new();
    let mut wlan_member_devices = HashMap::<_, Vec<_>>::new();
    let mut wlans = HashMap::new();
    let mut wlans_of_vlan = HashMap::<_, BTreeSet<_>>::new();
    let mut interfaces_of_vlan = HashMap::<_, BTreeSet<_>>::new();

    for wlan_group in data.wireless_lan_group_list {
        if let (Some(wlan_group_id), Some(controller)) = (
            wlan_group.id.parse().ok().map(WlanGroupId),
            wlan_group.custom_fields.controller.map(DeviceId),
        ) {
            controllers.insert(controller, wlan_group_id);
            let mgmt_vlan = wlan_group.custom_fields.wlan_mgmt.map(VlanId);
            let mut wlans_ids = BTreeSet::new();
            for wlan in wlan_group.wireless_lans {
                let vlan = wlan.vlan.and_then(|vlan| vlan.id.parse().ok()).map(VlanId);
                if let (Some(id), Some(wlan_auth)) = (
                    wlan.id.parse().ok().map(WlanId),
                    match wlan.auth_type.as_deref() {
                        Some("wpa-personal") => Some(WlanAuth::Wpa(WlanWpaSettings {
                            key: wlan.auth_psk.into_boxed_str(),
                        })),
                        Some("open") => Some(WlanAuth::Open(WlanOpenSettings { use_owe: true })),
                        _ => None,
                    },
                ) {
                    wlans_ids.insert(id);
                    wlans.insert(
                        id,
                        WlanData {
                            ssid: wlan.ssid.into_boxed_str(),
                            vlan,
                            wlan_auth,
                            wlan_group: wlan_group_id,
                        },
                    );
                    if let Some(vlan) = vlan {
                        wlans_of_vlan.entry(vlan).or_default().insert(id);
                    }
                }
            }
            wlan_groups.insert(
                wlan_group_id,
                WlanGroupData {
                    mgmt_vlan,
                    controller,
                    aps: Box::new([]),
                    wlans: wlans_ids.into_iter().collect(),
                },
            );
        }
    }

    for device in data.device_list {
        if let Some(device_id) = device.id.parse().ok().map(DeviceId) {
            let credentials = device
                .tenant
                .and_then(|tenant| credentials_by_tenants.get(tenant.id.as_str()))
                .or_else(|| {
                    device
                        .location
                        .and_then(|location| location.tenant)
                        .and_then(|tenant| credentials_by_tenants.get(tenant.id.as_str()))
                })
                .or_else(|| {
                    device
                        .site
                        .tenant
                        .and_then(|tenant| credentials_by_tenants.get(tenant.id.as_str()))
                })
                .copied()
                .map(Box::<str>::from);
            let primary_ip_v6 = device.primary_ip6.and_then(|primary_ip| {
                primary_ip
                    .address
                    .split_once('/')
                    .and_then(|(address, _)| Ipv6Addr::from_str(address).ok())
            });
            let primary_ip_v4 = device.primary_ip4.and_then(|primary_ip| {
                primary_ip
                    .address
                    .split_once('/')
                    .and_then(|(address, _)| Ipv4Addr::from_str(address).ok())
            });
            let primary_ip =
                Option::or(primary_ip_v6.map(IpAddr::V6), primary_ip_v4.map(IpAddr::V4));
            let mut ports = HashSet::new();
            let mut loopback_ip = None;
            for interface in device.interfaces {
                if interface.name.as_str() == "lo" && interface.type_.as_str() == "virtual" {
                    if let Some(ip) = interface
                        .ip_addresses
                        .iter()
                        .filter_map(|address| IpNet::from_str(&address.address).ok())
                        .filter(|ip| ip.max_prefix_len() == ip.prefix_len())
                        .map(|ip| ip.addr())
                        .next()
                    {
                        loopback_ip = Some(ip);
                    }
                };
                if let Some(id) = interface.id.parse().ok().map(InterfaceId) {
                    ports.insert(CablePort::Interface(id));
                    let ips = interface
                        .ip_addresses
                        .into_iter()
                        .filter_map(|address| IpNet::from_str(&address.address).ok())
                        .collect();
                    let use_ospf = interface.tags.iter().any(|t| t.slug == "ospf");
                    let enable_dhcp_client = interface.tags.iter().any(|t| t.slug == "dhcp-client");
                    let external = PhysicalPortId::from_str(&interface.name).ok();
                    let port_type = match interface.type_.as_str() {
                        "10gbase-x-sfpp" | "1000base-x-sfp" | "1000base-t" | "100base-tx"
                        | "40gbase-x-qsfpp" => Some(PortType::Ethernet),
                        "ieee802.11n" | "ieee802.11ac" | "ieee802.11ad" => Some(PortType::Wireless),
                        "bridge" => Some(PortType::Bridge),
                        "virtual" => Some(PortType::Loopback),
                        &_ => {
                            warn!("Unknown interface type: {}", interface.type_);
                            None
                        }
                    };
                    let vlan = interface
                        .untagged_vlan
                        .and_then(|vlan| vlan.id.parse().ok().map(VlanId));
                    let tagged_vlans = interface
                        .tagged_vlans
                        .into_iter()
                        .filter_map(|vlan| vlan.id.parse().ok().map(VlanId))
                        .collect();
                    let bridge = interface
                        .bridge
                        .and_then(|b| b.id.parse().ok())
                        .map(InterfaceId);
                    interfaces.insert(
                        id,
                        Interface {
                            name: interface.name.into_boxed_str(),
                            label: interface.label.into_boxed_str(),
                            device: device_id,
                            external,
                            port_type,
                            vlan,
                            tagged_vlans,
                            ips,
                            use_ospf,
                            enable_dhcp_client,
                            bridge,
                        },
                    );
                    if let Some(vlan_id) = vlan {
                        interfaces_of_vlan.entry(vlan_id).or_default().insert(id);
                    }
                }
            }
            let platform = device.platform.map(|p| p.name).unwrap_or_default();
            let serial = Some(device.serial.into_boxed_str()).filter(|s| !s.is_empty());
            let wlan_controller_of = controllers.get(&device_id).cloned();
            let wlan_ap_of = device.custom_field_data.wlan_group.map(WlanGroupId);
            let mut vlans = HashSet::new();
            if let Some(wlan_group) = wlan_ap_of {
                wlan_member_devices
                    .entry(wlan_group)
                    .or_default()
                    .push(device_id);
                if let Some(mgmt_vlan) = wlan_groups.get(&wlan_group).and_then(|d| d.mgmt_vlan) {
                    vlans.insert(mgmt_vlan);
                }
            }
            devices.insert(
                device_id,
                Device {
                    name: device.name.map(String::into_boxed_str).unwrap_or_default(),
                    ports,
                    primary_ip,
                    primary_ip_v4,
                    primary_ip_v6,
                    loopback_ip,
                    credentials,
                    has_routeros: platform == "routeros",
                    serial,
                    wlan_controller_of,
                    wlan_ap_of,
                    vlans: vlans.into_iter().collect(),
                },
            );
        }
    }
    for (wlan, devices) in wlan_member_devices {
        if let Some(group) = wlan_groups.get_mut(&wlan) {
            group.aps = devices.into_boxed_slice();
        }
    }
    let mut vxlan_of_vlan = HashMap::new();
    for l2vpn_entry in data.l2vpn_list {
        if l2vpn_entry.type_.as_str() == "vxlan" {
            if let (Some(vxlan_id), Some(vni)) = (
                l2vpn_entry.id.parse().ok().map(VxlanId),
                l2vpn_entry.identifier.map(|id| id as u32),
            ) {
                let mut interface_terminations = BTreeSet::new();
                let mut vlan_terminations = BTreeSet::new();
                for termination in l2vpn_entry.terminations.into_iter() {
                    match termination.assigned_object {
                        FetchTopologyL2vpnListTerminationsAssignedObject::InterfaceType(
                            if_type,
                        ) => {
                            if let Some(if_id) = if_type.id.parse().ok().map(InterfaceId) {
                                interface_terminations.insert(if_id);
                            }
                        }
                        FetchTopologyL2vpnListTerminationsAssignedObject::VLANType(vlan_type) => {
                            if let Some(vlan_id) = vlan_type.id.parse().ok().map(VlanId) {
                                vlan_terminations.insert(vlan_id);
                                vxlan_of_vlan.insert(vlan_id, vxlan_id);
                            }
                        }
                        FetchTopologyL2vpnListTerminationsAssignedObject::VMInterfaceType => {}
                    }
                }
                vxlans.insert(
                    vxlan_id,
                    VxlanData {
                        name: l2vpn_entry.name.into_boxed_str(),
                        vni,
                        interface_terminations: Box::from_iter(interface_terminations),
                        vlan_terminations: Box::from_iter(vlan_terminations),
                    },
                );
            }
        }
    }
    let mut vlan_groups = HashMap::new();
    let mut vlans = HashMap::new();
    for vlan_group in data.vlan_group_list {
        if let Some(vlan_group_id) = vlan_group.id.parse().ok().map(VlanGroupId) {
            let mut vlan_ids = BTreeSet::new();
            for vlan in vlan_group.vlans {
                if let Some(vlan_id) = vlan.id.parse().ok().map(VlanId) {
                    vlan_ids.insert(vlan_id);
                    vlans.insert(
                        vlan_id,
                        VlanData {
                            name: vlan.name.into_boxed_str(),
                            vlan_id: vlan.vid as u16,
                            group: vlan_group_id,
                            terminations: Box::from_iter(
                                interfaces_of_vlan.remove(&vlan_id).unwrap_or_default(),
                            ),
                            vxlan: vxlan_of_vlan.get(&vlan_id).copied(),
                            wlans: Box::from_iter(
                                wlans_of_vlan.remove(&vlan_id).unwrap_or_default(),
                            ),
                        },
                    );
                }
            }
            vlan_groups.insert(
                vlan_group_id,
                VlanGroupData {
                    vlans: Box::from_iter(vlan_ids),
                },
            );
        }
    }

    Ok(Topology {
        fetch_time,
        devices,
        interfaces,
        cable_path_endpoints,
        vxlans,
        wlan_groups,
        wlans,
        vlan_groups,
        vlans,
    })
}

fn parse_termination(
    termination: CableConnectionTermination,
) -> Option<(CablePort, Box<[CablePort]>)> {
    match termination {
        CableConnectionTermination::FrontPortType(fp) => {
            let port_id = fp.id.parse().ok();
            let rear_id = fp.rear_port.id.parse().ok();
            if let (Some(front_id), Some(rear_id)) = (port_id, rear_id) {
                Some((
                    CablePort::FrontPort(front_id),
                    Box::from([CablePort::RearPort(rear_id)]),
                ))
            } else {
                None
            }
        }
        CableConnectionTermination::RearPortType(rp) => {
            let rear_port_id = rp.id.parse().ok().map(CablePort::RearPort);
            if let Some(rear_port_id) = rear_port_id {
                let front_ports = rp
                    .frontports
                    .into_iter()
                    .filter_map(|p| p.id.parse().ok().map(CablePort::FrontPort))
                    .collect();
                Some((rear_port_id, front_ports))
            } else {
                None
            }
        }
        CableConnectionTermination::InterfaceType(ip) => ip
            .id
            .parse()
            .ok()
            .map(|id| (CablePort::Interface(InterfaceId(id)), Box::default())),
        CableConnectionTermination::CircuitTerminationType => None,
        CableConnectionTermination::ConsolePortType => None,
        CableConnectionTermination::ConsoleServerPortType => None,
        CableConnectionTermination::PowerFeedType => None,
        CableConnectionTermination::PowerOutletType => None,
        CableConnectionTermination::PowerPortType => None,
    }
}
