use crate::topology::{
    Cable, CableId, CablePort, Device, DeviceId, FrontPort, FrontPortId, Interface, InterfaceId,
    IpAddressData, IpAddressId, IpPrefixData, IpPrefixId, IpRangeData, IpRangeId, RearPort,
    RearPortId, SiteData, SiteId, Topology, TopologyHolder, VlanData, VlanGroupData, VlanGroupId, VlanId, VxlanData,
    VxlanId, WlanData, WlanGroupData, WlanGroupId, WlanId,
};
use std::{
    collections::HashMap,
    hash::Hash,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tokio::{sync::Mutex, time::Instant};

#[derive(Default, Clone)]
pub struct TopologyBuilder {
    pub devices: MapBuilder<DeviceId, Device>,
    pub sites: MapBuilder<SiteId, SiteData>,
    pub interfaces: MapBuilder<InterfaceId, Interface>,
    pub vxlans: MapBuilder<VxlanId, VxlanData>,
    pub wlan_groups: MapBuilder<WlanGroupId, WlanGroupData>,
    pub wlans: MapBuilder<WlanId, WlanData>,
    pub vlan_groups: MapBuilder<VlanGroupId, VlanGroupData>,
    pub vlans: MapBuilder<VlanId, VlanData>,
    pub front_ports: MapBuilder<FrontPortId, FrontPort>,
    pub rear_ports: MapBuilder<RearPortId, RearPort>,
    pub cables: MapBuilder<CableId, Cable>,
    pub ip_addresses: MapBuilder<IpAddressId, IpAddressData>,
    pub ip_prefixes: MapBuilder<IpPrefixId, IpPrefixData>,
    pub ip_ranges: MapBuilder<IpRangeId, IpRangeData>,
}

#[derive(Clone)]
pub struct MapBuilder<ID: From<u32> + Copy + Eq + Hash, Data> {
    values: HashMap<ID, Data>,
    next_id: u32,
}
impl<ID: From<u32> + Copy + Eq + Hash, Data> MapBuilder<ID, Data> {
    pub fn next_id(&mut self) -> ID {
        let id = self.next_id;
        self.next_id += 1;
        id.into()
    }
    pub fn insert(&mut self, id: ID, data: Data) {
        self.values.insert(id, data);
    }
}
impl<ID: From<u32> + Copy + Eq + Hash, Data> Default for MapBuilder<ID, Data> {
    fn default() -> Self {
        MapBuilder {
            values: HashMap::default(),
            next_id: 0,
        }
    }
}
impl<ID: From<u32> + Copy + Eq + Hash, Data> Deref for MapBuilder<ID, Data> {
    type Target = HashMap<ID, Data>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl<ID: From<u32> + Copy + Eq + Hash, Data> DerefMut for MapBuilder<ID, Data> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

impl<ID: From<u32> + Copy + Eq + Hash, Data> From<MapBuilder<ID, Data>> for HashMap<ID, Data> {
    fn from(value: MapBuilder<ID, Data>) -> Self {
        value.values
    }
}

impl TopologyBuilder {
    pub fn build(mut self) -> Topology {
        for (id, interface) in self.interfaces.deref() {
            self.devices
                .get_mut(&interface.device)
                .expect("device not found")
                .ports
                .insert(CablePort::Interface(*id));
            if let Some(vlan_id) = &interface.vlan {
                let mut vlan = self.vlans.remove(vlan_id).expect("vlan not found");
                if !vlan.terminations.contains(id) {
                    vlan.terminations = vlan.terminations.into_iter().chain(Some(*id)).collect()
                }
                self.vlans.insert(*vlan_id, vlan);
            }
            for ip in &interface.ips {
                self.ip_addresses
                    .get_mut(ip)
                    .expect("ip not found")
                    .interface = Some(*id);
            }
        }
        for (id, cable) in self.cables.deref() {
            for cable_port in cable.port_a.iter().chain(cable.port_b.iter()) {
                match cable_port {
                    CablePort::Interface(if_id) => {
                        self.interfaces
                            .get_mut(&if_id)
                            .expect("interface not found")
                            .cable = Some(*id);
                    }
                    CablePort::FrontPort(fp_id) => {
                        self.front_ports
                            .get_mut(fp_id)
                            .expect("front_port not found")
                            .cable = Some(*id);
                    }
                    CablePort::RearPort(rp_id) => {
                        self.rear_ports
                            .get_mut(rp_id)
                            .expect("rear_port not found")
                            .cable = Some(*id);
                    }
                }
            }
        }
        for (id, front_port) in self.front_ports.deref() {
            if let Some(rp_id) = front_port.rear_port {
                self.rear_ports
                    .get_mut(&rp_id)
                    .expect("rear_port not found")
                    .front_port = Some(*id);
            }
        }
        for (id, rear_port) in self.rear_ports.deref() {
            if let Some(fp_id) = rear_port.front_port {
                self.front_ports
                    .get_mut(&fp_id)
                    .expect("front_port not found")
                    .rear_port = Some(*id);
            }
        }

        let mut prefix_idx = HashMap::new();
        for (id, prefix_data) in self.ip_prefixes.deref() {
            prefix_idx.insert(prefix_data.prefix, *id);
        }

        let mut ranges_of_prefix = HashMap::new();

        let mut ip_ranges_idx = HashMap::new();
        for (id, range) in self.ip_ranges.deref_mut() {
            let prefix_id = if let Some(prefix_id) = prefix_idx.get(&range.net) {
                *prefix_id
            } else {
                let prefix_id = self.ip_prefixes.next_id();
                prefix_idx.insert(range.net, prefix_id);
                self.ip_prefixes.insert(
                    prefix_id,
                    IpPrefixData {
                        prefix: range.net,
                        addresses: Box::new([]),
                        children: Box::new([]),
                        parent: None,
                        ranges: Box::new([]),
                    },
                );
                prefix_id
            };
            ranges_of_prefix
                .entry(prefix_id)
                .or_insert(Vec::new())
                .push(*id);
            ip_ranges_idx
                .entry(range.net)
                .or_insert_with(Vec::new)
                .push(*id);
        }
        let mut ips_of_prefixes = HashMap::new();
        for (id, address_data) in self.ip_addresses.deref_mut() {
            let net = address_data.ip.trunc();
            let prefix_id = if let Some(prefix_id) = prefix_idx.get(&net) {
                *prefix_id
            } else {
                let prefix_id = self.ip_prefixes.next_id();
                prefix_idx.insert(net, prefix_id);
                self.ip_prefixes.insert(
                    prefix_id,
                    IpPrefixData {
                        prefix: net,
                        addresses: Box::new([]),
                        children: Box::new([]),
                        parent: None,
                        ranges: Box::new([]),
                    },
                );
                prefix_id
            };
            ips_of_prefixes
                .entry(prefix_id)
                .or_insert(Vec::new())
                .push(*id);
        }

        let mut children_prefix = HashMap::new();
        for (id, prefix_data) in self.ip_prefixes.deref_mut() {
            prefix_data.parent = None;
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
        for (id, prefix_data) in self.ip_prefixes.deref_mut() {
            prefix_data.children = children_prefix
                .remove(id)
                .map(Vec::into_boxed_slice)
                .unwrap_or_default();
            prefix_data.addresses = ips_of_prefixes
                .remove(id)
                .map(Vec::into_boxed_slice)
                .unwrap_or_default();
            prefix_data.ranges = ranges_of_prefix
                .remove(id)
                .map(Vec::into_boxed_slice)
                .unwrap_or_default();
        }

        Topology {
            fetch_time: Instant::now(),
            devices: self.devices.into(),
            sites: self.sites.into(),
            interfaces: self.interfaces.into(),
            front_ports: self.front_ports.into(),
            rear_ports: self.rear_ports.into(),
            cables: self.cables.into(),
            vxlans: self.vxlans.into(),
            wlan_groups: self.wlan_groups.into(),
            wlans: self.wlans.into(),
            vlan_groups: self.vlan_groups.into(),
            vlans: self.vlans.into(),
            ip_addresses: self.ip_addresses.into(),
            ip_prefixes: self.ip_prefixes.into(),
            ip_ranges: self.ip_ranges.into(),
            ip_range_idx: ip_ranges_idx
                .into_iter()
                .map(|(id, ranges)| (id, ranges.into_boxed_slice()))
                .collect(),
        }
    }
}
impl From<TopologyBuilder> for TopologyHolder {
    fn from(value: TopologyBuilder) -> Self {
        TopologyHolder {
            data: Arc::new(Mutex::new(Some(Arc::new(value.build())))),
        }
    }
}

#[cfg(test)]
mod physical_port_names {
    use crate::{device::ros::hw_facts::build_ethernet_ports, topology::PhysicalPortId};
    use std::str::FromStr;

    /// Every ethernet port of every supported board must be recognized from its
    /// RouterOS default name and map back to exactly that name - otherwise a NetBox
    /// interface named like the port silently isn't treated as a port at all and its
    /// addresses end up on an empty bridge.
    #[test]
    fn every_board_port_round_trips() {
        for model in [
            b"RB962UiGS-5HacT2HnT".as_slice(),
            b"RB750Gr3",
            b"CRS326-24G-2S+",
            b"CRS318-16P-2S+",
            b"C52iG-5HaxD2HaxD",
            b"cAPGi-5HaxD2HaxD",
            b"CCR1009-7G-1C-1S+",
            b"CRS354-48G-4S+2Q+",
            b"CRS109-8G-1S-2HnD",
        ] {
            for port in build_ethernet_ports(model) {
                let name = port.default_name.to_string();
                let id = PhysicalPortId::from_str(&name)
                    .unwrap_or_else(|_| panic!("{name} of {model:?} not recognized"));
                assert_eq!(id.default_name().map(|n| n.to_string()), Some(name.clone()));
                assert!(id.is_ethernet(), "{name}");
            }
        }
    }

    #[test]
    fn port_names_and_short_names() {
        for (name, id, short) in [
            ("ether7", PhysicalPortId::Ethernet(7), "e07"),
            ("sfp-sfpplus2", PhysicalPortId::SfpSfpPlus(2), "s02"),
            ("sfp1", PhysicalPortId::Sfp(1), "s01"),
            ("combo1", PhysicalPortId::Combo(1), "c01"),
            ("qsfpplus1-2", PhysicalPortId::QsfpPlus(1, 2), "q01-2"),
        ] {
            assert_eq!(PhysicalPortId::from_str(name), Ok(id), "{name}");
            assert_eq!(id.short_name().to_string(), short);
        }
    }

    /// Names that merely contain a port name are virtual interfaces in NetBox (a
    /// VLAN interface, the 40G parent of a QSFP+ breakout), not that port.
    #[test]
    fn only_exact_port_names_are_ports() {
        for name in [
            "qsfpplus1",
            "vlan5-ether1",
            "ether1-uplink",
            "xsfp1",
            "bridge",
        ] {
            assert_eq!(PhysicalPortId::from_str(name), Err(()), "{name}");
        }
    }
}
