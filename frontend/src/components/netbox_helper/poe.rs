//! „PoE“: the direction and both ports (`poeOptions`), or what the device reports feeding
//! (`access.poeObserved`).
use super::{Choice, NetEntry, NetboxHelper, t};
use crate::{
    components::{adjust_target::SelectedTarget, menu::popup::Select},
    error::FrontendError,
    graphql::{
        authenticated::{
            PoeObserved, PoeOptions, poe_observed,
            poe_options::{self, PoePortFields},
        },
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Button, ButtonVariant, Form, FormGroup, Spinner, SpinnerSize};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

pub enum PoeMsg {
    Options(Result<PoeOptionsData, FrontendError>),
    Feeds(bool),
    Port(Choice),
    PeerDevice(Choice),
    Peer(Choice),
    Observe,
    Observed(Result<Vec<Observed>, FrontendError>),
    /// what the device reports: its port feeds the other end (device, port) if known
    Take(u32, Option<(u32, u32)>),
}

/// What „PoE“ is choosing: whether the device feeds (else it is fed), its port, the other end.
#[derive(Debug, Clone, PartialEq)]
pub struct PoeChoice {
    pub(super) feeds: bool,
    pub(super) port: Option<u32>,
    pub(super) peer_device: Option<u32>,
    pub(super) peer: Option<u32>,
}

impl Default for PoeChoice {
    fn default() -> Self {
        Self {
            feeds: true,
            port: None,
            peer_device: None,
            peer: None,
        }
    }
}

impl NetboxHelper {
    pub(super) fn update_poe(&mut self, ctx: &Context<Self>, msg: PoeMsg) -> bool {
        match msg {
            PoeMsg::Options(options) => self.poe_options = Some(options),
            PoeMsg::Feeds(feeds) => {
                self.poe.feeds = feeds;
                self.choose(ctx);
            }
            PoeMsg::Port(port) => {
                self.poe.port = Some(port.id);
                // the other end of its cable
                let far = self
                    .poe_port(port.id)
                    .and_then(|p| p.connected.first())
                    .map(|far| (far.device.id as u32, far.id as u32));
                if let Some((device, far)) = far {
                    self.poe.peer_device = Some(device);
                    self.poe.peer = Some(far);
                }
                self.choose(ctx);
            }
            PoeMsg::PeerDevice(device) => {
                if self.poe.peer_device != Some(device.id) {
                    self.poe.peer = None;
                }
                self.poe.peer_device = Some(device.id);
                self.choose(ctx);
            }
            PoeMsg::Peer(port) => {
                self.poe.peer = Some(port.id);
                self.choose(ctx);
            }
            PoeMsg::Observe => {
                self.poe_observing = true;
                fetch_poe_observed(ctx.link().clone(), ctx.props().id, ctx.props().target.clone());
            }
            PoeMsg::Observed(observed) => {
                self.poe_observing = false;
                self.poe_observed = Some(observed);
            }
            PoeMsg::Take(port, far) => {
                self.poe = PoeChoice {
                    feeds: true,
                    port: Some(port),
                    peer_device: far.map(|(device, _)| device),
                    peer: far.map(|(_, port)| port),
                };
                self.choose(ctx);
            }
        }
        true
    }
    /// A port of the device or of another device of „PoE“.
    pub(super) fn poe_port(&self, id: u32) -> Option<&PoePortFields> {
        let options = self.poe_options.as_ref()?.as_ref().ok()?;
        options
            .ports
            .iter()
            .chain(options.peers.iter().flat_map(|peer| &peer.ports))
            .find(|port| port.id as u32 == id)
    }
    pub(super) fn poe_form(&self, ctx: &Context<Self>) -> Html {
        let options = match &self.poe_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => options,
        };
        let choices = |ports: &[PoePortFields]| -> Vec<Choice> {
            ports
                .iter()
                .map(|port| Choice {
                    id: port.id as u32,
                    text: t::poe_port(
                        &port.name,
                        port.label.as_deref(),
                        port.poe.as_ref().map(poe_mode),
                        port.connected
                            .first()
                            .map(|far| (far.device.name.as_str(), far.name.as_str())),
                        port.cabled,
                    ),
                })
                .collect()
        };
        let directions = vec![
            NetEntry { value: true, text: t::POE_FEEDS.to_string() },
            NetEntry { value: false, text: t::POE_FED.to_string() },
        ];
        let direction = directions.iter().find(|d| d.value == self.poe.feeds).cloned();
        let own = choices(&options.ports);
        let devices: Vec<Choice> = options
            .peers
            .iter()
            .map(|peer| Choice { id: peer.id as u32, text: peer.name.clone() })
            .collect();
        let peer_ports = self
            .poe
            .peer_device
            .and_then(|id| options.peers.iter().find(|peer| peer.id as u32 == id))
            .map(|peer| choices(&peer.ports))
            .unwrap_or_default();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let observed = match &self.poe_observed {
            _ if self.poe_observing => html!(<Spinner size={SpinnerSize::Md}/>),
            None => Html::default(),
            Some(Err(error)) => error.to_html(),
            Some(Ok(observed)) if observed.is_empty() => html!(<p>{t::POE_OBSERVED_NONE}</p>),
            Some(Ok(observed)) => html! {
                <ul class="poe-observed">
                { for observed.iter().map(|o| {
                    let far = o.far.as_ref();
                    // NetBox has it as the device does: the cable, PSE here and PD there
                    let mode = |id: i64| self.poe_port(id as u32).and_then(|p| p.poe.clone());
                    let recorded = !o.from_neighbor
                        && o.port.interface.as_ref().is_some_and(|port| mode(port.id) == Some(poe_options::PoeMode::PSE))
                        && far.is_some_and(|far| mode(far.id) == Some(poe_options::PoeMode::PD));
                    let text = t::poe_observed(
                        &o.port.name,
                        far.map(|far| (far.device.name.as_str(), far.name.as_str())),
                        o.from_neighbor,
                        recorded,
                    );
                    let take = o.port.interface.as_ref().filter(|_| !recorded).map(|port| {
                        let (port, far) = (
                            port.id as u32,
                            far.map(|far| (far.device.id as u32, far.id as u32)),
                        );
                        html! {
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::POE_TAKE}
                                onclick={ctx.link().callback(move |_| PoeMsg::Take(port, far))}
                            />
                        }
                    });
                    html!(<li>{text}{" "}{take}</li>)
                })}
                </ul>
            },
        };
        html! {<>
            <div class="helper-actions">
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::POE_OBSERVE}
                    disabled={self.poe_observing}
                    onclick={ctx.link().callback(|_| PoeMsg::Observe)}
                />
            </div>
            {observed}
            <Form>
                <FormGroup label={t::POE_DIRECTION}>
                    <Select<NetEntry<bool>>
                        selected={direction}
                        entries={directions}
                        onselect={ctx.link().callback(|entry: NetEntry<bool>| PoeMsg::Feeds(entry.value))}
                    />
                </FormGroup>
                <FormGroup label={t::PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PORT}
                        selected={selected(&own, self.poe.port)}
                        entries={own.clone()}
                        onselect={ctx.link().callback(PoeMsg::Port)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_DEVICE}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_DEVICE}
                        selected={selected(&devices, self.poe.peer_device)}
                        entries={devices.clone()}
                        onselect={ctx.link().callback(PoeMsg::PeerDevice)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PEER_PORT}
                        selected={selected(&peer_ports, self.poe.peer)}
                        entries={peer_ports.clone()}
                        onselect={ctx.link().callback(PoeMsg::Peer)}
                    />
                </FormGroup>
            </Form>
        </>}
    }
}

