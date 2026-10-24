//! Helper „VLAN am Port“: a VLAN untagged on a port of the device (802.1Q mode access,
//! `docs/concepts/interface-view.md`): the port of a customer network on a VLAN that reaches no
//! port yet (`VlanWithoutPort`), or a customer's port whose network is on another device, over
//! the VLAN group's VXLAN. Tagged VLANs on the port go (Z26).
//!
//! The VLANs come by group, and only those the generator can put on the port: none whose VID
//! another VLAN of the device has (`AmbiguousVlanIds`), none of a tunneled group whose VXLAN
//! is in another bridge on the device than the port would be (`PortInMultipleBridges`). A free
//! port goes into the modeled bridge carrying its VLAN, else into the default one
//! (`docs/l2-generation.md`), and so does the VXLAN. The plan checks the same for its port and
//! then runs the L2 setup on its preview, for what these rules miss.
use super::{NetboxPlan, PlanStep, UpdatePortVlan, port_name};
use crate::{
    Error,
    device::ros::l2::{KeepNameGenerator, L2Setup},
    topology::{
        DeviceId, InterfaceId, Topology, VlanId,
        access::{device::DeviceAccess, interface::InterfaceAccess, vlan::VlanAccess},
        findings::{VlanRef, interface_ref, vlan_ref},
        natural::naturalize_interface,
    },
};
use async_graphql::{InputObject, SimpleObject};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct PortVlanInput {
    pub device: u32,
    pub port: u32,
    pub vlan: u32,
}

