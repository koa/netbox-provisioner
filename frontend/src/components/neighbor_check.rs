//! What the devices of a site see at their ports against NetBox's cables there
//! (`AccessibleDevice.neighborFindings`): read on a click, every device at once, each shown as
//! it answers.
use crate::{
    components::findings::{Level, level_label, netbox_link},
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
}

pub enum NeighborCheckMsg {
    Start,
    Result(usize, DeviceResult),
}

impl Component for NeighborCheck {
    type Message = NeighborCheckMsg;
    type Properties = NeighborCheckProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { results: None }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NeighborCheckMsg::Start => {
                let devices = &ctx.props().devices;
                self.results = Some(devices.iter().map(|_| DeviceResult::Pending).collect());
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
                    { for ctx.props().devices.iter().zip(results).map(|((_, name), result)| html! {
                        <>
                            <dt>{name.to_string()}</dt>
                            <dd>{device_result(result)}</dd>
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

fn device_result(result: &DeviceResult) -> Html {
    match result {
        DeviceResult::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
        DeviceResult::Unreachable => html!({ t::UNREACHABLE }),
        DeviceResult::Failed(error) => error.to_html(),
        DeviceResult::Done { findings, .. } if findings.is_empty() => html!({ t::IN_LINE }),
        DeviceResult::Done { netbox, findings } => {
            let links = Links { netbox };
            html! {
                <ul class="finding-list">
                    { for findings.iter().map(|finding| html! {
                        <li>
                            {level_label(match finding.severity {
                                Severity::ERROR => Level::Error,
                                Severity::WARNING => Level::Warning,
                                Severity::HINT | Severity::Other(_) => Level::Hint,
                            })}
                            <span>{links.describe(&finding.kind)}</span>
                        </li>
                    }) }
                </ul>
            }
        }
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
