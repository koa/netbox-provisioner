//! Helper „Loopback“: the device gets `lo` (virtual) with the next free address of each
//! loopback pool (`/32`, `/128`), and those become its primary IPs, its router ID and
//! management address (`docs/netbox-modeling.md`). What it has already stays: an `lo`, an
//! address of a version on it, a primary IP that is that address.
//!
//! A free address in NetBox may still be in use by a device NetBox doesn't know of: an address
//! the network routes (`routes::Routes`) is skipped and shown in the plan
//! (`NetboxPlan.undocumented`), so it can be documented.
use super::{
    CreateAddress, CreateInterface, NetboxPlan, PlanStep, SetPrimaryIp, Target, device_ref,
    routes::Routes,
};
use crate::{
    Error,
    netbox::rest::NetboxRest,
    topology::{
        PrefixPool,
        access::{AccessTopology, device::DeviceAccess},
        findings::IpVersion,
    },
};
use ipnet::IpNet;
use std::net::IpAddr;

pub async fn plan(device: &DeviceAccess) -> Result<NetboxPlan, Error> {
    let mut plan = NetboxPlan::new(device, format!("Loopback {}", device.name()));
    let lo = device.interfaces().iter().find(|i| i.name() == "lo").cloned();
    let on = match &lo {
        Some(lo) => Target::Existing(lo.id().0),
        None => {
            plan.steps.push(PlanStep::CreateInterface(CreateInterface {
                device: device_ref(device),
                name: "lo".into(),
                kind: "virtual".into(),
                vlan: None,
                tags: Vec::new(),
            }));
            Target::Step(plan.steps.len() - 1)
        }
    };
    let rest = NetboxRest::reader()?;
    // read once an address is needed: a plan that only sets what exists asks no router
    let mut routes = None;
    for v6 in [false, true] {
        let version = if v6 { IpVersion::V6 } else { IpVersion::V4 };
        let existing = lo
            .iter()
            .flat_map(|lo| lo.ips())
            .find(|ip| ip.net().is_some_and(|net| matches!(net, IpNet::V6(_)) == v6));
        let (address_id, address) = match &existing {
            Some(ip) => (
                Target::Existing(ip.id().0),
                ip.net().map(|net| net.to_string()).unwrap_or_default(),
            ),
            None => {
                if routes.is_none() {
                    let read = Routes::read(device.topology()).await?;
                    plan.checked_with = Some(read.router.clone());
                    routes = Some(read);
                }
                let routes = routes.as_ref().expect("read above");
                let (prefix, host) = next_free(device, &rest, routes, &mut plan, v6).await?;
                let address = IpNet::new(host, if v6 { 128 } else { 32 })
                    .map_err(|error| Error::Preview(error.to_string().into()))?
                    .to_string();
                plan.steps.push(PlanStep::CreateAddress(CreateAddress {
                    address: address.clone().into(),
                    device: device_ref(device),
                    interface: "lo".into(),
                    prefix: prefix.to_string().into(),
                    on,
                }));
                (Target::Step(plan.steps.len() - 1), address)
            }
        };
        let primary = if v6 {
            device.primary_ip_v6()
        } else {
            device.primary_ip_v4()
        };
        let before_id = primary.as_ref().map(|ip| ip.id().0);
        if address_id != Target::Existing(before_id.unwrap_or(u32::MAX)) {
            plan.steps.push(PlanStep::SetPrimaryIp(SetPrimaryIp {
                device: device_ref(device),
                version,
                address: address.into(),
                before: primary
                    .as_ref()
                    .and_then(|ip| ip.net())
                    .map(|net| net.to_string().into()),
                address_id,
                before_id,
            }));
        }
    }
    Ok(plan)
}

/// How many free addresses of a pool prefix are looked at: past that many in use but not
/// documented, NetBox is too far off for a helper to pick one.
const CANDIDATES: usize = 32;

/// The first address of the loopback pool for the IP version that NetBox has free
/// (`available-ips`) and the network doesn't route, and the prefix it is in. The routed ones
/// passed over go into `plan.undocumented`.
async fn next_free(
    device: &DeviceAccess,
    rest: &NetboxRest,
    routes: &Routes,
    plan: &mut NetboxPlan,
    v6: bool,
) -> Result<(IpNet, IpAddr), Error> {
    for (id, prefix) in device.topology().pool_prefixes(PrefixPool::Loopback, v6) {
        let free = rest
            .get(&format!("ipam/prefixes/{}/available-ips/?limit={CANDIDATES}", id.0))
            .await?;
        let candidates = free
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|free| free["address"].as_str())
            .filter_map(|address| address.parse::<IpNet>().ok());
        for candidate in candidates {
            match routes.in_use(candidate.addr(), prefix) {
                Some(taken) => plan.undocumented.push(taken),
                None => return Ok((prefix, candidate.addr())),
            }
        }
    }
    Err(Error::NoFreeAddress("loopback", if v6 { 6 } else { 4 }))
}
