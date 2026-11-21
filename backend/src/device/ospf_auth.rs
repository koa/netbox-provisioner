//! Scope: Which OSPF segments an apply may move to new credentials, and to which (A10,
//! `docs/concepts/ospf-auth.md`): a segment's key, what each device of the apply reads for it,
//! the decision over all of them and what is in doubt; not placing the credentials in the
//! config (`device::ros::ospf`).
//!
//! Credentials change only where it is shown that every end of the segment is in the apply;
//! in doubt they stay as they are.
#[cfg(test)]
mod test;

use crate::config::{CONFIG, OspfAuth};
use crate::device::ros::ospf::{OspfCredentials, OspfPort, OspfSwitch};
use crate::device::ros::warning::{
    OspfAuthDoubt, OspfAuthKept, OspfCredentialsDiffer, OspfEndNotOspf, OspfEndsOutside,
    OspfForeignNeighbor, OspfNeighborMissing,
};
use crate::topology::DeviceId;
use crate::topology::access::device::DeviceAccess;
use hmac::{Hmac, KeyInit, Mac};
use ipnet::{IpNet, Ipv4Net};
use mikrotik_model::ascii::AsciiString;
use mikrotik_model::command::Reads;
use mikrotik_model::model::{
    OspfNeighborState, RoutingOspfInterfaceTemplateAuth, RoutingOspfNeighborState,
};
use mikrotik_model::resource::collect_resource;
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::net::{IpAddr, Ipv4Addr};

/// The OSPF instances every segment runs in, as the generator names them.
pub const INSTANCES: [&[u8]; 2] = [b"default-v2", b"default-v3"];

/// The credentials the provisioner has for the segment `network`: `sha256` with the key from
/// the current version's secret, the version as `auth-id`.
pub fn target_credentials(settings: &OspfAuth, network: Ipv4Net) -> OspfCredentials {
    OspfCredentials {
        auth: Some(RoutingOspfInterfaceTemplateAuth::Sha256),
        id: Some(settings.current()),
        key: Some(segment_key(settings.current_secret(), network)),
    }
}

/// `hex(HMAC-SHA256(secret, "<network>"))`: 64 characters, which RouterOS takes for `sha256`
/// (CHR 7.24.6). Both ends compute it from the same data, it is stored nowhere else.
fn segment_key(secret: &str, network: Ipv4Net) -> AsciiString {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(network.to_string().as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in mac.finalize().into_bytes() {
        write!(hex, "{byte:02x}").expect("to a string");
    }
    hex.as_str().into()
}

/// What a device of the apply reads for the decision: its OSPF ports with their segments and
/// credentials (`DeviceCfg::ospf_ports`, after a generate) and its OSPF neighbors.
pub struct MemberRead {
    pub device: DeviceAccess,
    pub ports: Box<[OspfPort]>,
    pub neighbors: Box<[RoutingOspfNeighborState]>,
}

/// The device's OSPF neighbors, for [`MemberRead`].
pub async fn read_neighbors(
    reader: &impl Reads,
) -> Result<Box<[RoutingOspfNeighborState]>, crate::Error> {
    Ok(collect_resource::<RoutingOspfNeighborState>(reader).await?)
}

/// What the apply of one device does with its segments' credentials.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Decision {
    /// the segments moved to the provisioner's credentials
    pub switch: OspfSwitch,
    /// the ends of each segment moved, by its port: who has to be its neighbor again after the
    /// script
    pub expected: Box<[ExpectedNeighbors]>,
    /// the segments not at the provisioner's credentials that stay, and why
    pub kept: Box<[OspfAuthKept]>,
}

/// The other ends of a segment moved, as router IDs: the device's neighbors at `port` once
/// the script is through.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpectedNeighbors {
    pub port: AsciiString,
    pub router_ids: BTreeSet<Ipv4Addr>,
}

