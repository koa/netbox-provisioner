//! The L3 and L2 MTU of the ports, the same at both ends of a cable.
use super::{BaseDeviceDataTarget, SetupError};
use crate::topology::{
    PhysicalPortId,
    access::{device::DeviceAccess, interface::InterfaceAccess},
};

/// RouterOS's L3 MTU of a port NetBox gives none.
pub(crate) const DEFAULT_MTU: u16 = 1500;
/// What the L2 MTU leaves on top of the L3 MTU, for a VLAN tag.
const VLAN_TAG_SIZE: u16 = 4;

/// The L3 MTU NetBox gives `port`, the same as the far ends of its cable have: OSPF only
/// forms an adjacency between equal MTUs, and RouterOS 7 has no `mtu-ignore`.
pub(super) fn port_mtu(port: &InterfaceAccess) -> Result<Option<u16>, SetupError> {
    let mtu = port.mtu();
    for far_end in port.connected_interfaces() {
        let far_mtu = far_end.mtu().unwrap_or(DEFAULT_MTU);
        if far_mtu != mtu.unwrap_or(DEFAULT_MTU) {
            return Err(SetupError::MtuMismatch {
                port: port.name().into(),
                mtu: mtu.unwrap_or(DEFAULT_MTU),
                far_end: format!(
                    "{} {}",
                    far_end
                        .device()
                        .map(|d| d.name().to_string())
                        .unwrap_or_default(),
                    far_end.name()
                ),
                far_mtu,
            });
        }
    }
    Ok(mtu)
}

/// The L2 MTU of a port with the L3 MTU `mtu`: the board's (`l2mtu`), raised to carry `mtu`
/// with a VLAN tag.
pub(super) fn l2mtu_for(l2mtu: Option<u16>, mtu: Option<u16>) -> Option<u16> {
    match mtu {
        Some(mtu) => Some(l2mtu.unwrap_or_default().max(mtu + VLAN_TAG_SIZE)),
        None => l2mtu,
    }
}

impl BaseDeviceDataTarget {
    /// The L3 MTU NetBox gives an Ethernet port, with its L2 MTU raised to carry it (a
    /// `wlan` radio gets it in `WirelessDeviceDataTarget`).
    pub(super) fn setup_mtu(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        for port in device.interfaces() {
            match (port.external_port(), port_mtu(&port)?) {
                (_, None) | (Some(PhysicalPortId::Wlan(_)), _) => {}
                (Some(port_id), Some(mtu)) => {
                    let ethernet_port = self
                        .get_ethernet_port(port_id)?
                        .ok_or_else(|| SetupError::MtuNotSupported(port.name().into()))?;
                    ethernet_port.mtu = mtu;
                    ethernet_port.l_2_mtu = l2mtu_for(ethernet_port.l_2_mtu, Some(mtu));
                }
                (None, Some(_)) => return Err(SetupError::MtuNotSupported(port.name().into())),
            }
        }
        Ok(())
    }
}
