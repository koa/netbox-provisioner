use crate::topology::{
    CablePort, Device, DeviceId, Interface, InterfaceId, Topology, TopologyHolder, VlanData,
    VlanGroupData, VlanGroupId, VlanId, VxlanData, VxlanId, WlanData, WlanGroupData, WlanGroupId,
    WlanId,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Default, Clone)]
pub struct TopologyBuilder {
    pub devices: HashMap<DeviceId, Device>,
    pub interfaces: HashMap<InterfaceId, Interface>,
    pub cable_path_endpoints: HashMap<CablePort, HashSet<CablePort>>,
    pub vxlans: HashMap<VxlanId, VxlanData>,
    pub wlan_groups: HashMap<WlanGroupId, WlanGroupData>,
    pub wlans: HashMap<WlanId, WlanData>,
    pub vlan_groups: HashMap<VlanGroupId, VlanGroupData>,
    pub vlans: HashMap<VlanId, VlanData>,

    next_device_id: u32,
    next_interface_id: u32,
    next_vxlan_id: u32,
    next_vlan_id: u32,
    next_vlan_group_id: u32,
    next_wlan_id: u32,
    next_wlan_group_id: u32,
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
        Topology {
            fetch_time: Instant::now(),
            devices: self.devices,
            interfaces: self.interfaces,
            cable_path_endpoints: self.cable_path_endpoints,
            vxlans: self.vxlans,
            wlan_groups: self.wlan_groups,
            wlans: self.wlans,
            vlan_groups: self.vlan_groups,
            vlans: self.vlans,
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
