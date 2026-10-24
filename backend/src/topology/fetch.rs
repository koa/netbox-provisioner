use crate::{
    netbox::{
        NetboxError, fetch_topology,
        fetch_topology::{
            CableConnectionTermination, CableConnectionTerminationOnCircuitTerminationType,
            CableConnectionTerminationOnFrontPortType,
            CableConnectionTerminationOnInterfaceType, CableConnectionTerminationOnRearPortType,
            ResponseData,
        },
    },
    topology::{
        Cable, CableId, CablePort, Device, DeviceId, DeviceRole, FrontPort, FrontPortId, Interface,
        InterfaceId, IpAddressData, IpAddressId, IpPrefixData, IpPrefixId, IpRangeData, IpRangeId,
        Duplex, PhysicalPortId, PoeMode, PortType, PrefixPool, Radio, RadioLink, RadioRole, WirelessLinkEnd, RearPort, RearPortId, CircuitTermination, CircuitTerminationId, SiteData, SiteId,
        Topology,
        VlanData, VlanGroupData, VlanGroupId, VlanId, VxlanData, VxlanId, WlanAuth, WlanData,
        WlanGroupData, WlanGroupId, WlanId, WlanOpenSettings, WlanWpaSettings,
    },
};
use ipnet::IpNet;
use log::warn;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};
use tokio::time::Instant;

pub async fn build_topology() -> Result<Topology, NetboxError> {
    let fetch_time = Instant::now();
    let data = fetch_topology().await?;
    Ok(build_topology_from(data, fetch_time))
}

