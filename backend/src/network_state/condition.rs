//! Scope: The admins' page „Zustand“: what to check or maintain in the network, most urgent
//! first (`docs/concepts/monitoring.md`, C8); not reading the devices (`read`, `device_data`,
//! `device::uplink`).
//!
//! `Query.topology.networkCondition`: the findings of a site's, or every site's, OSPF routers
//! (the network map's candidates), in the order of the concept: a router the provisioner
//! doesn't reach, a dead link, a link up only in OSPFv2, an uplink losing pings or without a
//! default route, a reboot in the last 24 h, a PoE port without load. Health and resources are
//! shown, not judged (Z16). Reads the LSDB, then each router, then the uplinks' pings: one
//! device's reads stay sequential.

use super::{
    device_data::{self, DeviceMapData},
    edges::{DownReason, Edge, EdgeKind, EdgeState, edges},
    graphql::{MapEdgeKind, MapNodeDetails, details_nodes, supports_ospf},
    read::{read, readers},
};
use crate::{
    device::{
        poe::{self, PoeProblem},
        uplink::{self, UplinkPing},
    },
    error_info::ErrorInfo,
    graphql::scalars::ScalarDuration,
    topology::{
        DeviceId,
        access::{DeviceTarget, device::DeviceAccess},
        findings::{DeviceRef, InterfaceRef, device_ref, interface_ref},
    },
};
use async_graphql::futures_util::future::join_all;
use async_graphql::{Enum, SimpleObject};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

/// A reboot in this time is a finding.
pub const RECENT_REBOOT: Duration = Duration::from_secs(24 * 3600);

#[derive(SimpleObject)]
pub struct NetworkCondition {
    /// most urgent first, then by device
    findings: Box<[ConditionFinding]>,
    /// the routers that answered, with health and resources, by name
    devices: Box<[MapNodeDetails]>,
    /// why the LSDB couldn't be read: the links are then unknown, not found dead
    lsdb_error: Option<ErrorInfo>,
}

