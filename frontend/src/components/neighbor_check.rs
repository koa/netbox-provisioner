//! What the devices of a site see at their ports against NetBox's cables there
//! (`AccessibleDevice.neighborFindings`): read on a click, every device at once, each shown as
//! it answers. A missing cable whose two ends NetBox has can be taken over: the helper
//! „Verbindung“ opens below it with those ends.
use crate::{
    components::{
        adjust_target::SelectedTarget,
        findings::{Level, level_label, netbox_link},
        netbox_helper::{HelperPreset, LinkPreset, NetboxHelper},
    },
    error::FrontendError,
    graphql::{
        authenticated::{
            NeighborFindings,
            neighbor_findings::{
                self, Interface, NeighborFindingsTopologyDeviceByIdAccessNeighborFindings as Finding,
                NeighborFindingsTopologyDeviceByIdAccessNeighborFindingsKind as Kind, Port, Seen,
                Severity,
            },
        },
        query_authenticated,
    },
    i18n::neighbors as t,
};
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
    Done { netbox: String, findings: Vec<Finding> },
}

pub struct NeighborCheck {
    /// per device of the props, once started
    results: Option<Vec<DeviceResult>>,
    /// the findings (device, finding) whose cable is being taken over
    taking_over: HashSet<(usize, usize)>,
}

pub enum NeighborCheckMsg {
    Start,
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
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NeighborCheckMsg::Start => {
                let devices = &ctx.props().devices;
                self.results = Some(devices.iter().map(|_| DeviceResult::Pending).collect());
                self.taking_over.clear();
                for (index, (id, _)) in devices.iter().enumerate() {
                    let scope = ctx.link().clone();
                    let id = i64::from(*id);
                    spawn_local(async move {
                        let result = match query_authenticated::<NeighborFindings, _>(
                            scope.clone(),
                            neighbor_findings::Variables { id },
                        )
                        .await
                        {
                            Ok(data) => match data.topology.device_by_id.and_then(|d| d.access) {
                                Some(access) => DeviceResult::Done {
                                    netbox: data.netbox_url,
                                    findings: access.neighbor_findings,
                                },
                                None => DeviceResult::Unreachable,
                            },
                            Err(error) => DeviceResult::Failed(error),
                        };
                        scope.send_message(NeighborCheckMsg::Result(index, result));
                    });
                }
            }
            NeighborCheckMsg::TakeOver(device, finding) => {
                self.taking_over.insert((device, finding));
            }
            NeighborCheckMsg::Result(index, result) => {
                if let Some(slot) = self.results.as_mut().and_then(|r| r.get_mut(index)) {
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
            DeviceResult::Done { findings, .. } if findings.is_empty() => html!({ t::IN_LINE }),
            DeviceResult::Done { netbox, findings } => {
                let links = Links { netbox };
                html! {
                    <ul class="finding-list">
                        { for findings.iter().enumerate().map(|(index, finding)| {
                            let preset = link_preset(&finding.kind);
                            let open = self.taking_over.contains(&(device, index));
                            html! {
                                <li>
                                    {level_label(level(&finding.severity))}
                                    <span>{links.describe(&finding.kind)}</span>
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

/// Neighbor findings as a list, the worst first (the interface view's live checks); without
/// taking a cable over, the row's actions do that.
pub fn neighbor_finding_list(netbox: &str, findings: &[Finding]) -> Html {
    let links = Links { netbox };
    html! {
        <ul class="finding-list">
            { for findings.iter().map(|finding| html! {
                <li>
                    {level_label(level(&finding.severity))}
                    <span>{links.describe(&finding.kind)}</span>
                </li>
            }) }
        </ul>
    }
}

/// Who the device sees, linked into NetBox.
pub fn seen_list(netbox: &str, seen: &[Seen]) -> Html {
    Links { netbox }.seen(seen)
}

fn level(severity: &Severity) -> Level {
    match severity {
        Severity::ERROR => Level::Error,
        Severity::WARNING => Level::Warning,
        Severity::HINT | Severity::Other(_) => Level::Hint,
    }
}

struct Links<'a> {
    netbox: &'a str,
}

impl Links<'_> {
    fn interface(&self, interface: &Interface) -> Html {
        netbox_link(
            self.netbox,
            &format!("dcim/interfaces/{}/", interface.id),
            &format!("{} {}", interface.device.name, interface.name),
        )
    }
    /// the port as the device names it, linked to its NetBox interface where there is one
    fn port(&self, port: &Port) -> Html {
        match &port.interface {
            Some(interface) => netbox_link(
                self.netbox,
                &format!("dcim/interfaces/{}/", interface.id),
                &port.name,
            ),
            None => html!({ port.name.clone() }),
        }
    }
    fn seen_one(&self, seen: &Seen) -> Html {
        match (&seen.interface, &seen.device) {
            (Some(interface), _) => netbox_link(
                self.netbox,
                &format!("dcim/interfaces/{}/", interface.id),
                &format!("{} {}", seen.identity, seen.interface_name),
            ),
            (None, Some(device)) => netbox_link(
                self.netbox,
                &format!("dcim/devices/{}/", device.id),
                &format!("{} {}", seen.identity, seen.interface_name),
            ),
            (None, None) => html!({ t::unknown(&seen.identity, &seen.interface_name, &seen.board) }),
        }
    }
    fn seen(&self, seen: &[Seen]) -> Html {
        t::seen(seen.iter().map(|s| self.seen_one(s)).collect())
    }

    fn describe(&self, kind: &Kind) -> Html {
        match kind {
            Kind::CableMissing(f) => t::cable_missing(self.port(&f.port), self.seen(&f.seen)),
            Kind::CableElsewhere(f) => t::cable_elsewhere(
                self.port(&f.port),
                self.interface(&f.far_end),
                self.seen(&f.seen),
            ),
            Kind::OtherFarPort(f) => t::other_far_port(
                self.port(&f.port),
                self.interface(&f.far_end),
                self.seen_one(&f.seen),
            ),
            Kind::NeighborNotSeen(f) => t::not_seen(self.port(&f.port), self.interface(&f.far_end)),
            Kind::NeighborsNotInNetbox(f) => {
                t::not_in_netbox(self.port(&f.port), self.seen(&f.seen))
            }
            Kind::DiscoveryOff(f) => html!({ t::discovery_off(&f.list) }),
        }
    }
}
