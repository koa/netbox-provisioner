//! Scope: The site's check of what its devices see at their ports; not showing the findings
//! (`components::neighbors`).
//!
//! What the devices of a site see at their ports against NetBox's cables there
//! (`AccessibleDevice.neighborFindings`): read on a click, every device at once, each shown as
//! it answers, with when it was read. Subscribed afterwards: a cable taken over in NetBox drops
//! its finding without reading the device again. A click again reads the devices anew
//! (`refreshNeighbors`). A missing cable whose two ends NetBox has can be taken over: the
//! helper „Verbindung“ opens below it with those ends.
use crate::target::SelectedTarget;
use crate::{
    components::{
        findings::severity_label,
        neighbors,
        netbox_helper::{HelperPreset, LinkPreset, NetboxHelper},
    },
    error::FrontendError,
    graphql::{
        authenticated::neighbor_findings::{
            NeighborFinding as Finding, NeighborFindingKind as Kind, NeighborFindings,
            NeighborFindingsVariables, RefreshNeighbors, RefreshNeighborsVariables,
        },
        live::{Subscription, watch_data},
        request,
    },
    i18n::{device::as_of, neighbors as t},
    reading_time::{local_time, read_at, same_reading},
};
use cynic::{MutationBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize,
};
use std::collections::HashSet;
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct NeighborCheckProps {
    /// id and name of each device to read
    pub devices: Vec<(u32, Box<str>)>,
}

pub enum DeviceResult {
    Pending,
    Unreachable,
    Failed(FrontendError),
    Done {
        netbox: String,
        findings: Vec<Finding>,
        /// when the device was read (milliseconds since the epoch)
        read_at: f64,
    },
}

pub struct NeighborCheck {
    /// per device of the props, once started
    results: Option<Vec<DeviceResult>>,
    /// the findings (device, finding) whose cable is being taken over
    taking_over: HashSet<(usize, usize)>,
    /// per device, once started: answers again when NetBox or the device changed
    subscriptions: Vec<Subscription>,
}

pub enum NeighborCheckMsg {
    Start,
    /// the devices' neighbors read before are dropped: subscribe, reading them anew
    Subscribe,
    Result(usize, DeviceResult),
    TakeOver(usize, usize),
}

impl Component for NeighborCheck {
    type Message = NeighborCheckMsg;
    type Properties = NeighborCheckProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            results: None,
            taking_over: HashSet::new(),
            subscriptions: Vec::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NeighborCheckMsg::Start => {
                let devices = &ctx.props().devices;
                self.results = Some(devices.iter().map(|_| DeviceResult::Pending).collect());
                self.taking_over.clear();
                self.subscriptions.clear();
                let scope = ctx.link().clone();
                let ids = devices.iter().map(|(id, _)| *id as i32).collect();
                spawn_local(async move {
                    let refresh = RefreshNeighbors::build(RefreshNeighborsVariables { ids });
                    let _ = request(scope.clone(), refresh).await;
                    scope.send_message(NeighborCheckMsg::Subscribe);
                });
            }
            NeighborCheckMsg::Subscribe => {
                self.subscriptions = ctx
                    .props()
                    .devices
                    .iter()
                    .enumerate()
                    .map(|(index, (id, _))| {
                        let scope = ctx.link().clone();
                        let variables = NeighborFindingsVariables { id: *id as i32 };
                        watch_data(
                            ctx.link(),
                            NeighborFindings::build(variables),
                            move |data| {
                                let result = match data {
                                    Ok(data) => {
                                        match data.topology.device_by_id.and_then(|d| d.access) {
                                            Some(access) => DeviceResult::Done {
                                                netbox: data.topology.netbox_url,
                                                findings: access.neighbor_findings,
                                                read_at: read_at(access.neighbors_age),
                                            },
                                            None => DeviceResult::Unreachable,
                                        }
                                    }
                                    Err(error) => DeviceResult::Failed(error),
                                };
                                scope.send_message(NeighborCheckMsg::Result(index, result));
                            },
                        )
                    })
                    .collect();
                return false;
            }
            NeighborCheckMsg::TakeOver(device, finding) => {
                self.taking_over.insert((device, finding));
            }
            NeighborCheckMsg::Result(index, mut result) => {
                if let Some(slot) = self.results.as_mut().and_then(|r| r.get_mut(index)) {
                    if let (
                        DeviceResult::Done {
                            read_at: before, ..
                        },
                        DeviceResult::Done { read_at, .. },
                    ) = (&*slot, &mut result)
                    {
                        *read_at = same_reading(Some(*before), *read_at);
                    }
                    *slot = result;
                }
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let running = self
            .results
            .as_ref()
            .is_some_and(|r| r.iter().any(|r| matches!(r, DeviceResult::Pending)));
        let label = if self.results.is_some() {
            t::AGAIN
        } else {
            t::START
        };
        let onclick = ctx.link().callback(|_| NeighborCheckMsg::Start);
        let results = self.results.as_ref().map(|results| {
            html! {
                <dl class="neighbor-results">
                    { for ctx.props().devices.iter().zip(results).enumerate().map(|(index, ((id, name), result))| html! {
                        <>
                            <dt>{name.to_string()}</dt>
                            <dd>{self.device_result(ctx, index, *id, result)}</dd>
                        </>
                    }) }
                </dl>
            }
        });
        html! {
            <Card class="findings">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{t::INTRO}</p>
                    <Button variant={ButtonVariant::Secondary} disabled={running} {onclick}>{label}</Button>
                    {results}
                </CardBody>
            </Card>
        }
    }
}

