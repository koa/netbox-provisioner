//! Helper „Richtfunk abräumen“: takes a radio link's network out of NetBox, so „Funkbrücke“ or
//! another link can take its place (`docs/concepts/interface-view.md`). From one radio of the
//! link, its L3 ends are the bridges of both bridge devices and the router ports (or their
//! bridge) cabled to them. Of these it takes the IPv4 addresses in the segment's networks (any
//! on a bridge device's bridge, in the transfer pool, or at two ends at least), each network's
//! prefix once
//! nothing else is in it, `ospf` where no other IPv4 stays, and at both cables the label where
//! it names the device at the other end. Cables, bridge devices, their bridges and the radios
//! stay; the wireless link goes only when asked (`wireless_link`).
//!
//! Applying then cuts the bridge devices off (nothing announces their loopback any more), and a
//! router reached only over this link (`NetboxPlan.cut_off`).
use super::{
    DeleteAddress, DeleteWirelessLink, NetboxPlan, PlanStep, UpdateInterface, device_ref,
    disconnect::{address_text, delete_prefixes, has_other_link},
    flags::InterfaceTag,
};
use crate::{
    Error,
    topology::{
        DeviceId, InterfaceId, PrefixPool, Topology,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess,
            ip_addresses::IpAddressAccess,
        },
        findings::{interface_ref, names},
    },
};
use async_graphql::InputObject;
use ipnet::IpNet;
use std::sync::Arc;

/// A radio of the link, for the device whose page asks (its job writes it).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct RadioDisconnectInput {
    pub device: u32,
    pub radio: u32,
    /// the wireless link goes too
    pub wireless_link: bool,
}

/// One bridge device of the link: its radio, bridge, the port cabled to a router and that
/// router's port.
struct Side {
    device: DeviceAccess,
    bridge: Option<InterfaceAccess>,
    ether: Option<InterfaceAccess>,
    router_port: Option<InterfaceAccess>,
}

pub fn plan(topology: &Arc<Topology>, input: &RadioDisconnectInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let radio = topology
        .get_interface_by_id(InterfaceId(input.radio))
        .ok_or(Error::UnknownInterface(input.radio))?;
    let peer = radio.wireless_peer();
    let sides = std::iter::once(&radio)
        .chain(peer.iter())
        .map(side)
        .collect::<Result<Vec<_>, _>>()?;
    let mut plan = NetboxPlan::new(
        &device,
        format!(
            "Richtfunk abräumen {}",
            sides.iter().map(|s| s.device.name()).collect::<Vec<_>>().join(" – ")
        ),
    );

    // the L3 ends, each once
    let mut ends = Vec::<InterfaceAccess>::new();
    for side in &sides {
        let router_l3 = side
            .router_port
            .as_ref()
            .map(|port| port.bridge().unwrap_or_else(|| port.clone()));
        for end in router_l3.into_iter().chain(side.bridge.clone()) {
            if !ends.iter().any(|e| e.id() == end.id()) {
                ends.push(end);
            }
        }
    }
    let bridges = sides.iter().filter_map(|s| s.bridge.as_ref()).collect::<Vec<_>>();
    let is_bridge = |end: &InterfaceAccess| bridges.iter().any(|b| b.id() == end.id());
    let networks = |end: &InterfaceAccess| {
        end.ips()
            .iter()
            .filter_map(|ip| ip.net())
            .filter(|net| matches!(net, IpNet::V4(_)))
            .map(|net| net.trunc())
            .collect::<Vec<_>>()
    };
    let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
    let segment = ends
        .iter()
        .flat_map(&networks)
        .filter(|net| {
            let at = ends.iter().filter(|end| networks(end).contains(net)).count();
            at >= 2
                || ends.iter().any(|end| is_bridge(end) && networks(end).contains(net))
                || pools.iter().any(|(_, pool)| pool.contains(&net.addr()))
        })
        .collect::<Vec<_>>();
    let in_segment = |ip: &IpAddressAccess| {
        ip.net()
            .is_some_and(|net| matches!(net, IpNet::V4(_)) && segment.contains(&net.trunc()))
    };

    let mut freed = Vec::<IpAddressAccess>::new();
    for end in &ends {
        let owner = end.device().ok_or(Error::UnknownInterface(end.id().0))?;
        for ip in end.ips().iter().filter(|ip| in_segment(ip)) {
            let primary = [owner.primary_ip_v4(), owner.primary_ip_v6()];
            if primary.iter().flatten().any(|primary| primary.id() == ip.id()) {
                return Err(Error::PrimaryAddress {
                    address: address_text(ip),
                    device: owner.name().into(),
                });
            }
            plan.steps.push(PlanStep::DeleteAddress(DeleteAddress {
                id: ip.id().0,
                address: address_text(ip),
                interface: interface_ref(end),
            }));
            freed.push(ip.clone());
        }
    }
    delete_prefixes(&mut plan, freed.iter());

    // `ospf` where no IPv4 stays, the labels naming the other end of each cable
    let mut updates = Vec::<UpdateInterface>::new();
    let mut without_ospf = Vec::new();
    for end in &ends {
        let stays = end.ips().iter().any(|ip| {
            !in_segment(ip) && ip.net().is_some_and(|net| matches!(net, IpNet::V4(_)))
        });
        if end.use_ospf() && !stays {
            let index = update(&mut updates, end);
            updates[index].tags_removed.push(InterfaceTag::Ospf);
            without_ospf.push(end.id());
        }
    }
    for side in &sides {
        let (Some(ether), Some(router_port)) = (&side.ether, &side.router_port) else {
            continue;
        };
        let router = router_port.device();
        for (end, far) in [(router_port, Some(side.device.clone())), (ether, router)] {
            let Some(far) = far else { continue };
            if end.label().is_some_and(|label| names(label, far.name())) {
                let index = update(&mut updates, end);
                updates[index].label = Some("".into());
            }
        }
    }
    plan.steps.extend(updates.into_iter().map(PlanStep::UpdateInterface));

    if input.wireless_link
        && let (Some(end), Some(peer)) = (radio.wireless_link(), &peer)
    {
        plan.steps.push(PlanStep::DeleteWirelessLink(DeleteWirelessLink {
            id: end.id,
            a: interface_ref(&radio),
            b: interface_ref(peer),
        }));
    }
    if plan.steps.is_empty() {
        return Err(Error::NothingToDisconnect(super::port_name(&radio)));
    }

    // a bridge device without `ospf` left is reached no more, a router maybe not either
    for side in &sides {
        let ospf_left = side
            .device
            .interfaces()
            .iter()
            .any(|i| i.use_ospf() && !without_ospf.contains(&i.id()));
        if !ospf_left {
            plan.cut_off.push(device_ref(&side.device));
        }
        if let Some(port) = &side.router_port
            && let Some(router) = port.device()
            && freed.iter().any(|ip| port.bridge().unwrap_or_else(|| port.clone()).ips().iter().any(|own| own.id() == ip.id()))
            && !has_other_link(&router, port)
            && !plan.cut_off.iter().any(|cut| cut.id == router.id().0)
        {
            plan.cut_off.push(device_ref(&router));
        }
    }
    Ok(plan)
}

