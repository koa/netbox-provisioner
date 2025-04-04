use crate::netbox::fetch_topology::FetchTopologyL2vpnListTerminationsAssignedObject;
use crate::topology::{
    VxlanData, VxlanId, WlanAuth, WlanData, WlanGroupData, WlanGroupId, WlanOpenSettings,
    WlanWpaSettings,
};
use crate::{
    netbox::{NetboxError, fetch_topology},
    topology::{
        CablePort, Device, DeviceId, Interface, InterfaceId, PhysicalPortId, Topology,
        fetch::fetch_topology::CableConnectionTermination,
    },
};
use ipnet::IpNet;
use log::info;
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

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
        if chain.cables.len() > 1 {
            let cables = chain.cables.iter().map(|c| c.id).collect::<Box<[_]>>();
            info!("Cable chain: {cables:?}");
        }
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

    for wlan_group in data.wireless_lan_group_list {
        if let (Some(id), Some(controller)) = (
            wlan_group.id.parse().ok().map(WlanGroupId),
            wlan_group.custom_fields.controller.map(DeviceId),
        ) {
            controllers.insert(controller, id);
            let transport_vxlan = wlan_group.custom_fields.wlan_group.map(VxlanId);
            let wlans = wlan_group
                .wireless_lans
                .into_iter()
                .filter_map(|wlan| {
                    if let (Some(wlan_auth), Some(vlan)) = (
                        match wlan.auth_type.as_deref() {
                            Some("wpa-personal") => Some(WlanAuth::Wpa(WlanWpaSettings {
                                key: wlan.auth_psk.into_boxed_str(),
                            })),
                            Some("open") => {
                                Some(WlanAuth::Open(WlanOpenSettings { use_owe: true }))
                            }
                            _ => None,
                        },
                        wlan.vlan.map(|vlan| vlan.vid as u16),
                    ) {
                        Some(WlanData {
                            ssid: wlan.ssid.into_boxed_str(),
                            vlan,
                            wlan_auth,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            wlan_groups.insert(
                id,
                WlanGroupData {
                    transport_vxlan,
                    controller,
                    aps: Box::new([]),
                    wlans,
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
            let primary_ip = device
                .primary_ip6
                .and_then(|primary_ip| {
                    primary_ip
                        .address
                        .split_once('/')
                        .and_then(|(address, _)| Ipv6Addr::from_str(address).ok())
                        .map(IpAddr::V6)
                })
                .or_else(|| {
                    device.primary_ip4.and_then(|primary_ip| {
                        primary_ip
                            .address
                            .split_once('/')
                            .and_then(|(address, _)| Ipv4Addr::from_str(address).ok())
                            .map(IpAddr::V4)
                    })
                });
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
                    let external = PhysicalPortId::from_str(&interface.name).ok();
                    interfaces.insert(
                        id,
                        Interface {
                            name: interface.name.into_boxed_str(),
                            device: device_id,
                            external,
                            ips,
                        },
                    );
                }
            }
            let platform = device.platform.map(|p| p.name).unwrap_or_default();
            let serial = Some(device.serial.into_boxed_str()).filter(|s| !s.is_empty());
            let wlan_controller_of = controllers.get(&device_id).cloned();
            let wlan_ap_of = device.custom_field_data.wlan_group.map(WlanGroupId);
            let wlan_vxlan = if let Some(wlan_group) = wlan_ap_of {
                wlan_member_devices
                    .entry(wlan_group)
                    .or_default()
                    .push(device_id);
                wlan_groups.get(&wlan_group).and_then(|d| d.transport_vxlan)
            } else {
                None
            };
            devices.insert(
                device_id,
                Device {
                    name: device.name.map(String::into_boxed_str).unwrap_or_default(),
                    ports,
                    primary_ip,
                    loopback_ip,
                    credentials,
                    has_routeros: platform == "routeros",
                    serial,
                    wlan_controller_of,
                    wlan_ap_of,
                    wlan_vxlan,
                },
            );
        }
    }
    for (wlan, devices) in wlan_member_devices {
        if let Some(group) = wlan_groups.get_mut(&wlan) {
            group.aps = devices.into_boxed_slice();
        }
    }
    for l2vpn_entry in data.l2vpn_list {
        if l2vpn_entry.type_.as_str() == "vxlan" {
            if let (Some(vxlan_id), Some(vni)) = (
                l2vpn_entry.id.parse().ok().map(VxlanId),
                l2vpn_entry.identifier.map(|id| id as u32),
            ) {
                let terminations = l2vpn_entry
                    .terminations
                    .into_iter()
                    .filter_map(|t| match t.assigned_object {
                        FetchTopologyL2vpnListTerminationsAssignedObject::InterfaceType(
                            if_type,
                        ) => if_type.id.parse().ok().map(InterfaceId),
                        FetchTopologyL2vpnListTerminationsAssignedObject::VLANType => None,
                        FetchTopologyL2vpnListTerminationsAssignedObject::VMInterfaceType => None,
                    })
                    .collect();
                let wlan_group = wlan_groups
                    .iter()
                    .find(|(_, e)| e.transport_vxlan == Some(vxlan_id))
                    .map(|(id, _)| *id);
                vxlans.insert(
                    vxlan_id,
                    VxlanData {
                        name: l2vpn_entry.name.into_boxed_str(),
                        vni,
                        terminations,
                        wlan_group,
                    },
                );
            }
        }
    }

    Ok(Topology {
        devices,
        interfaces,
        cable_path_endpoints,
        vxlans,
        wlan_groups,
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