/// What „VLAN am Port“ can choose from for a device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PortVlanOptions {
    pub ports: Vec<PortVlanPort>,
    /// the VLAN groups with a VLAN a port can have, by name
    pub groups: Vec<PortVlanGroup>,
    /// the group to start with: of a VLAN the device has, one of a WLAN first
    pub group: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PortVlanPort {
    pub id: u32,
    pub name: Box<str>,
    pub label: Option<Box<str>>,
    /// the VLAN it has untagged now
    pub vlan: Option<VlanRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PortVlanGroup {
    pub id: u32,
    pub name: Box<str>,
    /// a VNI: its VLANs reach other devices over VXLAN
    pub vxlan: bool,
    /// by VID
    pub vlans: Vec<VlanRef>,
}

/// Whether `port` can take a customer's VLAN: an Ethernet port, not in a bridge, no uplink
/// (`dhcp-client`, `nat`), without address, not cabled to a RouterOS device (that is a link).
fn check_port(port: &InterfaceAccess) -> Result<(), Error> {
    if !port.is_ethernet_port() {
        return Err(Error::NotEthernet(port_name(port)));
    }
    if let Some(bridge) = port.bridge() {
        return Err(Error::PortInBridge {
            port: port_name(port),
            bridge: bridge.name().into(),
        });
    }
    if port.is_enable_dhcp_client() || port.is_nat() {
        return Err(Error::PortUplink(port_name(port)));
    }
    if let Some(address) = port.ips().first() {
        return Err(Error::PortAddressed {
            port: port_name(port),
            address: address.net().map(|net| net.to_string()).unwrap_or_default().into(),
        });
    }
    if let Some(far) = port
        .connected_interfaces()
        .into_iter()
        .find(|far| far.device().is_some_and(|d| d.has_routeros()))
    {
        return Err(Error::PortCabled {
            port: port_name(port),
            far: Some(port_name(&far)),
        });
    }
    Ok(())
}

/// The VLANs a port of `device` can have: those the device has already, and those of a group
/// with a VNI, whose VXLAN brings them from another device. A local VLAN of another device
/// would reach nothing here.
fn reachable(device: &DeviceAccess) -> Vec<VlanAccess> {
    let mut vlans = device
        .topology()
        .list_vlans()
        .filter(|vlan| vlan.vxlan().is_some() || has(device, vlan, None))
        .collect::<Vec<_>>();
    vlans.sort_by_key(|vlan| (vlan.vlan_id(), vlan.name().map(Box::<str>::from)));
    vlans
}

/// Whether `device` has `vlan`, leaving out what `port` carries (it gets another VLAN).
fn has(device: &DeviceAccess, vlan: &VlanAccess, port: Option<InterfaceId>) -> bool {
    vlan.devices_but(port).iter().any(|d| d.id() == device.id())
}

/// Why the generator couldn't put `vlan` on a free port of `device` (`port`: the one it goes
/// on, whose VLANs don't count then).
fn conflict(
    device: &DeviceAccess,
    vlan: &VlanAccess,
    port: Option<InterfaceId>,
) -> Result<(), Error> {
    let topology = device.topology();
    let group = |vlan: &VlanAccess| vlan.group().map(|(_, name)| Box::from(name)).unwrap_or_default();
    // RouterOS knows the VID only: two VLANs with one VID on a device are one
    if let Some(taken) = topology.list_vlans().find(|other| {
        other.id != vlan.id && other.vlan_id() == vlan.vlan_id() && has(device, other, port)
    }) {
        return Err(Error::VlanIdTaken {
            vid: vlan.vlan_id().unwrap_or_default(),
            vlan: taken.name().unwrap_or_default().into(),
            group: group(&taken),
        });
    }
    // tunneled, it joins its group's VXLAN, which is in one bridge only: where the port goes
    let tunneled = |vlan: &VlanAccess| {
        vlan.vxlan().is_some() && vlan.devices_but(port).iter().any(|d| d.id() != device.id())
    };
    if !tunneled(vlan) {
        return Ok(());
    }
    // the modeled bridge carrying a VLAN on the device, `None` the default bridge
    let bridge_of = |vlan: &VlanAccess| {
        device
            .interfaces()
            .iter()
            .filter(|i| Some(i.id()) != port)
            .filter(|i| {
                i.untagged_vlan().is_some_and(|v| v.id == vlan.id)
                    || i.tagged_vlans().any(|v| v.id == vlan.id)
            })
            .find_map(|i| if i.is_bridge() { Some(i.clone()) } else { i.bridge() })
    };
    let target = bridge_of(vlan).map(|b| b.id());
    let group_id = vlan.group().map(|(id, _)| id);
    for other in topology.list_vlans() {
        if other.id == vlan.id
            || other.group().map(|(id, _)| id) != group_id
            || !has(device, &other, port)
            || !tunneled(&other)
        {
            continue;
        }
        let bridge = bridge_of(&other);
        if bridge.as_ref().map(|b| b.id()) != target {
            return Err(Error::VxlanInBridge {
                group: group(vlan),
                bridge: bridge.map(|b| b.name().into()),
            });
        }
    }
    Ok(())
}

pub fn options(device: &DeviceAccess) -> PortVlanOptions {
    let mut ports = device
        .interfaces()
        .into_vec()
        .into_iter()
        .filter(|port| check_port(port).is_ok())
        .collect::<Vec<_>>();
    ports.sort_by_cached_key(|port| naturalize_interface(port.name()));
    let mut groups = Vec::<PortVlanGroup>::new();
    for vlan in reachable(device) {
        if conflict(device, &vlan, None).is_err() {
            continue;
        }
        let (Some((id, name)), Some(vlan_ref)) = (vlan.group(), vlan_ref(&vlan)) else {
            continue;
        };
        match groups.iter_mut().find(|group| group.id == id.0) {
            Some(group) => group.vlans.push(vlan_ref),
            None => groups.push(PortVlanGroup {
                id: id.0,
                name: name.into(),
                vxlan: vlan.vxlan().is_some(),
                vlans: vec![vlan_ref],
            }),
        }
    }
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    // the group of a VLAN the device has, of a WLAN's first
    let mut own = device
        .topology()
        .list_vlans()
        .filter(|vlan| has(device, vlan, None))
        .collect::<Vec<_>>();
    own.sort_by_key(|vlan| (vlan.vlan_id(), vlan.id.0));
    let group = own
        .iter()
        .filter(|vlan| vlan.wlan().next().is_some())
        .chain(&own)
        .filter_map(|vlan| vlan.group().map(|(id, _)| id.0))
        .find(|id| groups.iter().any(|group| group.id == *id));
    PortVlanOptions {
        ports: ports
            .iter()
            .map(|port| PortVlanPort {
                id: port.id().0,
                name: port.name().into(),
                label: port.label().map(Box::from),
                vlan: port.untagged_vlan().as_ref().and_then(vlan_ref),
            })
            .collect(),
        groups,
        group,
    }
}

pub fn plan(topology: &Arc<Topology>, input: &PortVlanInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let port = topology
        .get_interface_by_id(InterfaceId(input.port))
        .filter(|port| port.device().is_some_and(|d| d.id() == device.id()))
        .ok_or(Error::UnknownInterface(input.port))?;
    check_port(&port)?;
    let vlan = reachable(&device)
        .into_iter()
        .find(|vlan| vlan.id == VlanId(input.vlan))
        .ok_or(Error::UnknownVlan(input.vlan))?;
    let chosen = vlan_ref(&vlan).ok_or(Error::UnknownVlan(input.vlan))?;
    let before = port.untagged_vlan();
    let tagged = port.tagged_vlans().collect::<Vec<_>>();
    if before.as_ref().is_some_and(|b| b.id == vlan.id) && tagged.is_empty() {
        return Err(Error::PortHasVlan {
            port: port_name(&port),
            vlan: chosen.vid,
        });
    }
    conflict(&device, &vlan, Some(port.id()))?;
    let mut plan = NetboxPlan::new(
        &device,
        format!("VLAN {} an {}", chosen.vid, port_name(&port)),
    );
    plan.steps.push(PlanStep::UpdatePortVlan(UpdatePortVlan {
        interface: interface_ref(&port),
        vlan: Some(chosen),
        vlan_before: before.as_ref().and_then(vlan_ref),
        tagged_before: tagged.iter().filter_map(vlan_ref).collect(),
    }));
    // what the rules above miss: the device's L2 as the generator sets it up, if it did before
    if L2Setup::new(&device, KeepNameGenerator).is_ok() {
        match plan.preview_device() {
            Ok(after) => {
                L2Setup::new(&after, KeepNameGenerator)?;
            }
            // a topology not built from NetBox (tests)
            Err(Error::NoPreview) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod test;