pub(super) fn fetch_poe_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<PoeOptions, _>(
            scope.clone(),
            poe_options::Variables { id: id as i64 },
        )
        .await
        .map(|data| match data.topology.device_by_id {
            Some(device) => device.poe_options,
            None => PoeOptionsData {
                ports: Vec::new(),
                peers: Vec::new(),
            },
        });
        scope.send_message(PoeMsg::Options(options));
    });
}

/// What the device reports feeding, reached as the page reaches it.
pub(super) fn fetch_poe_observed(scope: Scope<NetboxHelper>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| {
                poe_observed::AdhocCredentials { username, password }
            });
        let observed = query_authenticated::<PoeObserved, _>(
            scope.clone(),
            poe_observed::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .and_then(|device| device.access)
                .map(|access| access.poe_observed)
                .unwrap_or_default()
        });
        scope.send_message(PoeMsg::Observed(observed));
    });
}

pub(super) type PoeOptionsData = poe_options::PoeOptionsTopologyDeviceByIdPoeOptions;

pub(super) type Observed = poe_observed::PoeObservedTopologyDeviceByIdAccessPoeObserved;

/// NetBox' PoE mode as the UI shows it.
fn poe_mode(mode: &poe_options::PoeMode) -> &str {
    match mode {
        poe_options::PoeMode::PSE => "PSE",
        poe_options::PoeMode::PD => "PD",
        poe_options::PoeMode::Other(other) => other,
    }
}