/// The index of `interface`'s update in `updates`, a new one where it has none: one per
/// interface, a second would find the first one's change when it is written.
fn update(updates: &mut Vec<UpdateInterface>, interface: &InterfaceAccess) -> usize {
    if let Some(index) = updates.iter().position(|u| u.interface.id == interface.id().0) {
        return index;
    }
    updates.push(UpdateInterface {
        interface: interface_ref(interface),
        label: None,
        label_before: interface.label().map(Box::from),
        mtu: None,
        mtu_before: interface.mtu(),
        tags_added: Vec::new(),
        tags_removed: Vec::new(),
        poe: None,
        poe_before: None,
        poe_removed: false,
    });
    updates.len() - 1
}

/// The bridge device of `radio` and what of it the link has.
fn side(radio: &InterfaceAccess) -> Result<Side, Error> {
    let device = radio.device().ok_or(Error::UnknownInterface(radio.id().0))?;
    let bridge = radio.bridge();
    let to_router = |port: &InterfaceAccess| {
        port.connected_interfaces().into_iter().find(|far| {
            far.device().is_some_and(|d| {
                d.has_routeros() && d.id() != device.id() && !d.is_bridge_device()
            })
        })
    };
    let cabled = device
        .interfaces()
        .into_vec()
        .into_iter()
        .filter(|i| i.is_ethernet_port())
        .filter_map(|port| to_router(&port).map(|far| (port, far)))
        .collect::<Vec<_>>();
    // the bridge's port first
    let in_bridge = cabled.iter().find(|(port, _)| {
        port.bridge().zip(bridge.as_ref()).is_some_and(|(a, b)| a.id() == b.id())
    });
    let (ether, router_port) = in_bridge
        .or(cabled.first())
        .cloned()
        .map_or((None, None), |(port, far)| (Some(port), Some(far)));
    Ok(Side {
        device,
        bridge,
        ether,
        router_port,
    })
}

#[cfg(test)]
mod test;