/// Builds the in-memory topology from an already fetched NetBox response - split
/// from `build_topology` so a stored response (`docs/netbox-response-example.json`)
/// can be turned into a topology offline.
pub(crate) fn build_topology_from(data: ResponseData, fetch_time: Instant) -> Topology {
    let source = super::Source(Arc::new(data.clone()));
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
    let mut front_ports = HashMap::new();
    let mut rear_ports = HashMap::new();
    let mut cables = HashMap::new();

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
                    name: wlan_group.name.into_boxed_str(),
                    mgmt_vlan,
                    controller,
                    aps: Box::new([]),
                    wlans: wlans_ids.into_iter().collect(),
                },
            );
        }
    }

    let mut interface_of_address = HashMap::new();
    let mut sites = HashMap::<SiteId, (Box<str>, Vec<DeviceId>)>::new();
    for device in data.device_list {
        if let Some(device_id) = device.id.parse().ok().map(DeviceId) {
            let role = DeviceRole::from_slug(device.role.slug.as_str());
            let site = device.site.id.parse().ok().map(SiteId);
            if let Some(site) = site {
                sites
                    .entry(site)
                    .or_insert_with(|| (device.site.name.as_str().into(), Vec::new()))
                    .1
                    .push(device_id);
            }
            let country = device.site.region.as_ref().map(country_of_region);
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
            let primary_ip_v6 = device
                .primary_ip6
                .and_then(|primary_ip| primary_ip.id.parse().map(IpAddressId).ok());
            let primary_ip_v4 = device
                .primary_ip4
                .and_then(|primary_ip| primary_ip.id.parse().map(IpAddressId).ok());
            let primary_ip = Option::or(primary_ip_v6, primary_ip_v4);
            let mut ports = HashSet::new();
            let mut loopback_ip = None;
            for interface in device.interfaces {
                if interface.name.as_str() == "lo"
                    && interface.type_.as_str() == "virtual"
                    && let Some(ip) = interface
                        .ip_addresses
                        .iter()
                        .filter_map(|address| address.id.parse().map(IpAddressId).ok())
                        .next()
                {
                    loopback_ip = Some(ip);
                }
                if let Some(id) = interface.id.parse().ok().map(InterfaceId) {
                    ports.insert(CablePort::Interface(id));
                    let ips = interface
                        .ip_addresses
                        .into_iter()
                        .filter_map(|address| address.id.parse().map(IpAddressId).ok())
                        .collect::<Box<[_]>>();
                    for ip_id in &ips {
                        interface_of_address.insert(*ip_id, id);
                    }
                    let use_ospf = interface.tags.iter().any(|t| t.slug == "ospf");
                    let enable_dhcp_client = interface.tags.iter().any(|t| t.slug == "dhcp-client");
                    let enable_dhcp_server = interface.tags.iter().any(|t| t.slug == "dhcp");
                    let nat = interface.tags.iter().any(|t| t.slug == "nat");
                    let external = PhysicalPortId::from_str(&interface.name).ok();
                    let sixty_ghz = matches!(interface.type_.as_str(), "ieee802.11ad" | "ieee802.11ay");
                    let port_type = match interface.type_.as_str() {
                        "10gbase-x-sfpp" | "1000base-x-sfp" | "1000base-t" | "100base-tx"
                        | "40gbase-x-qsfpp" => Some(PortType::Ethernet),
                        "ieee802.11a" | "ieee802.11g" | "ieee802.11n" | "ieee802.11ac"
                        | "ieee802.11ad" | "ieee802.11ay" | "ieee802.11ax" => Some(PortType::Wireless),
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
                    let poe = interface.poe_mode.as_deref().and_then(|mode| match mode {
                        "pse" => Some(PoeMode::Pse),
                        "pd" => Some(PoeMode::Pd),
                        _ => None,
                    });
                    let wireless_link = interface.wireless_link.map(|link| {
                        let ends = [&link.interface_a.id, &link.interface_b.id]
                            .map(|end| end.parse().ok().map(InterfaceId));
                        WirelessLinkEnd {
                            id: link.id.parse().unwrap_or_default(),
                            peer: ends.into_iter().flatten().find(|end| *end != id),
                            link: RadioLink {
                                ssid: link.ssid.into_boxed_str(),
                                encrypted: link
                                    .auth_type
                                    .as_deref()
                                    .is_some_and(|auth| !auth.is_empty() && auth != "open"),
                                psk: Some(link.auth_psk)
                                    .filter(|psk| !psk.is_empty())
                                    .map(String::into_boxed_str),
                                country: link
                                    .custom_field_data
                                    .rf_country
                                    .filter(|country| !country.is_empty()),
                                frequency_mode: link
                                    .custom_field_data
                                    .rf_frequency_mode
                                    .filter(|mode| !mode.is_empty()),
                            },
                        }
                    });
                    let radio = interface
                        .rf_role
                        .as_deref()
                        .and_then(|role| match role {
                            "ap" => Some(RadioRole::Ap),
                            "station" => Some(RadioRole::Station),
                            _ => None,
                        })
                        .map(|role| Radio {
                            role,
                            frequency: interface.rf_channel_frequency.map(|f| f.0.round() as u32),
                            channel_width: interface.rf_channel_width.map(|w| w.0.round() as u32),
                            tx_power: interface
                                .tx_power
                                .and_then(|power| i8::try_from(power).ok()),
                            link: wireless_link.as_ref().map(|end| end.link.clone()),
                        });
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
                            enable_dhcp_server,
                            nat,
                            bridge,
                            cable: None,
                            poe,
                            mtu: interface.mtu.and_then(|mtu| u16::try_from(mtu).ok()),
                            speed: interface.speed.and_then(|speed| speed.kbits()),
                            duplex: match interface.duplex.as_deref() {
                                Some("half") => Some(Duplex::Half),
                                Some("full") => Some(Duplex::Full),
                                Some("auto") => Some(Duplex::Auto),
                                _ => None,
                            },
                            radio,
                            wireless_link,
                            sixty_ghz,
                        },
                    );
                    if let Some(vlan_id) = vlan {
                        interfaces_of_vlan.entry(vlan_id).or_default().insert(id);
                    }
                }
            }
            for rear_port in device.rearports {
                if let Ok(port_id) = rear_port.id.parse().map(RearPortId) {
                    rear_ports.insert(
                        port_id,
                        RearPort {
                            name: rear_port.name.into_boxed_str(),
                            device: device_id,
                            front_port: None,
                            cable: None,
                        },
                    );
                    ports.insert(CablePort::RearPort(port_id));
                }
            }
            for front_port in device.frontports {
                if let Ok(port_id) = front_port.id.parse().map(FrontPortId) {
                    // NetBox maps a front port to rear port positions via a list;
                    // the topology model only knows a single rear port per front port.
                    let rear_port = front_port
                        .mappings
                        .first()
                        .and_then(|m| m.rear_port.id.parse().ok())
                        .map(RearPortId);
                    front_ports.insert(
                        port_id,
                        FrontPort {
                            name: front_port.name.into_boxed_str(),
                            device: device_id,
                            rear_port,
                            cable: None,
                        },
                    );
                    if let Some(rear_port_entry) =
                        rear_port.as_ref().and_then(|id| rear_ports.get_mut(id))
                    {
                        rear_port_entry.front_port = Some(port_id);
                    }
                    ports.insert(CablePort::FrontPort(port_id));
                }
            }
            let platform = device.platform.map(|p| p.name).unwrap_or_default();
            let serial = Some(device.serial.into_boxed_str()).filter(|s| !s.is_empty());
            let wlan_controller_of = controllers.get(&device_id).cloned();
            let wlan_ap_of = device.custom_field_data.wlan_group.map(WlanGroupId);
            if let Some(wlan_group) = wlan_ap_of {
                wlan_member_devices
                    .entry(wlan_group)
                    .or_default()
                    .push(device_id);
            }
            devices.insert(
                device_id,
                Device {
                    name: device.name.map(String::into_boxed_str).unwrap_or_default(),
                    site,
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
                    role,
                    country,
                    dns_servers: device.config_context.dns_servers.unwrap_or_default(),
                    propagate_default_route: device
                        .custom_field_data
                        .propagate_default_route
                        .unwrap_or(false),
                },
            );
        }
    }
    for (wlan, devices) in wlan_member_devices {
        if let Some(group) = wlan_groups.get_mut(&wlan) {
            group.aps = devices.into_boxed_slice();
        }
    }
    let mut vlan_groups = HashMap::new();
    let mut vlans = HashMap::new();
    for vlan_group in data.vlan_group_list {
        if let Some(vlan_group_id) = vlan_group.id.parse().ok().map(VlanGroupId) {
            let vxlan = vlan_group.custom_field_data.vxlan_vni.map(|vni| {
                let vxlan_id = VxlanId(vlan_group_id.0);
                (vxlan_id, vni)
            });
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
                            vxlan: vxlan.map(|(vxlan_id, _)| vxlan_id),
                            wlans: Box::from_iter(
                                wlans_of_vlan.remove(&vlan_id).unwrap_or_default(),
                            ),
                        },
                    );
                }
            }
            if let Some((vxlan_id, vni)) = vxlan {
                vxlans.insert(
                    vxlan_id,
                    VxlanData {
                        name: vlan_group.name.clone().into_boxed_str(),
                        vni,
                        vlans: Box::from_iter(vlan_ids.iter().copied()),
                    },
                );
            }
            vlan_groups.insert(
                vlan_group_id,
                VlanGroupData {
                    name: vlan_group.name.into_boxed_str(),
                    vlans: Box::from_iter(vlan_ids),
                },
            );
        }
    }
    let mut circuit_terminations = HashMap::new();
    for circuit in data.circuit_list {
        let ends = circuit
            .terminations
            .iter()
            .filter_map(|end| end.id.parse().ok().map(CircuitTerminationId))
            .collect::<Vec<_>>();
        for (index, id) in ends.iter().enumerate() {
            let peer = match ends.as_slice() {
                [a, z] => Some(if index == 0 { *z } else { *a }),
                _ => None,
            };
            circuit_terminations.insert(
                *id,
                CircuitTermination {
                    circuit: circuit.cid.as_str().into(),
                    peer,
                    cable: None,
                },
            );
        }
    }
    for cable in data.cable_list {
        if let Ok(cable_id) = cable.id.parse().map(CableId) {
            let port_a: Box<[CablePort]> = cable
                .a_terminations
                .into_iter()
                .filter_map(termination_2_cable_port)
                .collect();
            let port_b: Box<[CablePort]> = cable
                .b_terminations
                .into_iter()
                .filter_map(termination_2_cable_port)
                .collect();
            for port in port_a.iter().chain(port_b.iter()) {
                match &port {
                    CablePort::Interface(if_id) => {
                        if let Some(interface) = interfaces.get_mut(if_id) {
                            interface.cable = Some(cable_id);
                        }
                    }
                    CablePort::FrontPort(fp_id) => {
                        if let Some(fp) = front_ports.get_mut(fp_id) {
                            fp.cable = Some(cable_id);
                        }
                    }
                    CablePort::RearPort(rp_id) => {
                        if let Some(rp) = rear_ports.get_mut(rp_id) {
                            rp.cable = Some(cable_id);
                        }
                    }
                    CablePort::CircuitTermination(ct_id) => {
                        if let Some(ct) = circuit_terminations.get_mut(ct_id) {
                            ct.cable = Some(cable_id);
                        }
                    }
                }
            }
            cables.insert(cable_id, Cable { port_a, port_b, kind: cable.type_.filter(|kind| !kind.is_empty()).map(Into::into) });
        }
    }
    let mut ip_prefixes = HashMap::new();
    let mut prefix_idx = HashMap::new();

    for prefix_data in data.prefix_list {
        if let (Ok(id), Ok(prefix)) = (
            prefix_data.id.parse().map(IpPrefixId),
            prefix_data.prefix.parse::<IpNet>(),
        ) {
            prefix_idx.insert(prefix, id);
            let pool = prefix_data.tags.iter().find_map(|tag| match tag.slug.as_str() {
                "loopback" => Some(PrefixPool::Loopback),
                "transfer" => Some(PrefixPool::Transfer),
                _ => None,
            });
            let site = prefix_data.scope.as_ref().and_then(|scope| match scope {
                fetch_topology::FetchTopologyPrefixListScope::SiteType(site) => {
                    site.id.parse().ok().map(SiteId)
                }
                _ => None,
            });
            ip_prefixes.insert(
                id,
                IpPrefixData {
                    prefix,
                    addresses: Box::new([]),
                    children: Box::new([]),
                    parent: None,
                    ranges: Box::new([]),
                    pool,
                    site,
                },
            );
        }
    }
    let mut children_prefix = HashMap::new();
    for (id, prefix_data) in &mut ip_prefixes {
        let mut prefix = prefix_data.prefix;
        while let Some(net_address) = prefix.supernet() {
            if let Some(parent_idx) = prefix_idx.get(&net_address) {
                prefix_data.parent = Some(*parent_idx);
                children_prefix
                    .entry(*parent_idx)
                    .or_insert(Vec::new())
                    .push(*id);
                break;
            }
            prefix = net_address;
        }
    }

    let mut ip_ranges = HashMap::new();
    let mut ip_ranges_idx = HashMap::new();
    let mut ranges_of_prefix = HashMap::new();
    for ip_range in data.ip_range_list {
        let is_dhcp = ip_range
            .role
            .map(|role| role.slug.as_str() == "dhcp")
            .unwrap_or(false);
        if let (Ok(id), Ok(start_addr), Ok(end_addr)) = (
            ip_range.id.parse().map(IpRangeId),
            ip_range.start_address.parse::<IpNet>(),
            ip_range.end_address.parse::<IpNet>(),
        ) {
            let start_net = start_addr.trunc();
            let end_net = end_addr.trunc();
            if start_net == end_net {
                let prefix = prefix_idx.get(&start_net).copied();
                if let Some(prefix_id) = prefix {
                    ranges_of_prefix
                        .entry(prefix_id)
                        .or_insert(Vec::new())
                        .push(id);
                }
                ip_ranges.insert(
                    id,
                    IpRangeData {
                        is_dhcp,
                        net: start_net,
                        start: start_addr.addr(),
                        end: end_addr.addr(),
                        prefix,
                    },
                );
                ip_ranges_idx
                    .entry(start_net)
                    .or_insert_with(Vec::new)
                    .push(id);
            } else {
                log::error!(
                    "different ranges: start: {}, end: {} on {id:?}",
                    start_net,
                    end_net
                );
            }
        }
    }

    let mut ip_addresses = HashMap::new();
    let mut ip_addresses_of_prefix = HashMap::new();
    for ip_addr_data in data.ip_address_list {
        if let (Ok(id), Ok(ip)) = (
            ip_addr_data.id.parse().map(IpAddressId),
            ip_addr_data.address.parse::<IpNet>(),
        ) {
            let prefix = prefix_idx.get(&ip.trunc()).copied();
            if let Some(prefix_id) = prefix {
                ip_addresses_of_prefix
                    .entry(prefix_id)
                    .or_insert(Vec::new())
                    .push(id);
            }

            ip_addresses.insert(
                id,
                IpAddressData {
                    ip,
                    interface: interface_of_address.get(&id).copied(),
                    prefix,
                },
            );
        }
    }
    for (id, prefix) in &mut ip_prefixes {
        prefix.children = children_prefix
            .remove(id)
            .map(Vec::into_boxed_slice)
            .unwrap_or_default();
        prefix.ranges = ranges_of_prefix
            .remove(id)
            .map(Vec::into_boxed_slice)
            .unwrap_or_default();
        prefix.addresses = ip_addresses_of_prefix
            .remove(id)
            .map(Vec::into_boxed_slice)
            .unwrap_or_default();
    }

    Topology {
        fetch_time,
        source: Some(source),
        devices,
        sites: sites
            .into_iter()
            .map(|(id, (name, devices))| {
                let devices = devices.into_boxed_slice();
                (id, SiteData { name, devices })
            })
            .collect(),
        interfaces,
        front_ports,
        rear_ports,
        circuit_terminations,
        cables,
        vxlans,
        wlan_groups,
        wlans,
        vlan_groups,
        vlans,
        ip_addresses,
        ip_prefixes,
        ip_ranges,
        ip_range_idx: ip_ranges_idx
            .into_iter()
            .map(|(id, ranges)| (id, ranges.into_boxed_slice()))
            .collect(),
        findings: Box::default(),
    }
    .with_findings()
}

