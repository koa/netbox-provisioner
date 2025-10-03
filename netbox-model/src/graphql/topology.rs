use crate::graphql::netbox::{
    CableId, CableTerminationTerminationType, CableType, DeviceFrontPort, DeviceId,
    DeviceInterface, DeviceRearPort, DeviceType, FetchTopology, FrontPortId, IPaddressType,
    InterfaceId, IpAddressId, IpRangeId, IprangeType, L2VpnassignmentType, PrefixId, PrefixType,
    RearPortId, Vlan, VlanGroup, VlanGroupId, VlanId, VxLan, VxlanId, Wlan, WlanGroup, WlanGroupId,
    WlanId,
};
use self_cell::self_cell;
use std::collections::HashMap;
use std::iter::FilterMap;

self_cell!(
    pub struct Topology {
        owner: FetchTopology,
        #[covariant]
        dependent: TopologyIndex,
    }
);
struct TopologyIndex<'a> {
    devices: HashMap<DeviceId, &'a DeviceType>,
    interfaces: HashMap<InterfaceId, (DeviceId, &'a DeviceInterface)>,
    front_ports: HashMap<FrontPortId, (DeviceId, &'a DeviceFrontPort)>,
    rear_ports: HashMap<RearPortId, (DeviceId, &'a DeviceRearPort)>,
    front_port_by_rear_port: HashMap<RearPortId, FrontPortId>,
    ip_addresses: HashMap<IpAddressId, &'a IPaddressType>,
    prefixes: HashMap<PrefixId, &'a PrefixType>,
    vlan_groups: HashMap<VlanGroupId, &'a VlanGroup>,
    vlans: HashMap<VlanId, (VlanGroupId, &'a Vlan)>,
    cables: HashMap<CableId, &'a CableType>,
    cable_by_termination: HashMap<CableTerminationId, (CableTerminationSide, &'a CableType)>,
    vxlans: HashMap<VxlanId, &'a VxLan>,
    vxlan_of_interface: HashMap<InterfaceId, VxlanId>,
    vxlan_of_vlan: HashMap<VlanId, VxlanId>,
    wlan_groups: HashMap<WlanGroupId, &'a WlanGroup>,
    wlans: HashMap<WlanId, (WlanGroupId, &'a Wlan)>,
    ip_ranges: HashMap<IpRangeId, &'a IprangeType>,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CableTerminationSide {
    A,
    B,
}
impl CableTerminationSide {
    pub fn other_side(self) -> CableTerminationSide {
        match self {
            CableTerminationSide::A => CableTerminationSide::B,
            CableTerminationSide::B => CableTerminationSide::A,
        }
    }
}
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CableTerminationId {
    FrontPort(FrontPortId),
    RearPort(RearPortId),
    Interface(InterfaceId),
}
impl CableTerminationId {
    fn new(termination: &CableTerminationTerminationType) -> Option<Self> {
        match termination {
            CableTerminationTerminationType::ConnectedInterface(interface) => {
                Some(CableTerminationId::Interface(interface.id))
            }
            CableTerminationTerminationType::ConnectedFrontPort(fp) => {
                Some(CableTerminationId::FrontPort(fp.id))
            }
            CableTerminationTerminationType::ConnectedRearPort(rp) => {
                Some(CableTerminationId::RearPort(rp.id))
            }
            CableTerminationTerminationType::Other => None,
        }
    }
}

impl Topology {
    pub fn create(data: FetchTopology) -> Topology {
        Topology::new(data, |data| {
            let devices = data
                .device_list
                .iter()
                .map(|device| (device.id, device))
                .collect();
            let interfaces = data
                .device_list
                .iter()
                .flat_map(|device| {
                    device
                        .interfaces
                        .iter()
                        .map(move |interface| (interface.id, (device.id, interface)))
                })
                .collect::<HashMap<_, _>>();
            let front_ports = data
                .device_list
                .iter()
                .flat_map(|device| {
                    device
                        .frontports
                        .iter()
                        .map(move |front_port| (front_port.id, (device.id, front_port)))
                })
                .collect();
            let rear_ports = data
                .device_list
                .iter()
                .flat_map(|device| {
                    device
                        .rearports
                        .iter()
                        .map(move |rear_port| (rear_port.id, (device.id, rear_port)))
                })
                .collect();
            let front_port_by_rear_port = data
                .device_list
                .iter()
                .flat_map(|device| device.frontports.iter().map(|fp| (fp.rear_port.id, fp.id)))
                .collect();
            let ip_addresses = data
                .ip_address_list
                .iter()
                .map(|ip_address| (ip_address.id, ip_address))
                .collect();

            let prefixes = data
                .prefix_list
                .iter()
                .map(|prefix| (prefix.id, prefix))
                .collect();
            let vlan_groups = data
                .vlan_group_list
                .iter()
                .map(|vlan_group| (vlan_group.id, vlan_group))
                .collect();
            let vlans = data
                .vlan_group_list
                .iter()
                .flat_map(|vlan_group| {
                    vlan_group
                        .vlans
                        .iter()
                        .map(|vlan| (vlan.id, (vlan_group.id, vlan)))
                })
                .collect();
            let cables = data
                .cable_list
                .iter()
                .map(|cable| (cable.id, cable))
                .collect();
            let cable_by_termination = data
                .cable_list
                .iter()
                .flat_map(|cable| {
                    FilterMap::chain(
                        cable.a_terminations.iter().filter_map(move |termination| {
                            CableTerminationId::new(termination).map(|termination_id| {
                                (termination_id, (CableTerminationSide::A, cable))
                            })
                        }),
                        cable.b_terminations.iter().filter_map(move |termination| {
                            CableTerminationId::new(termination).map(|termination_id| {
                                (termination_id, (CableTerminationSide::B, cable))
                            })
                        }),
                    )
                })
                .collect();
            let vxlans = data
                .l2vpn_list
                .iter()
                .map(|vxlans| (vxlans.id, vxlans))
                .collect();
            let wlan_groups = data
                .wireless_lan_group_list
                .iter()
                .map(|wlan| (wlan.id, wlan))
                .collect();
            let wlans = data
                .wireless_lan_group_list
                .iter()
                .flat_map(|wlan_group| {
                    wlan_group
                        .wireless_lans
                        .iter()
                        .map(|wlan| (wlan.id, (wlan_group.id, wlan)))
                })
                .collect();
            let ip_ranges = data
                .ip_range_list
                .iter()
                .map(|ip_range| (ip_range.id, ip_range))
                .collect();

            let vxlan_of_interface = data
                .l2vpn_list
                .iter()
                .flat_map(|vxlan| {
                    vxlan.terminations.iter().filter_map(|t| {
                        if let L2VpnassignmentType::InterfaceType(if_type) = &t.assigned_object {
                            Some((if_type.id, vxlan.id))
                        } else {
                            None
                        }
                    })
                })
                .collect();
            let vxlan_of_vlan = data
                .l2vpn_list
                .iter()
                .flat_map(|vxlan| {
                    vxlan.terminations.iter().filter_map(|t| {
                        if let L2VpnassignmentType::Vlantype(vlan_type) = &t.assigned_object {
                            Some((vlan_type.id, vxlan.id))
                        } else {
                            None
                        }
                    })
                })
                .collect();

            TopologyIndex {
                devices,
                interfaces,
                front_ports,
                rear_ports,
                front_port_by_rear_port,
                ip_addresses,
                prefixes,
                vlan_groups,
                vlans,
                cables,
                cable_by_termination,
                vxlans,
                vxlan_of_interface,
                vxlan_of_vlan,
                wlan_groups,
                wlans,
                ip_ranges,
            }
        })
    }
    pub fn device(&self, id: DeviceId) -> Option<&DeviceType> {
        self.borrow_dependent().devices.get(&id).copied()
    }
    pub fn devices(&self) -> impl Iterator<Item = &DeviceType> {
        self.borrow_dependent().devices.values().copied()
    }
    pub fn interface(&self, id: InterfaceId) -> Option<(DeviceId, &DeviceInterface)> {
        self.borrow_dependent().interfaces.get(&id).copied()
    }
    pub fn interfaces(&self) -> impl Iterator<Item = (DeviceId, &DeviceInterface)> {
        self.borrow_dependent().interfaces.values().copied()
    }
    pub fn front_port(&self, id: FrontPortId) -> Option<(DeviceId, &DeviceFrontPort)> {
        self.borrow_dependent().front_ports.get(&id).copied()
    }
    pub fn rear_port(&self, id: RearPortId) -> Option<(DeviceId, &DeviceRearPort)> {
        self.borrow_dependent().rear_ports.get(&id).copied()
    }
    pub fn front_port_by_rear_port(&self, id: RearPortId) -> Option<FrontPortId> {
        self.borrow_dependent()
            .front_port_by_rear_port
            .get(&id)
            .copied()
    }
    pub fn front_ports(&self) -> impl Iterator<Item = (DeviceId, &DeviceFrontPort)> {
        self.borrow_dependent().front_ports.values().copied()
    }
    pub fn rear_ports(&self) -> impl Iterator<Item = (DeviceId, &DeviceRearPort)> {
        self.borrow_dependent().rear_ports.values().copied()
    }
    pub fn ip_address(&self, id: IpAddressId) -> Option<&IPaddressType> {
        self.borrow_dependent().ip_addresses.get(&id).copied()
    }
    pub fn ip_addresses(&self) -> impl Iterator<Item = &IPaddressType> {
        self.borrow_dependent().ip_addresses.values().copied()
    }
    pub fn prefix(&self, id: PrefixId) -> Option<&PrefixType> {
        self.borrow_dependent().prefixes.get(&id).copied()
    }
    pub fn prefixes(&self) -> impl Iterator<Item = &PrefixType> {
        self.borrow_dependent().prefixes.values().copied()
    }
    pub fn vlan_group(&self, id: VlanGroupId) -> Option<&VlanGroup> {
        self.borrow_dependent().vlan_groups.get(&id).copied()
    }
    pub fn vlan_groups(&self) -> impl Iterator<Item = &VlanGroup> {
        self.borrow_dependent().vlan_groups.values().copied()
    }
    pub fn vlans(&self) -> impl Iterator<Item = (VlanGroupId, &Vlan)> {
        self.borrow_dependent().vlans.values().copied()
    }
    pub fn vlan(&self, id: VlanId) -> Option<(VlanGroupId, &Vlan)> {
        self.borrow_dependent().vlans.get(&id).copied()
    }
    pub fn cable(&self, id: CableId) -> Option<&CableType> {
        self.borrow_dependent().cables.get(&id).copied()
    }
    pub fn cables(&self) -> impl Iterator<Item = &CableType> {
        self.borrow_dependent().cables.values().copied()
    }
    pub fn cable_by_termination(
        &self,
        termination: &CableTerminationTerminationType,
    ) -> Option<(CableTerminationSide, &CableType)> {
        self.borrow_dependent()
            .cable_by_termination
            .get(&CableTerminationId::new(termination)?)
            .copied()
    }
    pub fn vxlans(&self) -> impl Iterator<Item = &VxLan> {
        self.borrow_dependent().vxlans.values().copied()
    }
    pub fn vxlan(&self, id: VxlanId) -> Option<&VxLan> {
        self.borrow_dependent().vxlans.get(&id).copied()
    }
    pub fn vxlan_of_interface(&self, id: InterfaceId) -> Option<VxlanId> {
        self.borrow_dependent().vxlan_of_interface.get(&id).copied()
    }
    pub fn vxlan_of_vlan(&self, id: VlanId) -> Option<VxlanId> {
        self.borrow_dependent().vxlan_of_vlan.get(&id).copied()
    }

    pub fn wlan_group(&self, id: WlanGroupId) -> Option<&WlanGroup> {
        self.borrow_dependent().wlan_groups.get(&id).copied()
    }
    pub fn wlan_groups(&self) -> impl Iterator<Item = &WlanGroup> {
        self.borrow_dependent().wlan_groups.values().copied()
    }
    pub fn wlans(&self) -> impl Iterator<Item = (WlanGroupId, &Wlan)> {
        self.borrow_dependent().wlans.values().copied()
    }
    pub fn wlan(&self, id: WlanId) -> Option<(WlanGroupId, &Wlan)> {
        self.borrow_dependent().wlans.get(&id).copied()
    }
    pub fn ip_ranges(&self) -> impl Iterator<Item = &IprangeType> {
        self.borrow_dependent().ip_ranges.values().copied()
    }
    pub fn ip_range(&self, id: IpRangeId) -> Option<&IprangeType> {
        self.borrow_dependent().ip_ranges.get(&id).copied()
    }
}