/// Whether `neighbors` (a device's) show every one of `expected` at its port, `Full` or
/// `2-Way`, in v2 and v3.
pub fn back(expected: &[ExpectedNeighbors], neighbors: &[RoutingOspfNeighborState]) -> bool {
    expected.iter().all(|segment| {
        INSTANCES.iter().all(|instance| {
            segment.router_ids.iter().all(|router_id| {
                neighbors.iter().any(|n| {
                    *n.instance.0 == **instance
                        && n.router_id == *router_id
                        && n.interface.as_ref().is_none_or(|i| *i == segment.port)
                        && matches!(n.state, OspfNeighborState::Full | OspfNeighborState::TwoWay)
                })
            })
        })
    })
}

/// An end of a segment as NetBox has it.
struct End {
    device: DeviceAccess,
    router_id: Ipv4Addr,
}

/// Decides for every device of an apply which of its segments go to the provisioner's
/// credentials: those where every condition holds (`docs/concepts/ospf-auth.md`, Freigabe),
/// shown for all ends together; the others stay, with what is in doubt. Without `settings`
/// nothing moves and nothing is said.
pub fn decide(
    settings: Option<&OspfAuth>,
    members: &[&MemberRead],
) -> BTreeMap<DeviceId, Decision> {
    let mut decisions = members
        .iter()
        .map(|member| (member.device.id(), Decision::default()))
        .collect::<BTreeMap<_, _>>();
    let Some(settings) = settings else {
        return decisions;
    };
    let networks = members
        .iter()
        .flat_map(|member| member.ports.iter().map(|port| port.network))
        .collect::<BTreeSet<_>>();
    for network in networks {
        let target = target_credentials(settings, network);
        let at = members
            .iter()
            .flat_map(|member| {
                member
                    .ports
                    .iter()
                    .filter(move |port| port.network == network)
                    .map(move |port| (member, port))
            })
            .collect::<Vec<_>>();
        if at
            .iter()
            .all(|(_, port)| port.v2 == target && port.v3 == target)
        {
            continue;
        }
        match proven(network, members) {
            Ok(ends) => {
                for (member, port) in &at {
                    let decision = decisions.entry(member.device.id()).or_default();
                    decision.switch.insert(network, target.clone());
                    let mut expected = decision.expected.to_vec();
                    expected.push(ExpectedNeighbors {
                        port: port.port.clone(),
                        router_ids: ends
                            .iter()
                            .filter(|end| end.device.id() != member.device.id())
                            .map(|end| end.router_id)
                            .collect(),
                    });
                    decision.expected = expected.into();
                }
            }
            Err(doubt) => {
                for (member, port) in &at {
                    if port.v2 == target && port.v3 == target {
                        continue;
                    }
                    let decision = decisions.entry(member.device.id()).or_default();
                    let mut kept = decision.kept.to_vec();
                    kept.push(OspfAuthKept {
                        port: port.port.to_string().into(),
                        network: network.to_string().into(),
                        doubt: doubt.clone(),
                    });
                    decision.kept = kept.into();
                }
            }
        }
    }
    decisions
}

/// [`decide`] for a device applied alone, with the provisioner's settings: only a segment of
/// which it is the one end moves.
pub fn decide_alone(member: Option<&MemberRead>) -> Decision {
    member
        .and_then(|member| decide(CONFIG.ospf_auth.as_ref(), &[member]).remove(&member.device.id()))
        .unwrap_or_default()
}

/// [`decide`] for the devices of a batch, with the provisioner's settings.
pub fn decide_together(members: &[&MemberRead]) -> BTreeMap<DeviceId, Decision> {
    // none read for it: nothing to decide (and no settings to load in the batch's tests)
    if members.is_empty() {
        return BTreeMap::new();
    }
    decide(CONFIG.ospf_auth.as_ref(), members)
}

