//! The live checks of the rows (`AccessibleDevice.interfaceLive`): who the device sees at a
//! row's ports, its OSPF neighbors there, the bridge devices of a radio link answering. Read
//! after the view, in a query of their own; the neighbor findings come over as the type of the
//! query `NeighborFindings`, so `components::neighbor_check` words them.
use super::{
    Interfaces, InterfacesMsg,
    rows::{self, Action, Device},
};
use crate::{
    components::{
        adjust_target::SelectedTarget,
        neighbor_check::{neighbor_finding_list, seen_list},
    },
    error::FrontendError,
    graphql::{
        authenticated::{
            InterfaceLive,
            findings::Severity,
            interface_live, interface_view,
            neighbor_findings::{
                NeighborFindingsTopologyDeviceByIdAccessNeighborFindings as NeighborFinding, Seen,
                Severity as NeighborSeverity,
            },
        },
        query_authenticated_response,
    },
    i18n::interfaces as t,
};
use patternfly_yew::prelude::{Icon, Spinner, SpinnerSize};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, rc::Rc};
use yew::{Html, html, html::Scope, platform::spawn_local};

/// The live checks of the device.
#[derive(Debug, Clone, PartialEq)]
pub enum Live {
    Pending,
    /// the device couldn't be asked, and why
    Unchecked(String),
    Checked(Rc<Checked>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// by the row's interface
    pub rows: HashMap<i64, Rc<LiveRow>>,
    pub other_findings: Vec<NeighborFinding>,
    /// the addresses only the device has on an interface of no row
    pub other_addresses: Vec<DeviceOnly>,
    /// the static leases only the device has of a server on no row
    pub other_leases: Vec<DeviceOnlyLease>,
}

/// What a row's live checks say.
#[derive(Debug, Clone, PartialEq)]
pub enum RowLive {
    Pending,
    Unchecked,
    Checked(Rc<LiveRow>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveRow {
    pub seen: Vec<Seen>,
    pub ospf: Vec<Ospf>,
    pub ospf_missing: Vec<Device>,
    pub bridges: Vec<Bridge>,
    pub findings: Vec<NeighborFinding>,
    /// the addresses only the device has there: the next apply removes them
    pub device_only: Vec<DeviceOnly>,
    /// the static leases only the device has there: the next apply removes them
    pub device_only_leases: Vec<DeviceOnlyLease>,
    /// what the findings suggest, as the row's actions
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ospf {
    pub instance: String,
    pub router_id: String,
    pub state: String,
    pub adjacent: bool,
    pub device: Option<Device>,
}

/// An address only the device has, NetBox the address elsewhere or on no interface.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceOnly {
    pub address: String,
    pub interface_name: String,
    pub disabled: bool,
    pub elsewhere: Option<Device>,
    pub unassigned: bool,
}

/// A static lease only the device has, NetBox the address elsewhere or on no interface.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceOnlyLease {
    pub address: String,
    pub mac: Option<String>,
    pub client_id: Option<String>,
    pub name: Option<String>,
    pub disabled: bool,
    pub settings: Vec<String>,
    pub elsewhere: Option<Device>,
    pub unassigned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bridge {
    pub device: Device,
    pub address: Option<String>,
    pub answers: bool,
}

impl Live {
    /// The checks of the row of `interface`.
    pub fn row(&self, interface: i64) -> RowLive {
        match self {
            Live::Pending => RowLive::Pending,
            Live::Unchecked(_) => RowLive::Unchecked,
            Live::Checked(checked) => checked
                .rows
                .get(&interface)
                .cloned()
                .map_or(RowLive::Unchecked, RowLive::Checked),
        }
    }
}

impl LiveRow {
    /// Its failures as severities: the neighbor findings, a missing OSPF neighbor and a bridge
    /// device not answering are errors, an address or lease only the device has a warning.
    pub fn severities(&self) -> Vec<Severity> {
        self.findings
            .iter()
            .map(|finding| match finding.severity {
                NeighborSeverity::ERROR => Severity::ERROR,
                NeighborSeverity::WARNING => Severity::WARNING,
                NeighborSeverity::HINT | NeighborSeverity::Other(_) => Severity::HINT,
            })
            .chain(self.ospf_missing.iter().map(|_| Severity::ERROR))
            .chain(
                self.bridges
                    .iter()
                    .filter(|bridge| bridge.address.is_some() && !bridge.answers)
                    .map(|_| Severity::ERROR),
            )
            .chain(self.device_only.iter().map(|_| Severity::WARNING))
            .chain(self.device_only_leases.iter().map(|_| Severity::WARNING))
            .collect()
    }
}

/// The row's live checks in its opened view.
pub fn details(netbox: &str, live: &RowLive, device: impl Fn(&Device) -> Html) -> Html {
    let row = match live {
        RowLive::Pending => {
            return html!(<p class="live-pending"><Spinner size={SpinnerSize::Sm}/>{" "}{t::LIVE_PENDING}</p>);
        }
        RowLive::Unchecked => return html!(<p class="live-unchecked">{t::LIVE_UNCHECKED}</p>),
        RowLive::Checked(row) => row,
    };
    let ok = |good: bool| {
        if good {
            html!(<span class="live-ok">{Icon::CheckCircle}</span>)
        } else {
            html!(<span class="live-failed">{Icon::ExclamationCircle}</span>)
        }
    };
    html! {<>
        <dl class="interface-facts live-facts">
            <dt>{t::SEES}</dt>
            <dd>
                if row.seen.is_empty() {
                    {t::SEES_NOBODY}
                } else {
                    {seen_list(netbox, &row.seen)}
                }
            </dd>
            if !row.ospf.is_empty() || !row.ospf_missing.is_empty() {
                <dt>{t::OSPF}</dt>
                <dd>
                    { for row.ospf.iter().map(|n| html! {
                        <div>
                            {ok(n.adjacent)}{" "}
                            { match &n.device {
                                Some(d) => device(d),
                                None => html!({ &n.router_id }),
                            } }
                            {" "}{t::ospf_state(&n.instance, &n.state)}
                        </div>
                    }) }
                    { for row.ospf_missing.iter().map(|d| html! {
                        <div>{ok(false)}{" "}{t::OSPF_MISSING}{" "}{device(d)}</div>
                    }) }
                </dd>
            }
            if !row.bridges.is_empty() {
                <dt>{t::BRIDGES}</dt>
                <dd>
                    { for row.bridges.iter().map(|b| html! {
                        <div>
                            if let Some(address) = &b.address {
                                {ok(b.answers)}{" "}{device(&b.device)}{" "}
                                {t::bridge_answer(address, b.answers)}
                            } else {
                                {device(&b.device)}{" "}{t::BRIDGE_NO_ADDRESS}
                            }
                        </div>
                    }) }
                </dd>
            }
            if !row.device_only.is_empty() {
                <dt>{t::DEVICE_ONLY}</dt>
                <dd>{device_only_list(&row.device_only, &device)}</dd>
            }
            if !row.device_only_leases.is_empty() {
                <dt>{t::LEASES_DEVICE_ONLY}</dt>
                <dd>{device_only_lease_list(&row.device_only_leases, &device)}</dd>
            }
        </dl>
        if !row.findings.is_empty() {
            {neighbor_finding_list(netbox, &row.findings)}
        }
    </>}
}

/// Addresses only the device has, each with where NetBox has it instead.
pub fn device_only_list(addresses: &[DeviceOnly], device: impl Fn(&Device) -> Html) -> Html {
    html! {
        { for addresses.iter().map(|a| html! {
            <div>
                <span class="live-failed">{Icon::ExclamationTriangle}</span>{" "}
                {t::device_only(&a.address, &a.interface_name, a.disabled)}
                if let Some(elsewhere) = &a.elsewhere {
                    {" "}{t::DEVICE_ONLY_ELSEWHERE}{" "}{device(elsewhere)}
                } else if a.unassigned {
                    {" "}{t::DEVICE_ONLY_UNASSIGNED}
                }
            </div>
        }) }
    }
}

/// Static leases only the device has, each with what a takeover can't keep and where NetBox
/// has the address instead.
pub fn device_only_lease_list(leases: &[DeviceOnlyLease], device: impl Fn(&Device) -> Html) -> Html {
    html! {
        { for leases.iter().map(|l| html! {
            <div>
                <span class="live-failed">{Icon::ExclamationTriangle}</span>{" "}
                {t::device_only_lease(
                    &l.address,
                    l.mac.as_deref().or(l.client_id.as_deref()).unwrap_or_default(),
                    l.name.as_deref(),
                    l.disabled,
                )}
                if !l.settings.is_empty() {
                    {" "}{t::lease_settings_lost(&l.settings)}
                }
                if let Some(elsewhere) = &l.elsewhere {
                    {" "}{t::DEVICE_ONLY_ELSEWHERE}{" "}{device(elsewhere)}
                } else if l.unassigned {
                    {" "}{t::DEVICE_ONLY_UNASSIGNED}
                } else if l.mac.is_none() {
                    {" "}{t::LEASE_BY_CLIENT_ID}
                }
            </div>
        }) }
    }
}

/// Reads the live checks for the card.
pub fn fetch(scope: Scope<Interfaces>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) = target
            .credentials
            .variables(|username, password| interface_live::AdhocCredentials { username, password });
        let response = query_authenticated_response::<InterfaceLive, _>(
            scope.clone(),
            interface_live::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await;
        // `access` is null where the device can't be reached, the error says why
        let live = response.and_then(|response| {
            match response.data.map(|data| Checked::of(data, id)).transpose()?.flatten() {
                Some(checked) => Ok(Live::Checked(Rc::new(checked))),
                None => Ok(Live::Unchecked(
                    FrontendError::Graphql(response.errors.unwrap_or_default()).message(),
                )),
            }
        });
        scope.send_message(InterfacesMsg::Live(
            live.unwrap_or_else(|error| Live::Unchecked(error.message())),
        ));
    });
}

impl Checked {
    /// `None` where the device has no access (not reachable).
    fn of(data: interface_live::ResponseData, device_id: u32) -> Result<Option<Checked>, FrontendError> {
        let Some(live) = data
            .topology
            .device_by_id
            .and_then(|device| device.access)
            .map(|access| access.interface_live)
        else {
            return Ok(None);
        };
        let rows = live
            .rows
            .into_iter()
            .map(|row| {
                let live = LiveRow {
                    seen: convert(&row.seen)?,
                    ospf: row
                        .ospf
                        .into_iter()
                        .map(|n| Ospf {
                            instance: n.instance,
                            router_id: n.router_id,
                            state: n.state,
                            adjacent: n.adjacent,
                            device: n.device.map(device),
                        })
                        .collect(),
                    ospf_missing: row.ospf_missing.into_iter().map(device).collect(),
                    bridges: row
                        .bridges
                        .into_iter()
                        .map(|b| Bridge {
                            device: device(b.device),
                            address: b.address,
                            answers: b.answers,
                        })
                        .collect(),
                    findings: convert(&row.findings)?,
                    device_only: row.device_only_addresses.into_iter().map(device_only).collect(),
                    device_only_leases: row.device_only_leases.into_iter().map(device_only_lease).collect(),
                    actions: convert::<_, interface_view::Action>(&row.actions)?
                        .into_iter()
                        .filter_map(|action| rows::action(action, device_id))
                        .collect(),
                };
                Ok((row.interface, Rc::new(live)))
            })
            .collect::<Result<_, FrontendError>>()?;
        Ok(Some(Checked {
            rows,
            other_findings: convert(&live.other_findings)?,
            other_addresses: live.other_addresses.into_iter().map(device_only).collect(),
            other_leases: live.other_leases.into_iter().map(device_only_lease).collect(),
        }))
    }
}

/// The same selection as another query's type.
fn convert<F: Serialize, T: DeserializeOwned>(items: &[F]) -> Result<Vec<T>, FrontendError> {
    items
        .iter()
        .map(|item| Ok(serde_json::from_value(serde_json::to_value(item)?)?))
        .collect()
}

fn device_only(address: interface_live::DeviceOnly) -> DeviceOnly {
    DeviceOnly {
        address: address.address,
        interface_name: address.interface_name,
        disabled: address.disabled,
        elsewhere: address.elsewhere.map(device),
        unassigned: address.unassigned,
    }
}

fn device_only_lease(lease: interface_live::DeviceOnlyLease) -> DeviceOnlyLease {
    DeviceOnlyLease {
        address: lease.address,
        mac: lease.mac,
        client_id: lease.client_id,
        name: lease.name,
        disabled: lease.disabled,
        settings: lease.settings,
        elsewhere: lease.elsewhere.map(device),
        unassigned: lease.unassigned,
    }
}

fn device(device: interface_live::Device) -> Device {
    Device {
        id: device.id,
        name: device.name,
        site: device.site,
    }
}