impl NeighborCheck {
    fn device_result(
        &self,
        ctx: &Context<Self>,
        device: usize,
        device_id: u32,
        result: &DeviceResult,
    ) -> Html {
        match result {
            DeviceResult::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
            DeviceResult::Unreachable => html!({ t::UNREACHABLE }),
            DeviceResult::Failed(error) => error.to_html(),
            DeviceResult::Done {
                findings, read_at, ..
            } if findings.is_empty() => {
                html!(<>{ t::IN_LINE }{" · "}{as_of(&local_time(*read_at))}</>)
            }
            DeviceResult::Done {
                netbox,
                findings,
                read_at,
            } => {
                html! {
                    <>
                    <div>{as_of(&local_time(*read_at))}</div>
                    <ul class="finding-list">
                        { for findings.iter().enumerate().map(|(index, finding)| {
                            let preset = link_preset(&finding.kind);
                            let open = self.taking_over.contains(&(device, index));
                            html! {
                                <li>
                                    {severity_label(finding.severity)}
                                    <span>{neighbors::describe(netbox, &finding.kind)}</span>
                                    if let Some(preset) = preset {
                                        if open {
                                            <NetboxHelper id={device_id} target={SelectedTarget::default()} preset={HelperPreset::Link(preset)}/>
                                        } else {
                                            {" "}
                                            <Button
                                                variant={ButtonVariant::Link}
                                                onclick={ctx.link().callback(move |_| NeighborCheckMsg::TakeOver(device, index))}
                                            >{t::TAKE_OVER}</Button>
                                        }
                                    }
                                </li>
                            }
                        }) }
                    </ul>
                    </>
                }
            }
        }
    }
}

/// A missing cable between two ports NetBox has, the device seeing one neighbor there: the
/// ends for the helper „Verbindung“.
fn link_preset(kind: &Kind) -> Option<LinkPreset> {
    let Kind::CableMissing(missing) = kind else {
        return None;
    };
    let port = missing.port.interface.as_ref()?;
    let [seen] = missing.seen.as_slice() else {
        return None;
    };
    let peer = seen.interface.as_ref()?;
    Some(LinkPreset {
        port: port.id as u32,
        peer_device: peer.device.id as u32,
        peer: peer.id as u32,
    })
}
