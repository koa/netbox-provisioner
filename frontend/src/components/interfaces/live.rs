//! The live checks of the rows (`AccessibleDevice.interfaceLive`): who the device sees at a
//! row's ports, its OSPF neighbors there, the bridge devices of a radio link answering. Read
//! after the view, in a subscription of their own (a job on the device or a change in NetBox
//! brings them anew), with the time they were read; the neighbor findings are those of
//! `neighborFindings`, so `components::neighbor_check` words them.
use super::{
    Interfaces, InterfacesMsg,
    rows::{self, Action},
};
use crate::{
    components::{
        adjust_target::SelectedTarget,
        neighbor_check::{neighbor_finding_list, seen_list},
    },
    error::FrontendError,
    graphql::{
        authenticated::{
            interface_live::{
                BridgeAnswer, DeviceOnlyAddress as DeviceOnly, DeviceOnlyLease,
                InterfaceLiveSubscription, InterfaceLiveVariables, OspfNeighbor, PoeMismatch,
                RefreshInterfaceLive, RefreshInterfaceLiveVariables,
            },
            neighbor_findings::{NeighborFinding, SeenNeighbor as Seen},
            types::{AdhocCredentials, DeviceRef as Device, PoeProblem, Severity},
        },
        live::{Subscription, read_at, watch},
        request,
    },
    i18n::interfaces as t,
};
use cynic::{MutationBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{Icon, Spinner, SpinnerSize};
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
    /// when the device was read (milliseconds since the epoch)
    pub read_at: f64,
    /// by the row's interface
    pub rows: HashMap<i32, Rc<LiveRow>>,
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
    pub ospf: Vec<OspfNeighbor>,
    pub ospf_missing: Vec<Device>,
    pub bridges: Vec<BridgeAnswer>,
    pub findings: Vec<NeighborFinding>,
    /// the addresses only the device has there: the next apply removes them
    pub device_only: Vec<DeviceOnly>,
    /// the static leases only the device has there: the next apply removes them
    pub device_only_leases: Vec<DeviceOnlyLease>,
    /// its ports whose PoE mode in NetBox the device contradicts
    pub poe: Vec<PoeMismatch>,
    /// what the findings suggest, as the row's actions
    pub actions: Vec<Action>,
}

impl Live {
    /// The checks of the row of `interface`.
    pub fn row(&self, interface: i32) -> RowLive {
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
    /// device not answering are errors, an address or lease only the device has and a PoE mode
    /// it contradicts a warning.
    pub fn severities(&self) -> Vec<Severity> {
        self.findings
            .iter()
            .map(|finding| finding.severity)
            .chain(self.ospf_missing.iter().map(|_| Severity::Error))
            .chain(
                self.bridges
                    .iter()
                    .filter(|bridge| bridge.address.is_some() && !bridge.answers)
                    .map(|_| Severity::Error),
            )
            .chain(self.device_only.iter().map(|_| Severity::Warning))
            .chain(self.device_only_leases.iter().map(|_| Severity::Warning))
            .chain(self.poe.iter().map(|_| Severity::Warning))
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
            if !row.poe.is_empty() {
                <dt>{t::POE}</dt>
                <dd>
                    { for row.poe.iter().map(|m| html! {
                        <div>
                            <span class="live-failed">{Icon::ExclamationTriangle}</span>{" "}
                            {t::poe_mismatch(
                                &m.port.name,
                                match m.problem {
                                    PoeProblem::NoPoeOut => t::PoeMismatch::NoPoeOut,
                                    PoeProblem::NoLoad => t::PoeMismatch::NoLoad,
                                    PoeProblem::PdMissing => t::PoeMismatch::PdMissing,
                                    PoeProblem::FeedsInstead => t::PoeMismatch::FeedsInstead,
                                    // `mismatches()` never produces this (helper::poe::feeding,
                                    // for the network map); unreachable here, but the enum is
                                    // shared and must still match exhaustively.
                                    PoeProblem::Undocumented | PoeProblem::Other => t::PoeMismatch::Other,
                                },
                                m.status.as_deref(),
                            )}
                        </div>
                    }) }
                </dd>
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

/// Subscribes to the live checks for the card.
pub fn subscribe_live(scope: &Scope<Interfaces>, id: u32, target: SelectedTarget) -> Subscription {
    let (credential_name, adhoc_credentials) = target
        .credentials
        .variables(|username, password| AdhocCredentials { username, password });
    let variables = InterfaceLiveVariables {
        id: id as i32,
        target: target.address.map(|a| a.to_string()),
        credential_name,
        adhoc_credentials,
    };
    let to = scope.clone();
    watch(scope, InterfaceLiveSubscription::build(variables), move |response| {
        // `access` is null where the device can't be reached, the error says why
        let live = match response {
            Ok(response) => match response.data.and_then(|data| Checked::of(data, id)) {
                Some(checked) => Live::Checked(Rc::new(checked)),
                None => Live::Unchecked(
                    FrontendError::Graphql(response.errors.unwrap_or_default()).message(),
                ),
            },
            Err(error) => Live::Unchecked(error.message()),
        };
        to.send_message(InterfacesMsg::Live(live));
    })
}

/// Drops the device's live checks: the subscription reads them anew.
pub fn check_again(scope: Scope<Interfaces>, id: u32) {
    spawn_local(async move {
        let refresh = RefreshInterfaceLive::build(RefreshInterfaceLiveVariables { id: id as i32 });
        if let Err(error) = request(scope.clone(), refresh).await {
            scope.send_message(InterfacesMsg::Live(Live::Unchecked(error.message())));
        }
    });
}

impl Checked {
    /// `None` where the device has no access (not reachable).
    fn of(data: InterfaceLiveSubscription, device_id: u32) -> Option<Checked> {
        let access = data.topology.device_by_id?.access?;
        let live = access.interface_live;
        let rows = live
            .rows
            .into_iter()
            .map(|row| {
                let live = LiveRow {
                    seen: row.seen,
                    ospf: row.ospf,
                    ospf_missing: row.ospf_missing,
                    bridges: row.bridges,
                    findings: row.findings,
                    device_only: row.device_only_addresses,
                    device_only_leases: row.device_only_leases,
                    poe: row.poe,
                    actions: row
                        .actions
                        .into_iter()
                        .filter_map(|action| rows::action(action, device_id))
                        .collect(),
                };
                (row.interface, Rc::new(live))
            })
            .collect();
        Some(Checked {
            read_at: read_at(access.interface_live_age),
            rows,
            other_findings: live.other_findings,
            other_addresses: live.other_addresses,
            other_leases: live.other_leases,
        })
    }
}
