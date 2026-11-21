//! Scope: What a resident has, from the NetBox owners of devices, ports and VLANs
//! (`docs/self-service.md`); not who logs in as which resident, nor what they may
//! change.
use crate::topology::{
    CablePort, DeviceId, FrontPortId, InterfaceId, OwnerData, OwnerId, Topology, VlanId,
    WlanGroupId, WlanId,
};
use std::collections::BTreeSet;

#[cfg(test)]
mod test;

/// What kind of object a resident has (or hasn't), for the refusal of one that isn't its own.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Belonging {
    Resident,
    Device,
    Port,
    Socket,
    Vlan,
    Wlan,
    Location,
}

impl Belonging {
    /// Its name in an error's params (`what`).
    pub fn code(self) -> &'static str {
        match self {
            Belonging::Resident => "RESIDENT",
            Belonging::Device => "DEVICE",
            Belonging::Port => "PORT",
            Belonging::Socket => "SOCKET",
            Belonging::Vlan => "VLAN",
            Belonging::Wlan => "WLAN",
            Belonging::Location => "LOCATION",
        }
    }
}

/// Everything of one owner: what NetBox gives it and what follows from that.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Owned {
    /// its devices (`owner` at the device)
    pub devices: BTreeSet<DeviceId>,
    /// the ports of its devices and the ports of other devices it has (`owner` at the
    /// interface)
    pub ports: BTreeSet<InterfaceId>,
    /// the sockets in its flat (`owner` at the front port)
    pub sockets: BTreeSet<FrontPortId>,
    /// its networks (`owner` at the VLAN)
    pub vlans: BTreeSet<VlanId>,
    /// the WLANs on its VLANs
    pub wlans: BTreeSet<WlanId>,
    /// the groups of its WLANs
    pub wlan_groups: BTreeSet<WlanGroupId>,
}

impl Topology {
    /// The owners NetBox names at a device, port or VLAN.
    pub fn owners(&self) -> impl Iterator<Item = (OwnerId, &OwnerData)> {
        self.owners.iter().map(|(id, owner)| (*id, owner))
    }
    /// What `owner` has; nothing for an owner no device, port or VLAN names.
    pub fn owned_by(&self, owner: OwnerId) -> Owned {
        let Some(data) = self.owners.get(&owner) else {
            return Owned::default();
        };
        let devices: BTreeSet<DeviceId> = data
            .devices
            .iter()
            .copied()
            .filter(|device| self.devices.contains_key(device))
            .collect();
        let ports = devices
            .iter()
            .filter_map(|device| self.devices.get(device))
            .flat_map(|device| device.ports.iter())
            .filter_map(|port| match port {
                CablePort::Interface(interface) => Some(*interface),
                _ => None,
            })
            .chain(data.interfaces.iter().copied())
            .filter(|interface| self.interfaces.contains_key(interface))
            .collect();
        let sockets = data
            .front_ports
            .iter()
            .copied()
            .filter(|port| self.front_ports.contains_key(port))
            .collect();
        let vlans: BTreeSet<VlanId> = data
            .vlans
            .iter()
            .copied()
            .filter(|vlan| self.vlans.contains_key(vlan))
            .collect();
        let wlans: BTreeSet<WlanId> = self
            .wlans
            .iter()
            .filter(|(_, wlan)| wlan.vlan.is_some_and(|vlan| vlans.contains(&vlan)))
            .map(|(id, _)| *id)
            .collect();
        let wlan_groups = wlans
            .iter()
            .filter_map(|wlan| self.wlans.get(wlan))
            .map(|wlan| wlan.wlan_group)
            .collect();
        Owned {
            devices,
            ports,
            sockets,
            vlans,
            wlans,
            wlan_groups,
        }
    }
}
