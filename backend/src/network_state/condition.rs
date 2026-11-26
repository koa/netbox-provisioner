//! Scope: The admins' page „Zustand“: what to check or maintain in the network, most urgent
//! first (`docs/concepts/monitoring.md`, C8); not reading the devices (`read`, `device_data`,
//! `device::uplink`).
//!
//! `Query.topology.networkCondition`: the findings of a site's, or every site's, OSPF routers
//! (the network map's candidates), in the order of the concept: a router the provisioner
//! doesn't reach, a dead link, a link up only in OSPFv2, an uplink losing pings or without a
//! default route, a WLAN client with a weak or bad signal (`wlan_signal`, Z64), a reboot in
//! the last 24 h, a PoE port contradicting NetBox (E8) or without load. Health and resources are shown, not judged (Z16).
//! Reads the LSDB (at most 5 s old), then each router, its WLAN clients and leases, then the
//! uplinks' pings (at most 30 s old, as the network map's details): one device's reads stay
//! sequential.

use super::{
    device_data::{self, DEVICE_DATA_AGE, DeviceMapData},
    edges::{DownReason, Edge, EdgeKind, EdgeState, edges},
    graphql::{MapEdgeKind, MapNodeDetails, details_nodes, supports_ospf},
    read::{read, readers},
};
use crate::{
    config::{CONFIG, SignalThresholds},
    device::{
        interface_live::client_name,
        metrics::{Lease, WlanClient},
        poe::{self, PoeProblem},
        uplink::{self, UplinkQuality, UplinkReading},
    },
    error_info::ErrorInfo,
    facts::Freshness,
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
    /// a router with an uplink (`uplink::has_uplink`) has no active default route
    NoDefaultRoute,
    /// pings to the uplink's gateway got lost
    UplinkLoss,
    /// a WLAN client's signal is below `wlan_signal.bad`
    SignalBad,
    /// a WLAN client's signal is below `wlan_signal.weak`
    SignalWeak,
    /// the router booted in the last 24 h
    Rebooted,
    /// the port contradicts NetBox's PoE mode (`ConditionFinding::poe`: no PoE out, the far
    /// end not `pd`, feeding where NetBox has it fed, feeding without a mode)
    PoeMismatch,
    /// NetBox has the port feed, nothing at the other end draws power (`PoeProblem::NoLoad`)
    PoeNoLoad,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConditionFinding {
    pub kind: ConditionKind,
    pub device: DeviceRef,
    /// a link's ends, `device`'s first
    pub link: Option<ConditionLink>,
    /// the PoE port without load or contradicting NetBox
    pub port: Option<InterfaceRef>,
    /// what the PoE port contradicts
    pub poe: Option<PoeProblem>,
    /// since the reboot
    pub uptime: Option<ScalarDuration>,
    pub uplink: Option<UplinkQuality>,
    /// the WLAN client with a weak or bad signal
    pub client: Option<ConditionClient>,
}

/// A WLAN client as the device it is registered at hears it.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConditionClient {
    pub mac_address: Box<str>,
    /// from a lease of any router by MAC
    pub hostname: Option<Box<str>>,
    pub ssid: Box<str>,
    /// the CAP it is on; empty for a radio of the device itself
    pub access_point: Box<str>,
    pub interface: Box<str>,
    /// dBm
    pub signal: i32,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConditionLink {
    pub kind: MapEdgeKind,
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

/// What each uplink router's reading found, those that answered.
pub type Uplinks = BTreeMap<DeviceId, UplinkReading>;

/// What the routers answered, those that did.
#[derive(Default)]
pub(crate) struct Readings {
    pub device_data: BTreeMap<DeviceId, DeviceMapData>,
    /// the WLAN clients registered at each
    pub clients: BTreeMap<DeviceId, Box<[WlanClient]>>,
    /// every router's bound leases, to name the clients
    pub leases: Box<[Lease]>,
    pub uplinks: Uplinks,
}

/// A router's WLAN clients and leases.
struct Wlan {
    clients: Box<[WlanClient]>,
    leases: Box<[Lease]>,
}

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
    let wlan = join_all(
        candidates
            .iter()
            .filter(|device| device_data.contains_key(&device.id()))
            .map(|device| async move { (device.id(), wlan(device).await) }),
    )
    .await;
    let mut clients = BTreeMap::new();
    let mut leases = Vec::new();
    for (id, wlan) in wlan {
        if let Some(wlan) = wlan {
            clients.insert(id, wlan.clients);
            leases.extend(wlan.leases);
        }
    }
    let uplinks = join_all(
        candidates
            .iter()
            .filter(|device| device_data.contains_key(&device.id()) && uplink::has_uplink(device))
            .map(|device| async move { (device.id(), ping(device).await) }),
    )
    .await
    .into_iter()
    .filter_map(|(id, ping)| Some((id, ping?)))
    .collect();
    let readings = Readings {
        device_data,
        clients,
        leases: leases.into(),
        uplinks,
    };
    NetworkCondition {
        lsdb_error,
        ..build(
            &candidates,
            edges.as_deref().unwrap_or_default(),
            &readings,
            CONFIG.wlan_signal(),
        )
    }
}

/// `device`'s WLAN clients, then its leases; none where it isn't reached or didn't answer.
async fn wlan(device: &DeviceAccess) -> Option<Wlan> {
    let accessible = device.connect(DeviceTarget::default()).await.ok()??;
    let freshness = Freshness::MaxAge(DEVICE_DATA_AGE);
    let clients = accessible.wlan_clients(freshness).await.ok()?;
    let leases = accessible.leases(freshness).await.ok()?;
    Some(Wlan { clients, leases })
}

/// A client's finding by the thresholds: bad, weak or none.
fn signal_kind(signal: i16, thresholds: SignalThresholds) -> Option<ConditionKind> {
    match signal {
        _ if signal < thresholds.bad => Some(ConditionKind::SignalBad),
        _ if signal < thresholds.weak => Some(ConditionKind::SignalWeak),
        _ => None,
    }
}

/// The reading of `device`'s uplink; none where it isn't reached or didn't answer.
async fn ping(device: &DeviceAccess) -> Option<UplinkReading> {
    let accessible = device.connect(DeviceTarget::default()).await.ok()??;
    let reader = accessible.reader().at(Freshness::MaxAge(DEVICE_DATA_AGE));
    uplink::ping(&reader).await.ok()
}

/// The synchronous part of [`network_condition`]: testable without a device to read from.
pub(crate) fn build(
    candidates: &[DeviceAccess],
    edges: &[Edge],
    readings: &Readings,
    thresholds: SignalThresholds,
) -> NetworkCondition {
    let device_data = &readings.device_data;
    let hostnames = readings
        .leases
        .iter()
        .filter_map(|lease| {
            let name = client_name(lease.hostname.as_deref()?)?;
            Some((lease.mac_address, name))
        })
        .collect::<BTreeMap<_, _>>();
    let mut findings = Vec::new();
    let finding = |kind, device: &DeviceAccess| ConditionFinding {
        kind,
        device: device_ref(device),
        link: None,
        port: None,
        uptime: None,
        uplink: None,
        client: None,
        poe: None,
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
            let kind = match feeding.problem {
                None => continue,
                Some(PoeProblem::NoLoad) => ConditionKind::PoeNoLoad,
                Some(_) => ConditionKind::PoeMismatch,
            };
            findings.push(ConditionFinding {
                port: Some(feeding.port),
                poe: feeding.problem,
                ..finding(kind, device)
            });
        }
        for client in readings.clients.get(&device.id()).into_iter().flatten() {
            if let Some(kind) = signal_kind(client.signal, thresholds) {
                findings.push(ConditionFinding {
                    client: Some(ConditionClient {
                        mac_address: client.mac_address.to_string().into(),
                        hostname: hostnames.get(&client.mac_address).cloned(),
                        ssid: client.ssid.clone(),
                        access_point: client.ap.clone(),
                        interface: client.interface.clone(),
                        signal: client.signal.into(),
                    }),
                    ..finding(kind, device)
                });
            }
        }
        match readings.uplinks.get(&device.id()) {
            Some(UplinkReading::NoDefaultRoute) => {
                findings.push(finding(ConditionKind::NoDefaultRoute, device))
            }
            Some(UplinkReading::Pinged(ping)) if ping.received < ping.sent || ping.sent == 0 => {
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
            .then_with(|| {
                let signal = |f: &ConditionFinding| f.client.as_ref().map(|c| c.signal);
                signal(a).cmp(&signal(b))
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
