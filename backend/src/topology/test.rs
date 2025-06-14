use crate::topology::{
    Cable, CableId, CablePort, Device, DeviceId, FrontPort, FrontPortId, Interface, InterfaceId,
    IpRangeData, IpRangeId, RearPort, RearPortId, Topology, TopologyHolder, VlanData,
    VlanGroupData, VlanGroupId, VlanId, VxlanData, VxlanId, WlanData, WlanGroupData, WlanGroupId,
    WlanId,
};
use std::{collections::HashMap, sync::Arc};
use tokio::{sync::Mutex, time::Instant};

#[derive(Default, Clone)]
pub struct TopologyBuilder {
    pub devices: HashMap<DeviceId, Device>,
    pub interfaces: HashMap<InterfaceId, Interface>,
    pub vxlans: HashMap<VxlanId, VxlanData>,
    pub wlan_groups: HashMap<WlanGroupId, WlanGroupData>,
    pub wlans: HashMap<WlanId, WlanData>,
    pub vlan_groups: HashMap<VlanGroupId, VlanGroupData>,
    pub vlans: HashMap<VlanId, VlanData>,
    pub front_ports: HashMap<FrontPortId, FrontPort>,
    pub rear_ports: HashMap<RearPortId, RearPort>,
    pub cables: HashMap<CableId, Cable>,
    pub ip_ranges: HashMap<IpRangeId, IpRangeData>,
    next_device_id: u32,
    next_interface_id: u32,
    next_vxlan_id: u32,
    next_vlan_id: u32,
    next_vlan_group_id: u32,
    next_wlan_id: u32,
    next_wlan_group_id: u32,
    next_front_port_id: u32,
    next_rear_port_id: u32,
    next_cable_id: u32,
    next_ip_range_id: u32,
}
fn post_incr(id: &mut u32) -> u32 {
    let ret = *id;
    *id += 1;
    ret
}

impl TopologyBuilder {
    pub fn next_device_id(&mut self) -> DeviceId {
        DeviceId(post_incr(&mut self.next_device_id))
    }
    pub fn next_interface_id(&mut self) -> InterfaceId {
        InterfaceId(post_incr(&mut self.next_interface_id))
    }
    pub fn next_vxlan_id(&mut self) -> VxlanId {
        VxlanId(post_incr(&mut self.next_vxlan_id))
    }
    pub fn next_vlan_id(&mut self) -> VlanId {
        VlanId(post_incr(&mut self.next_vlan_id))
    }
    pub fn next_vlan_group_id(&mut self) -> VlanGroupId {
        VlanGroupId(post_incr(&mut self.next_vlan_group_id))
    }
    pub fn next_wlan_id(&mut self) -> WlanId {
        WlanId(post_incr(&mut self.next_wlan_id))
    }
    pub fn next_wlan_group_id(&mut self) -> WlanGroupId {
        WlanGroupId(post_incr(&mut self.next_wlan_group_id))
    }
    pub fn next_front_port_id(&mut self) -> FrontPortId {
        FrontPortId(post_incr(&mut self.next_front_port_id))
    }
    pub fn next_rear_port_id(&mut self) -> RearPortId {
        RearPortId(post_incr(&mut self.next_rear_port_id))
    }
    pub fn next_cable_id(&mut self) -> CableId {
        CableId(post_incr(&mut self.next_cable_id))
    }
    pub fn next_ip_range_id(&mut self) -> IpRangeId {
        IpRangeId(post_incr(&mut self.next_ip_range_id))
    }
    pub fn build(mut self) -> Topology {
        for (id, interface) in &self.interfaces {
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
        }
        for (id, cable) in &self.cables {
            for cable_port in cable.port_b.iter() {
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
        for (id, front_port) in &self.front_ports {
            if let Some(rp_id) = front_port.rear_port {
                self.rear_ports
                    .get_mut(&rp_id)
                    .expect("rear_port not found")
                    .front_port = Some(*id);
            }
        }
        for (id, rear_port) in &self.rear_ports {
            if let Some(fp_id) = rear_port.front_port {
                self.front_ports
                    .get_mut(&fp_id)
                    .expect("front_port not found")
                    .rear_port = Some(*id);
            }
        }

        let mut ip_ranges_idx = HashMap::new();
        for (id, range) in &self.ip_ranges {
            ip_ranges_idx
                .entry(range.net)
                .or_insert_with(Vec::new)
                .push(*id);
        }

        Topology {
            fetch_time: Instant::now(),
            devices: self.devices,
            interfaces: self.interfaces,
            front_ports: self.front_ports,
            rear_ports: self.rear_ports,
            cables: self.cables,
            vxlans: self.vxlans,
            wlan_groups: self.wlan_groups,
            wlans: self.wlans,
            vlan_groups: self.vlan_groups,
            vlans: self.vlans,
            ip_ranges: self.ip_ranges,
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