fn termination_2_cable_port(termination: CableConnectionTermination) -> Option<CablePort> {
    match termination {
        CableConnectionTermination::CircuitTerminationType(
            CableConnectionTerminationOnCircuitTerminationType { id },
        ) => id
            .parse()
            .ok()
            .map(CircuitTerminationId)
            .map(CablePort::CircuitTermination),
        CableConnectionTermination::ConsolePortType => None,
        CableConnectionTermination::ConsoleServerPortType => None,
        CableConnectionTermination::FrontPortType(CableConnectionTerminationOnFrontPortType {
            id,
        }) => id.parse().ok().map(FrontPortId).map(CablePort::FrontPort),
        CableConnectionTermination::InterfaceType(CableConnectionTerminationOnInterfaceType {
            id,
        }) => id.parse().ok().map(InterfaceId).map(CablePort::Interface),
        CableConnectionTermination::PowerFeedType => None,
        CableConnectionTermination::PowerOutletType => None,
        CableConnectionTermination::PowerPortType => None,
        CableConnectionTermination::RearPortType(CableConnectionTerminationOnRearPortType {
            id,
        }) => id.parse().ok().map(RearPortId).map(CablePort::RearPort),
    }
}

/// The slug of the topmost region above (or being) `region`, as far as the query
/// reaches (`RegionAncestry`): the country.
fn country_of_region(region: &fetch_topology::RegionAncestry) -> Box<str> {
    let mut slug = &region.slug;
    if let Some(parent) = &region.parent {
        slug = &parent.slug;
        if let Some(parent) = &parent.parent {
            slug = &parent.slug;
            if let Some(parent) = &parent.parent {
                slug = &parent.slug;
                if let Some(parent) = &parent.parent {
                    slug = &parent.slug;
                    if let Some(parent) = &parent.parent {
                        slug = &parent.slug;
                    }
                }
            }
        }
    }
    slug.as_str().into()
}
