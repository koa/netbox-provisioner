//! Scope: The helper „WLAN ändern“: SSID and key of a WLAN; not who may (`graphql::resident`).
//!
//! Helper „WLAN ändern“: a new SSID or WPA personal key for a WLAN, the resident's
//! `setWlanSsid` and `setWlanPsk` (`docs/concepts/self-service.md`). The plan is the WLAN
//! group's controller's: it and all APs of the group get the change. Security type and VLAN
//! stay the admins', so only a WLAN with a WPA personal key gets a new one.
use super::{NetboxPlan, PlanStep, UpdateWirelessLan};
use crate::{
    Error,
    topology::{
        Topology, WlanAuth, WlanId,
        access::{AccessTopology, wlan::WlanAccess},
    },
};
use async_graphql::InputObject;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct WlanInput {
    pub wlan: u32,
    /// the new SSID, none to keep it
    pub ssid: Option<String>,
    /// the new key, none to keep it
    pub psk: Option<String>,
}

/// The shortest key a WLAN gets (the concept's rule, WPA allows 8).
pub const PSK_MIN: usize = 12;
/// The longest passphrase WPA allows (64 characters would be the key in hex).
pub const PSK_MAX: usize = 63;
/// The longest SSID 802.11 allows, in bytes.
pub const SSID_MAX: usize = 32;

/// The WLAN of `id`, if the topology has it (its group has a controller).
pub fn wlan(topology: &Arc<Topology>, id: u32) -> Result<WlanAccess, Error> {
    let wlan = WlanAccess::create(topology.clone(), WlanId(id));
    wlan.data()
        .is_some()
        .then_some(wlan)
        .ok_or(Error::UnknownWlan(id))
}

/// Whether `psk` can be a WPA passphrase here: [`PSK_MIN`] to [`PSK_MAX`] printable ASCII
/// characters (what WPA allows, RouterOS takes and a phone can type).
pub fn check_psk(psk: &str) -> Result<(), Error> {
    let printable = psk.chars().all(|c| c.is_ascii_graphic() || c == ' ');
    if (PSK_MIN..=PSK_MAX).contains(&psk.len()) && printable {
        Ok(())
    } else {
        Err(Error::PskInvalid)
    }
}

/// Whether `ssid` can be the SSID of `wlan`: 1 to [`SSID_MAX`] bytes, no other WLAN of its
/// group has it (the residents' too, which one doesn't see).
pub fn check_ssid(wlan: &WlanAccess, ssid: &str) -> Result<(), Error> {
    if ssid.is_empty() || ssid.len() > SSID_MAX || ssid.trim() != ssid {
        return Err(Error::SsidInvalid);
    }
    let taken = wlan.wlan_group().is_some_and(|group| {
        group.wlan().any(|other| {
            AccessTopology::id(&other) != AccessTopology::id(wlan) && other.ssid() == Some(ssid)
        })
    });
    if taken {
        return Err(Error::SsidTaken(ssid.into()));
    }
    Ok(())
}

pub fn plan(topology: &Arc<Topology>, input: &WlanInput) -> Result<NetboxPlan, Error> {
    let wlan = wlan(topology, input.wlan)?;
    let ssid_before: Box<str> = wlan.ssid().unwrap_or_default().into();
    let ssid = input
        .ssid
        .as_deref()
        .filter(|ssid| *ssid != &*ssid_before)
        .map(|ssid| check_ssid(&wlan, ssid).map(|()| Box::<str>::from(ssid)))
        .transpose()?;
    let psk_before = match wlan.auth() {
        Some(WlanAuth::Wpa(settings)) => Some(settings.psk()),
        _ => None,
    };
    let psk = match (&input.psk, psk_before) {
        (None, _) => None,
        (Some(psk), Some(before)) if psk == before => None,
        (Some(psk), Some(_)) => {
            check_psk(psk)?;
            Some(Box::<str>::from(psk.as_str()))
        }
        (Some(_), None) => return Err(Error::WlanNotPersonal(ssid_before)),
    };
    if ssid.is_none() && psk.is_none() {
        return Err(Error::NothingToChange);
    }
    let controller = wlan
        .wlan_group()
        .and_then(|group| group.controller())
        .ok_or(Error::UnknownWlan(input.wlan))?;
    let mut plan = NetboxPlan::new(&controller, format!("WLAN {ssid_before}"));
    plan.steps
        .push(PlanStep::UpdateWirelessLan(UpdateWirelessLan {
            id: input.wlan,
            ssid,
            ssid_before,
            psk_set: psk.is_some(),
            psk,
            psk_before: psk_before.unwrap_or_default().into(),
        }));
    Ok(plan)
}

#[cfg(test)]
mod test;
