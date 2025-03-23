use crate::topology::PhysicalPortId;
use crate::{
    netbox::{NetboxError, fetch_topology},
    topology::{
        CablePort, Device, DeviceId, Interface, InterfaceId, Topology,
        fetch::fetch_topology::CableConnectionTermination,
    },
};
use ipnet::IpNet;
use lazy_static::lazy_static;
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

    for device in data.device_list {
        if let Some(device_id) = device.id.parse().ok().map(|id| DeviceId(id)) {
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
                .map(|id| Box::<str>::from(id));
            let primary_ip = device
                .primary_ip6
                .and_then(|primary_ip| {
                    primary_ip
                        .address
                        .split_once('/')
                        .and_then(|(address, _)| Ipv6Addr::from_str(address).ok())
                        .map(|addr| IpAddr::V6(addr))
                })
                .or_else(|| {
                    device.primary_ip4.and_then(|primary_ip| {
                        primary_ip
                            .address
                            .split_once('/')
                            .and_then(|(address, _)| Ipv4Addr::from_str(address).ok())
                            .map(|addr| IpAddr::V4(addr))
                    })
                });
            let mut ports = HashSet::new();
            for interface in device.interfaces {
                if let Some(id) = interface.id.parse().ok().map(|id| InterfaceId(id)) {
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
            devices.insert(
                device_id,
                Device {
                    name: device
                        .name
                        .map(|name| String::into_boxed_str(name))
                        .unwrap_or_default(),
                    ports,
                    primary_ip,
                    credentials,
                    has_routeros: platform == "routeros",
                },
            );
        }
    }

    Ok(Topology {
        devices,
        interfaces,
        cable_path_endpoints,
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
            let rear_port_id = rp.id.parse().ok().map(|id| CablePort::RearPort(id));
            if let Some(rear_port_id) = rear_port_id {
                let front_ports = rp
                    .frontports
                    .into_iter()
                    .filter_map(|p| p.id.parse().ok().map(|id| CablePort::FrontPort(id)))
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
