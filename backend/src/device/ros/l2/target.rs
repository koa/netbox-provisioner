//! Scope: Writing an [`L2Setup`] into the base config: bridges, bridge ports and VLANs and the
//! ports' generated names; not computing it (`l2`).
use super::{L2Port, L2Setup, NameGenerator, SimpleBridgePort};
use crate::{
    device::ros::{
        BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, hw_facts::RadioPackage,
    },
    topology::{PhysicalPortId, access::device::DeviceAccess},
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        InterfaceBridgeProtocolMode, InterfaceVlanByName, InterfaceVlanCfg, InterfaceVxlanByName,
        InterfaceVxlanCfg, VlanFrameTypes,
    },
    value,
};
use std::collections::{HashMap, HashSet};

impl BaseDeviceDataTarget {
    /// Realizes an already-computed `L2Setup` as RouterOS bridge/bridge-port/
    /// bridge-vlan/interface-vlan mutations and renames each physical ethernet port
    /// to its generated name. IP/DHCP/OSPF config is handled separately via
    /// `L2Setup::logical_interfaces` (see `setup_ip_addresses`/`setup_ospf`), since
    /// none of that needs to know which of the three L2 domain kinds it came from.
    /// `radio_package` is the one of the device's radios, if NetBox uses them: on a CAP whose
    /// radios don't tag VLANs, a radio without a bridge of its own is an access port of the
    /// WLANs' VLAN (`WifiDeviceDataTarget::generate_from`).
    pub(in crate::device::ros) fn setup_l2<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        setup: &L2Setup<N>,
        radio_package: Option<RadioPackage>,
    ) -> Result<(), SetupError> {
        let radio_access_ports = if device.wifi_cap_of().is_some()
            && radio_package.is_some_and(|package| package.is_wifi() && !package.tags_vlans())
        {
            device
                .interfaces()
                .iter()
                .filter_map(|interface| match interface.external_port() {
                    Some(port @ PhysicalPortId::Wifi(_)) => match setup.placement_of_port(port) {
                        Some(placement) if placement.bridge.is_none() => {
                            Some(placement.name.clone())
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for single in setup.single_ports.iter() {
            if let Some(ethernet_port) = self.get_ethernet_port(single.port)? {
                ethernet_port.name = single.name.clone();
            }
        }
        for bridge in setup.simple_bridges.iter() {
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            bridge_cfg.0.vlan_filtering = false;
            bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Rstp;
            if setup.snoops_multicast(bridge.switch_ports(), bridge.carries_radio(), false) {
                bridge_cfg.0.snoop_multicast();
            }
            // Without vlan filtering RouterOS accepts the vlan fields but never reports them,
            // so a value here would be a diff forever; mvrp is reported by some versions
            // (7.20), not by others (7.24), and keeps whatever it is.
            bridge_cfg.0.ether_type = None;
            bridge_cfg.0.frame_types = None;
            bridge_cfg.0.ingress_filtering = None;
            bridge_cfg.0.pvid = None;
            bridge_cfg.0.mvrp = None;
            for port in &bridge.ports {
                if let SimpleBridgePort::Ethernet {
                    name,
                    port: port_id,
                    ..
                } = port
                {
                    if let Some(ethernet_port) = self.get_ethernet_port(*port_id)? {
                        ethernet_port.name = name.clone();
                    }
                    // the radio of a CAP joins the bridge through its datapath
                    // (`WifiDeviceDataTarget::generate_from`)
                    if matches!(port_id, PhysicalPortId::Wifi(_)) && device.wifi_cap_of().is_some()
                    {
                        continue;
                    }
                    let bridge_port = self
                        .bridge_port
                        .entry((bridge.name.clone(), name.clone()))
                        .or_default();
                    bridge_port.frame_types = VlanFrameTypes::AdmitOnlyUntaggedAndPriorityTagged;
                    // a `/interface/wireless` or `/interface/w60g` port has no hardware
                    // offloading, and RouterOS doesn't report `hw` for it (seen on 7.20.8,
                    // `wlan60-1` of a wAP 60G on 7.23.8)
                    if matches!(port_id, PhysicalPortId::Wlan(_) | PhysicalPortId::W60g(_)) {
                        bridge_port.hw = None;
                    }
                }
            }
        }
        for bridge in setup.tagged_bridges.iter() {
            // a CRS1xx/2xx switches the VLANs itself (`switch_menu`), its bridge stays in
            // hardware only without VLAN filtering
            let in_switch_menu = setup.vlans_in_switch_menu(bridge);
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            if in_switch_menu {
                bridge_cfg.0.vlan_filtering = false;
                bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Rstp;
                // as a simple bridge's: without VLAN filtering they aren't reported
                bridge_cfg.0.ether_type = None;
                bridge_cfg.0.frame_types = None;
                bridge_cfg.0.ingress_filtering = None;
                bridge_cfg.0.pvid = None;
                bridge_cfg.0.mvrp = None;
            } else {
                bridge_cfg.0.vlan_filtering = true;
                bridge_cfg.0.mvrp = Some(false);
                bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
            }
            if setup.snoops_multicast(
                bridge.switch_ports(),
                bridge.carries_radio(),
                !in_switch_menu,
            ) {
                bridge_cfg.0.snoop_multicast();
            }
            // A bridge whose only way to other bridges is one VXLAN (a CAP's `bridge-caps`;
            // its radios are clients) closes no loop, and STP over the VXLAN, a segment it
            // shares with every bridge of the WLAN group, blocked its port (ap-buero-1og,
            // 7.23.7: no client got an address until STP was off). Without STP RouterOS
            // doesn't report the STP settings, but `forward-reserved-addresses` (7.24.5). It
            // keeps them hidden and rejects an empty value (`forward-delay=""`, 7.24.5), so they
            // stay at RouterOS's defaults, which mikrotik-model reads for a hidden one.
            let uplinks = bridge
                .ports
                .values()
                .flat_map(|entry| &entry.ports)
                .filter_map(|port| match port {
                    L2Port::TaggedEthernet { name, port, .. }
                    | L2Port::UntaggedEthernet { name, port, .. }
                        if !matches!(port, PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_)) =>
                    {
                        Some((name, false))
                    }
                    L2Port::VxLan { name, .. } => Some((name, true)),
                    _ => None,
                })
                .collect::<HashSet<_>>();
            if uplinks.len() == 1 && uplinks.iter().all(|(_, vxlan)| *vxlan) {
                let bridge_cfg = &mut bridge_cfg.0;
                bridge_cfg.protocol_mode = InterfaceBridgeProtocolMode::None;
                bridge_cfg.region_name = None;
                bridge_cfg.forward_reserved_addresses = Some(false);
            }
            let mut tags_of_port = HashMap::<AsciiString, (Option<u16>, Vec<u16>)>::new();
            let mut ports_of_vlan = HashMap::<u16, (Vec<AsciiString>, Vec<AsciiString>)>::new();
            for (&vlan_id, entry) in &bridge.ports {
                if entry.has_vlan_interface() {
                    self.vlan.entry(entry.if_name.clone()).or_insert_with(|| {
                        InterfaceVlanByName(InterfaceVlanCfg {
                            interface: bridge.name.clone(),
                            vlan_id,
                            ..Default::default()
                        })
                    });
                    // With VLAN filtering, a VLAN interface on the bridge only sees
                    // traffic if the bridge itself is a tagged member of that VLAN.
                    ports_of_vlan
                        .entry(vlan_id)
                        .or_default()
                        .1
                        .push(bridge.name.clone());
                }
                for port in &entry.ports {
                    match port {
                        L2Port::TaggedEthernet {
                            name,
                            port: port_id,
                            ..
                        } => {
                            if let Some(ethernet_port) = self.get_ethernet_port(*port_id)? {
                                ethernet_port.name = name.clone();
                            }
                            tags_of_port
                                .entry(name.clone())
                                .or_default()
                                .1
                                .push(vlan_id);
                            ports_of_vlan
                                .entry(vlan_id)
                                .or_default()
                                .1
                                .push(name.clone());
                        }
                        L2Port::UntaggedEthernet {
                            name,
                            port: port_id,
                            ..
                        } => {
                            if let Some(ethernet_port) = self.get_ethernet_port(*port_id)? {
                                ethernet_port.name = name.clone();
                            }
                            tags_of_port.entry(name.clone()).or_default().0 = Some(vlan_id);
                            ports_of_vlan
                                .entry(vlan_id)
                                .or_default()
                                .0
                                .push(name.clone());
                        }
                        L2Port::VxLan { name, vxlan } => {
                            let Some(vni) = vxlan.vni() else { continue };
                            let my_ip = device
                                .primary_ip_v4()
                                .and_then(|ip| ip.addr())
                                .ok_or_else(|| SetupError::MissingVtepAddress(name.clone()))?;
                            self.vxlan.entry(name.clone()).or_insert_with(|| {
                                InterfaceVxlanByName(InterfaceVxlanCfg {
                                    vni,
                                    local_address: Some(my_ip),
                                    ..Default::default()
                                })
                            });
                            for remote in vxlan.vteps().into_iter().filter(|ip| *ip != my_ip) {
                                self.vxlan_vteps.entry((name.clone(), remote)).or_default();
                            }
                            tags_of_port
                                .entry(name.clone())
                                .or_default()
                                .1
                                .push(vlan_id);
                            ports_of_vlan
                                .entry(vlan_id)
                                .or_default()
                                .1
                                .push(name.clone());
                        }
                        L2Port::Cap { .. } => {
                            for name in &radio_access_ports {
                                let untagged = &mut ports_of_vlan.entry(vlan_id).or_default().0;
                                if !untagged.contains(name) {
                                    untagged.push(name.clone());
                                }
                                tags_of_port.entry(name.clone()).or_default().0 = Some(vlan_id);
                            }
                        }
                        L2Port::CapsMan { .. }
                        | L2Port::CapsManMgmt
                        | L2Port::CapMgmt
                        | L2Port::Ip { .. } => {}
                    }
                }
            }
            if in_switch_menu {
                // the switch tags and filters (`switch_menu`): plain ports of the bridge
                for port_name in tags_of_port.into_keys() {
                    self.bridge_port
                        .entry((bridge.name.clone(), port_name))
                        .or_default()
                        .frame_types = VlanFrameTypes::AdmitAll;
                }
                continue;
            }
            for (port_name, (untagged, tagged)) in tags_of_port {
                let is_radio = radio_access_ports.contains(&port_name);
                let bridge_port = self
                    .bridge_port
                    .entry((bridge.name.clone(), port_name))
                    .or_default();
                bridge_port.ingress_filtering = true;
                // a radio has no hardware offloading, RouterOS doesn't report `hw` for it
                if is_radio {
                    bridge_port.hw = None;
                }
                bridge_port.frame_types = if let Some(untagged_id) = untagged {
                    bridge_port.pvid = untagged_id;
                    if tagged.is_empty() {
                        VlanFrameTypes::AdmitOnlyUntaggedAndPriorityTagged
                    } else {
                        VlanFrameTypes::AdmitAll
                    }
                } else {
                    VlanFrameTypes::AdmitOnlyVlanTagged
                };
            }
            for (vlan_id, (untagged, tagged)) in ports_of_vlan {
                self.bridge_vlan
                    .entry((
                        bridge.name.clone(),
                        tagged.into_iter().collect(),
                        untagged.into_iter().collect(),
                        Some(value::PossibleRangeDash::Single(vlan_id))
                            .into_iter()
                            .collect(),
                    ))
                    .or_default();
            }
        }
        Ok(())
    }
}

/// `current` as the diff sees it: a bridge the target takes VLAN filtering from hides its
/// `ether-type`, `frame-types`, `ingress-filtering` and `pvid` (lab-sxt-a, 7.24.6), which the
/// target leaves out, so the `set` doesn't clear them with `=""` (E17: lab-crs125's bridge
/// to the switch menus). Only for that change: a bridge that stays with VLAN filtering keeps
/// its values in the diff, a target that sets one still sets it.
pub(in crate::device::ros) fn after_vlan_filtering_off(
    target: &BaseDeviceDataTarget,
    current: &mut BaseDeviceDataCurrent,
) {
    for bridge in current.bridge.iter_mut() {
        let current = &mut bridge.0;
        let Some(target) = target.bridge.get(&current.name).map(|bridge| &bridge.0) else {
            continue;
        };
        if !current.vlan_filtering || target.vlan_filtering {
            continue;
        }
        if target.ether_type.is_none() {
            current.ether_type = None;
        }
        if target.frame_types.is_none() {
            current.frame_types = None;
        }
        if target.ingress_filtering.is_none() {
            current.ingress_filtering = None;
        }
        if target.pvid.is_none() {
            current.pvid = None;
        }
    }
}
