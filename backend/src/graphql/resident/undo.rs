//! Scope: Taking back a resident's saved change: what was there before each of its parts, as
//! the parts a new change would have, worked out from the resident's view when it saves; not
//! saving (`change`) nor keeping it (`auto_rollout::changes`).
//!
//! Taking back makes drafts of these, which the resident previews and saves like any change.
//! A part without a „before“ the resident may set (a port with the operator's network, a
//! device at the operator's port) has none.
use super::{PortKind, Resident, change::ResidentChange};
use async_graphql::{SimpleObject, Union};
use serde::{Deserialize, Serialize};

/// A part of a change that takes another back.
#[derive(Debug, Clone, PartialEq, Eq, Union, Serialize, Deserialize)]
pub enum UndoStep {
    Wlan(WlanUndo),
    PortVlan(PortVlanUndo),
    ConnectDevice(ConnectDeviceUndo),
    DisconnectDevice(DisconnectDeviceUndo),
    DeviceRoom(DeviceRoomUndo),
}

/// What a WLAN had: only what the change set.
#[derive(Clone, PartialEq, Eq, SimpleObject, Serialize, Deserialize)]
pub struct WlanUndo {
    pub wlan: u32,
    pub ssid: Option<Box<str>>,
    pub psk: Option<Box<str>>,
    pub enabled: Option<bool>,
}

// the key stays out of logs
impl std::fmt::Debug for WlanUndo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WlanUndo")
            .field("wlan", &self.wlan)
            .field("ssid", &self.ssid)
            .field("psk", &self.psk.as_ref().map(|_| "***"))
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// The network a port row had; none: no network.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject, Serialize, Deserialize)]
pub struct PortVlanUndo {
    pub port: u32,
    pub kind: PortKind,
    pub vlan: Option<u32>,
}

/// Where a device was linked: the port row and its Anschluss.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject, Serialize, Deserialize)]
pub struct ConnectDeviceUndo {
    pub device: u32,
    pub port: u32,
    pub kind: PortKind,
    pub device_port: Option<u32>,
}

/// A device that was linked nowhere.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject, Serialize, Deserialize)]
pub struct DisconnectDeviceUndo {
    pub device: u32,
}

/// The room a device stood in.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject, Serialize, Deserialize)]
pub struct DeviceRoomUndo {
    pub device: u32,
    pub location: u32,
}

/// What takes `changes` back, from `before`, the resident as it was when they were saved.
pub fn undo_of(before: &Resident, changes: &[ResidentChange]) -> Box<[UndoStep]> {
    let mut steps = Vec::new();
    for change in changes {
        match change {
            ResidentChange::Wlan(input) => {
                let Some(wlan) = before
                    .networks
                    .iter()
                    .flat_map(|network| network.wlans.iter())
                    .find(|wlan| wlan.id == input.wlan)
                else {
                    continue;
                };
                steps.push(UndoStep::Wlan(WlanUndo {
                    wlan: wlan.id,
                    ssid: input.ssid.as_ref().map(|_| wlan.ssid.clone()),
                    psk: input.psk.as_ref().and_then(|_| wlan.psk.clone()),
                    enabled: input.enabled.map(|_| wlan.enabled),
                }));
            }
            ResidentChange::PortVlan(input) => {
                let Some(port) = before
                    .ports
                    .iter()
                    .find(|port| port.id == input.port && port.kind == input.kind)
                    .filter(|port| !port.foreign_network)
                else {
                    continue;
                };
                steps.push(UndoStep::PortVlan(PortVlanUndo {
                    port: port.id,
                    kind: port.kind,
                    vlan: port.network.as_ref().map(|network| network.id),
                }));
            }
            ResidentChange::ConnectDevice(input) => steps.extend(link_of(before, input.device)),
            ResidentChange::DisconnectDevice(device) => steps.extend(link_of(before, *device)),
            ResidentChange::DeviceRoom(input) => {
                let location = before
                    .devices
                    .iter()
                    .find(|device| device.id == input.device)
                    .and_then(|device| device.location);
                if let Some(location) = location {
                    steps.push(UndoStep::DeviceRoom(DeviceRoomUndo {
                        device: input.device,
                        location,
                    }));
                }
            }
        }
    }
    steps.into()
}

/// How `device` was linked in `before`: at the port row its cable came from, with its
/// Anschluss, or nowhere; none at a port not the resident's (the operator's router).
fn link_of(before: &Resident, device: u32) -> Option<UndoStep> {
    let own = before.devices.iter().find(|own| own.id == device)?;
    let at = before.ports.iter().find(|port| {
        port.far_ends
            .iter()
            .any(|end| end.own_device && end.device.id == device)
    });
    match at {
        Some(port) => Some(UndoStep::ConnectDevice(ConnectDeviceUndo {
            device,
            port: port.id,
            kind: port.kind,
            device_port: own.uplink.as_ref().and_then(|uplink| {
                own.ports
                    .iter()
                    .find(|port| &port.name == uplink)
                    .map(|port| port.id)
            }),
        })),
        None if own.uplink.is_some() => None,
        None => Some(UndoStep::DisconnectDevice(DisconnectDeviceUndo { device })),
    }
}

#[cfg(test)]
mod test;
