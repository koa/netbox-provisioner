//! Where a VLAN is one network (its broadcast domain) and which devices have L3 in it: with a
//! VNI on its group (`vxlan_vni`) every RouterOS device with the VLAN, which the VXLAN joins;
//! without, the RouterOS devices with the VLAN that cables carrying it join (a VLAN used on
//! one device alone stays local there). The rule „one L3 gateway per VLAN“ of
//! `docs/interface-view.md` counts per domain; `topology::findings` tests it.
use crate::topology::{
    DeviceId,
    access::{AccessTopology, device::DeviceAccess, interface::InterfaceAccess, vlan::VlanAccess},
};
use std::collections::{BTreeMap, BTreeSet};

/// The domains of `vlan`, each the RouterOS devices in it by id.
pub fn domains(vlan: &VlanAccess) -> Vec<BTreeSet<DeviceId>> {
    let devices = vlan
        .devices()
        .iter()
        .filter(|device| device.has_routeros())
        .map(|device| device.id())
        .collect::<BTreeSet<_>>();
    if devices.is_empty() {
        return Vec::new();
    }
    if vlan.vxlan().is_some() {
        return vec![devices];
    }
    // the devices joined by a cable whose ends both carry the VLAN
    let mut domain_of = devices.iter().map(|d| (*d, *d)).collect::<BTreeMap<_, _>>();
    fn root(domain_of: &BTreeMap<DeviceId, DeviceId>, mut id: DeviceId) -> DeviceId {
        while domain_of[&id] != id {
            id = domain_of[&id];
        }
        id
    }
    let topology = vlan.topology();
    for id in &devices {
        let Some(device) = topology.get_device_by_id(id) else {
            continue;
        };
        for interface in device.interfaces() {
            if !carries(&interface, vlan) {
                continue;
            }
            for far in interface.connected_interfaces() {
                let Some(far_device) = far.device() else { continue };
                if devices.contains(&far_device.id()) && carries(&far, vlan) {
                    let (a, b) = (root(&domain_of, *id), root(&domain_of, far_device.id()));
                    domain_of.insert(a.max(b), a.min(b));
                }
            }
        }
    }
    let mut domains = BTreeMap::<DeviceId, BTreeSet<DeviceId>>::new();
    for id in &devices {
        domains.entry(root(&domain_of, *id)).or_default().insert(*id);
    }
    domains.into_values().collect()
}

/// The domain of `vlan` that `device` is in.
pub fn domain_of(vlan: &VlanAccess, device: &DeviceAccess) -> BTreeSet<DeviceId> {
    domains(vlan)
        .into_iter()
        .find(|domain| domain.contains(&device.id()))
        .unwrap_or_default()
}

/// The interfaces of `device` with an address in `vlan`: one with the VLAN untagged and an
/// address, or the bridge with an address of a port with the VLAN untagged.
pub fn l3_interfaces(vlan: &VlanAccess, device: &DeviceAccess) -> Vec<InterfaceAccess> {
    let mut found = Vec::<InterfaceAccess>::new();
    for interface in device.interfaces() {
        if interface.untagged_vlan().map(|v| v.id) != Some(vlan.id) {
            continue;
        }
        let l3 = if !interface.ips().is_empty() {
            Some(interface.clone())
        } else {
            interface.bridge().filter(|bridge| !bridge.ips().is_empty())
        };
        if let Some(l3) = l3
            && !found.contains(&l3)
        {
            found.push(l3);
        }
    }
    found
}

fn carries(interface: &InterfaceAccess, vlan: &VlanAccess) -> bool {
    interface.carried_vlans().iter().any(|v| v.id == vlan.id)
}
