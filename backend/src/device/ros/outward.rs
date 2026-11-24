//! Scope: Whether an apply changes what a device announces outward (`docs/auto-rollout.md`,
//! „Anhalten“, nach aussen), from its config before and after; not holding the rollout for it
//! (`auto_rollout::hold`).
//!
//! OSPF carries every address of the area (its interfaces' and, by `redistribute`, the
//! connected ones) to every router of it, also to one outside NetBox: the lab's uplink once
//! announced a default route into production. Outward is a port with `internet` or `nat`, or
//! an OSPF port whose IPv4 networks no other RouterOS device of NetBox has on an OSPF port (its
//! OSPF neighbor is outside NetBox). An apply announces outward when it changes an address or
//! the OSPF settings of such a port, changes what OSPF originates or redistributes, or adds an
//! address outside NetBox's aggregates (the own address space).
use super::{
    BaseDeviceDataCurrent, BaseDeviceDataTarget,
    cfg::{DeviceCfg, following_renames, ospf_ports_renamed},
    l2::{L2Setup, NameGenerator},
};
use crate::topology::{access::device::DeviceAccess, management::ospf_networks};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString,
    model::{RoutingOriginateDefault, RoutingOspfInterfaceTemplateCfg},
};
use std::collections::{BTreeMap, BTreeSet};

/// The ports of `device` that face outward, by their RouterOS names.
pub(super) fn outward_ports<N: NameGenerator>(
    device: &DeviceAccess,
    l2: &L2Setup<N>,
) -> BTreeSet<AsciiString> {
    let others = device
        .topology()
        .list_devices()
        .filter(|other| other.has_routeros() && other.id() != device.id())
        .map(|other| ospf_networks(&other))
        .collect::<Vec<_>>();
    l2.logical_interfaces()
        .filter(|port| {
            let networks = port
                .addresses
                .iter()
                .filter_map(|ip| ip.net())
                .filter(|net| matches!(net, IpNet::V4(net) if net.prefix_len() < 32))
                .map(|net| net.trunc())
                .collect::<BTreeSet<_>>();
            // a port without an IPv4 network has no OSPF neighbor to judge by
            let alone = port.use_ospf
                && !networks.is_empty()
                && others.iter().all(|theirs| theirs.is_disjoint(&networks));
            port.internet || port.nat || alone
        })
        .map(|port| port.if_name.clone())
        .collect()
}

/// Whether `net` lies in one of `aggregates`.
fn own(net: IpNet, aggregates: &[IpNet]) -> bool {
    aggregates
        .iter()
        .any(|aggregate| aggregate.contains(&net.trunc()))
}

impl DeviceCfg {
    /// Whether applying it changes what the device announces outward (module doc), with
    /// `aggregates` the own address space.
    pub fn announces_outward(&self, aggregates: &[IpNet]) -> bool {
        let renames = self.radio_renames();
        let now = Now {
            addresses: following_renames(&self.base_target, &self.base_current, &renames),
            ospf: ospf_ports_renamed(&self.base_target, &self.base_current, &renames),
        };
        announces(&self.base_target, &now, &self.outward_ports, aggregates)
    }
}

/// The device as it is, with the names `target` gives its ports: for the addresses
/// (`following_renames`) and the OSPF templates (`ospf_ports_renamed`).
pub(super) struct Now {
    pub addresses: BaseDeviceDataCurrent,
    pub ospf: BaseDeviceDataCurrent,
}

/// Whether going from `now` to `target` changes what the device announces outward, with
/// `outward_ports` its ports facing outward and `aggregates` the own address space.
pub(super) fn announces(
    target: &BaseDeviceDataTarget,
    now: &Now,
    outward_ports: &BTreeSet<AsciiString>,
    aggregates: &[IpNet],
) -> bool {
    let current = &now.addresses;
    let outward = |interface: &AsciiString| outward_ports.contains(interface);
    // addresses: by address the interface it is on, before and after
    let before = current
        .ipv_4_address
        .iter()
        .map(|address| (IpNet::V4(address.0.address), &address.0.interface))
        .chain(
            current
                .ipv_6_address
                .iter()
                .map(|address| (IpNet::V6(address.0.address), &address.0.interface)),
        )
        .collect::<BTreeMap<_, _>>();
    // the target has the address in the key only
    let after = target
        .ipv_4_address
        .iter()
        .map(|(net, address)| (IpNet::V4(*net), &address.0.interface))
        .chain(
            target
                .ipv_6_address
                .iter()
                .map(|(net, address)| (IpNet::V6(*net), &address.0.interface)),
        )
        .collect::<BTreeMap<_, _>>();
    let addresses = before.keys().chain(after.keys()).any(|net| {
        let (was, is) = (before.get(net), after.get(net));
        was != is
            && (was.is_some_and(|interface| outward(interface))
                || is.is_some_and(|interface| outward(interface))
                || (is.is_some() && !own(*net, aggregates)))
    });
    // what OSPF originates and redistributes, and which instances there are
    let originates = |originate: Option<RoutingOriginateDefault>| {
        originate.filter(|originate| *originate != RoutingOriginateDefault::Never)
    };
    let instances_before = current
        .ospf_instance
        .iter()
        .map(|instance| {
            (
                &instance.0.name,
                (
                    originates(instance.0.originate_default),
                    &instance.0.redistribute,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    // the target has the name in the key only
    let instances_after = target
        .ospf_instance
        .iter()
        .map(|(name, instance)| {
            (
                name,
                (
                    originates(instance.0.originate_default),
                    &instance.0.redistribute,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let instances = instances_before != instances_after;
    // the OSPF settings of each outward port: its template without the list of ports
    let ospf = &now.ospf;
    let templates = outward_ports.iter().any(|port| {
        // the target has the area and comment in the key only
        let after = target
            .ospf_interface
            .iter()
            .map(
                |((area, comment), template)| RoutingOspfInterfaceTemplateCfg {
                    area: area.clone(),
                    comment: comment.clone(),
                    ..template.clone()
                },
            );
        settings_of(
            ospf.ospf_interface
                .iter()
                .map(|template| template.data.clone()),
            port,
        ) != settings_of(after, port)
    });
    addresses || instances || templates
}

/// The OSPF settings of `port`: the template it is in, without the list of ports.
fn settings_of(
    mut templates: impl Iterator<Item = RoutingOspfInterfaceTemplateCfg>,
    port: &AsciiString,
) -> Option<RoutingOspfInterfaceTemplateCfg> {
    templates
        .find(|template| template.interfaces.contains(port))
        .map(|template| RoutingOspfInterfaceTemplateCfg {
            interfaces: BTreeSet::new(),
            ..template
        })
}

#[cfg(test)]
mod test;