/// The ends of the segment `network`, once shown that every one is a member, sees exactly the
/// others and has the same credentials; else what is in doubt, the first condition failing.
fn proven(network: Ipv4Net, members: &[&MemberRead]) -> Result<Box<[End]>, OspfAuthDoubt> {
    let ends = netbox_ends(network, members)?;
    // each end with its member, or the names of those outside
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for end in &ends {
        match members.iter().find(|m| m.device.id() == end.device.id()) {
            Some(member) => inside.push((end, member)),
            None => outside.push(Box::<str>::from(end.device.name())),
        }
    }
    if !outside.is_empty() {
        return Err(OspfAuthDoubt::EndsOutside(OspfEndsOutside {
            devices: outside.into(),
        }));
    }
    let mut credentials = Vec::new();
    for (end, member) in inside {
        let mut ports = member.ports.iter().filter(|port| port.network == network);
        let (Some(port), None) = (ports.next(), ports.next()) else {
            return Err(OspfAuthDoubt::EndNotOspf(OspfEndNotOspf {
                address: network.to_string().into(),
                device: Some(end.device.name().into()),
            }));
        };
        sees_exactly_the_others(member, port, &ends)?;
        credentials.push((port.v2.clone(), port.v3.clone()));
    }
    if credentials.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(OspfAuthDoubt::CredentialsDiffer(OspfCredentialsDiffer {
            devices: ends.iter().map(|end| end.device.name().into()).collect(),
        }));
    }
    Ok(ends)
}

/// The ends NetBox has in `network`: every address in it on an interface with `ospf` of a
/// RouterOS device with a primary IPv4, with the network's prefix length, one per device.
fn netbox_ends(network: Ipv4Net, members: &[&MemberRead]) -> Result<Box<[End]>, OspfAuthDoubt> {
    let topology = members[0].device.topology();
    let mut ends = BTreeMap::new();
    for address in topology.address_records_in(IpNet::V4(network)) {
        let not_ospf = |device: Option<&DeviceAccess>| {
            OspfAuthDoubt::EndNotOspf(OspfEndNotOspf {
                address: address
                    .net()
                    .map(|net| net.to_string())
                    .unwrap_or_default()
                    .into(),
                device: device.map(|device| device.name().into()),
            })
        };
        let interface = address.interface();
        let device = interface.as_ref().and_then(|interface| interface.device());
        let router_id = device
            .as_ref()
            .and_then(|device| device.primary_ip_v4()?.addr());
        match (&interface, &device, router_id) {
            (Some(interface), Some(device), Some(IpAddr::V4(router_id)))
                if device.has_routeros()
                    && interface.use_ospf()
                    && address.net().map(|net| net.prefix_len()) == Some(network.prefix_len())
                    && !ends.contains_key(&device.id()) =>
            {
                ends.insert(
                    device.id(),
                    End {
                        device: device.clone(),
                        router_id,
                    },
                );
            }
            _ => return Err(not_ospf(device.as_ref())),
        }
    }
    Ok(ends.into_values().collect())
}

/// Whether `member` sees exactly the other `ends` at `port`, each `Full` or `2-Way`, in v2 and
/// v3. A neighbor without an interface could be at any port, so it counts at every one.
fn sees_exactly_the_others(
    member: &MemberRead,
    port: &OspfPort,
    ends: &[End],
) -> Result<(), OspfAuthDoubt> {
    let others = ends
        .iter()
        .filter(|end| end.device.id() != member.device.id())
        .collect::<Vec<_>>();
    for instance in INSTANCES {
        let at_port = member
            .neighbors
            .iter()
            .filter(|n| *n.instance.0 == *instance)
            .filter(|n| n.interface.as_ref().is_none_or(|i| *i == port.port))
            .collect::<Vec<_>>();
        if let Some(foreign) = at_port
            .iter()
            .find(|n| !others.iter().any(|end| end.router_id == n.router_id))
        {
            return Err(OspfAuthDoubt::ForeignNeighbor(OspfForeignNeighbor {
                device: member.device.name().into(),
                router_id: foreign.router_id.to_string().into(),
            }));
        }
        if let Some(missing) = others.iter().find(|end| {
            !at_port.iter().any(|n| {
                n.router_id == end.router_id
                    && matches!(n.state, OspfNeighborState::Full | OspfNeighborState::TwoWay)
            })
        }) {
            return Err(OspfAuthDoubt::NeighborMissing(OspfNeighborMissing {
                device: member.device.name().into(),
                missing: missing.device.name().into(),
            }));
        }
    }
    Ok(())
}