/// What a finding is about, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum ConditionKind {
    /// the provisioner doesn't reach the router
    Unreachable,
    /// a cable or wireless link between two routers is down in OSPFv2
    LinkDown,
    /// the link is up in OSPFv2 only: the two are no neighbors in OSPFv3
    #[graphql(name = "LINK_IPV6_DOWN")]
    LinkIpv6Down,
    /// a router with an `internet` port has no active default route
    NoDefaultRoute,
    /// pings to the uplink's gateway got lost
    UplinkLoss,
    /// the router booted in the last 24 h
    Rebooted,
    /// NetBox has the port feed, nothing at the other end draws power (`PoeProblem::NoLoad`)
    PoeNoLoad,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConditionFinding {
    pub kind: ConditionKind,
    pub device: DeviceRef,
    /// a link's ends, `device`'s first
    pub link: Option<ConditionLink>,
    /// the PoE port without load
    pub port: Option<InterfaceRef>,
    /// since the reboot
    pub uptime: Option<ScalarDuration>,
    pub uplink: Option<ConditionUplink>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConditionLink {
    pub kind: MapEdgeKind,
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

/// The pings to an uplink's gateway.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConditionUplink {
    pub gateway: Box<str>,
    /// the share lost, 0 to 1
    pub loss: f64,
    /// the average round trip of the answered ones, milliseconds
    pub rtt_ms: Option<f64>,
}

impl From<&UplinkPing> for ConditionUplink {
    fn from(ping: &UplinkPing) -> Self {
        ConditionUplink {
            gateway: ping.gateway.to_string().into(),
            loss: ping.loss(),
            rtt_ms: ping.rtt.map(|rtt| rtt.as_secs_f64() * 1000.0),
        }
    }
}

/// What an uplink router's pings got: none without an active default route.
pub type Uplinks = BTreeMap<DeviceId, Option<UplinkPing>>;

pub async fn network_condition(devices: Vec<DeviceAccess>) -> NetworkCondition {
    let readers = readers(&devices);
    let candidates = devices
        .iter()
        .filter(|d| supports_ospf(d, &readers))
        .cloned()
        .collect::<Vec<_>>();
    let (edges, lsdb_error) = match read(&readers).await {
        Ok(Some(read)) => (Some(edges(&devices, &read.lsdb)), None),
        Ok(None) => (None, None),
        Err(error) => (None, Some(ErrorInfo::from(&error))),
    };
    let device_data = device_data::read_all(&candidates).await;
    let uplinks = join_all(
        candidates
            .iter()
            .filter(|device| device_data.contains_key(&device.id()) && has_uplink(device))
            .map(|device| async move { (device.id(), ping(device).await) }),
    )
    .await
    .into_iter()
    .filter_map(|(id, ping)| Some((id, ping?)))
    .collect();
    NetworkCondition {
        lsdb_error,
        ..build(
            &candidates,
            edges.as_deref().unwrap_or_default(),
            &device_data,
            &uplinks,
        )
    }
}

fn has_uplink(device: &DeviceAccess) -> bool {
    device.interfaces().iter().any(|i| i.is_internet())
}

/// The pings to `device`'s uplink: none where it isn't reached or didn't answer, `Some(None)`
/// without a default route.
async fn ping(device: &DeviceAccess) -> Option<Option<UplinkPing>> {
    let accessible = device.connect(DeviceTarget::default()).await.ok()??;
    uplink::ping(accessible.reader()).await.ok()
}

/// The synchronous part of [`network_condition`]: testable without a device to read from.
pub(crate) fn build(
    candidates: &[DeviceAccess],
    edges: &[Edge],
    device_data: &BTreeMap<DeviceId, DeviceMapData>,
    uplinks: &Uplinks,
) -> NetworkCondition {
    let mut findings = Vec::new();
    let finding = |kind, device: &DeviceAccess| ConditionFinding {
        kind,
        device: device_ref(device),
        link: None,
        port: None,
        uptime: None,
        uplink: None,
    };
    let unreachable = candidates
        .iter()
        .filter(|device| !device_data.contains_key(&device.id()))
        .map(DeviceAccess::id)
        .collect::<BTreeSet<_>>();
    for device in candidates {
        let Some(data) = device_data.get(&device.id()) else {
            findings.push(finding(ConditionKind::Unreachable, device));
            continue;
        };
        if let Some(resources) = &data.resources
            && resources.uptime < RECENT_REBOOT
        {
            findings.push(ConditionFinding {
                uptime: Some(resources.uptime.into()),
                ..finding(ConditionKind::Rebooted, device)
            });
        }
        for feeding in poe::feeding(device, &data.poe) {
            if feeding.problem == Some(PoeProblem::NoLoad) {
                findings.push(ConditionFinding {
                    port: Some(feeding.port),
                    ..finding(ConditionKind::PoeNoLoad, device)
                });
            }
        }
        match uplinks.get(&device.id()) {
            Some(None) => findings.push(finding(ConditionKind::NoDefaultRoute, device)),
            Some(Some(ping)) if ping.received < ping.sent || ping.sent == 0 => {
                findings.push(ConditionFinding {
                    uplink: Some(ping.into()),
                    ..finding(ConditionKind::UplinkLoss, device)
                })
            }
            _ => {}
        }
    }
    let topology = candidates.first().map(|d| d.topology().clone());
    for edge in edges {
        let kind = match edge.state {
            // a router gone: its finding says it
            EdgeState::Down(DownReason::RouterMissing(device)) if unreachable.contains(&device) => {
                continue;
            }
            EdgeState::Down(_) => ConditionKind::LinkDown,
            EdgeState::V3Down => ConditionKind::LinkIpv6Down,
            EdgeState::Up | EdgeState::NotOspf => continue,
        };
        let interface = |id| topology.as_ref()?.get_interface_by_id(id);
        let Some(a) = interface(edge.a) else {
            continue;
        };
        let Some(b) = interface(edge.b) else {
            continue;
        };
        let Some(device) = a.device() else {
            continue;
        };
        findings.push(ConditionFinding {
            link: Some(ConditionLink {
                kind: match edge.kind {
                    EdgeKind::Cable => MapEdgeKind::Cable,
                    EdgeKind::Radio => MapEdgeKind::Radio,
                },
                a: interface_ref(&a),
                b: interface_ref(&b),
            }),
            ..finding(kind, &device)
        });
    }
    findings.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.device.name.cmp(&b.device.name))
            .then_with(|| {
                let port = |f: &ConditionFinding| f.port.as_ref().map(|p| p.name.clone());
                port(a).cmp(&port(b))
            })
    });
    NetworkCondition {
        findings: findings.into(),
        devices: details_nodes(candidates, device_data).into(),
        lsdb_error: None,
    }
}

#[cfg(test)]
mod test;
