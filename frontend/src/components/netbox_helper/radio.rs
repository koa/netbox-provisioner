//! „Funkbrücke“ on a bridge's page: its role, the other bridge (from its wireless link), and
//! only where they aren't cabled the router ports (`radioBridgeCandidates`, `linkPorts`).
use super::{Choice, NetEntry, NetboxHelper, t};
use crate::{
    components::menu::popup::Select,
    error::FrontendError,
    graphql::{
        authenticated::{RadioBridgeCandidates, radio_bridge_candidates},
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Form, FormGroup, Spinner, SpinnerSize, TextInput};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

pub enum RadioMsg {
    Options(Result<Vec<Candidate>, FrontendError>),
    OwnAp(bool),
    Peer(Choice),
    /// side 0 is this bridge, 1 the other one
    RouterDevice(usize, Choice),
    RouterPort(usize, Choice),
    Address(usize, String),
    Ssid(String),
}

/// What „Funkbrücke“ is choosing: whether this bridge is the AP, the other bridge, and per side
/// (this one first) a router port where the bridge isn't cabled and an address.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioChoice {
    pub(super) own_ap: bool,
    pub(super) peer: Option<u32>,
    pub(super) router_device: [Option<u32>; 2],
    pub(super) router_port: [Option<u32>; 2],
    pub(super) address: [String; 2],
    pub(super) ssid: String,
}

impl Default for RadioChoice {
    fn default() -> Self {
        Self {
            own_ap: true,
            peer: None,
            router_device: [None; 2],
            router_port: [None; 2],
            address: Default::default(),
            ssid: String::new(),
        }
    }
}

impl NetboxHelper {
    pub(super) fn update_radio(&mut self, ctx: &Context<Self>, msg: RadioMsg) -> bool {
        match msg {
            RadioMsg::Options(options) => {
                // this bridge's role and the other end of its link, as NetBox has them
                if let Ok(candidates) = &options
                    && let Some(own) = candidates.iter().find(|c| c.device.id as u32 == ctx.props().id)
                {
                    if self.radio.peer.is_none() {
                        self.radio.peer = own.peer.as_ref().map(|peer| peer.id as u32);
                    }
                    if let Some(role) = &own.role {
                        self.radio.own_ap = !matches!(role, radio_bridge_candidates::RadioRole::STATION);
                    }
                }
                self.radio_options = Some(options);
                self.choose(ctx);
            }
            RadioMsg::OwnAp(ap) => {
                self.radio.own_ap = ap;
                self.choose(ctx);
            }
            RadioMsg::Peer(peer) => {
                if self.radio.peer != Some(peer.id) {
                    self.radio.router_device[1] = None;
                    self.radio.router_port[1] = None;
                    self.radio.address[1].clear();
                }
                self.radio.peer = Some(peer.id);
                self.choose(ctx);
            }
            RadioMsg::RouterDevice(side, device) => {
                if self.radio.router_device[side] != Some(device.id) {
                    self.radio.router_port[side] = None;
                }
                self.radio.router_device[side] = Some(device.id);
                self.choose(ctx);
            }
            RadioMsg::RouterPort(side, port) => {
                self.radio.router_port[side] = Some(port.id);
                self.choose(ctx);
            }
            RadioMsg::Address(side, address) => {
                self.radio.address[side] = address;
                self.choose(ctx);
            }
            RadioMsg::Ssid(ssid) => {
                self.radio.ssid = ssid;
                self.choose(ctx);
            }
        }
        true
    }
    /// This device among the radio bridges, if it is one.
    pub(super) fn own_candidate(&self) -> Option<&Candidate> {
        self.candidate(self.device)
    }
    pub(super) fn candidate(&self, id: u32) -> Option<&Candidate> {
        let candidates = self.radio_options.as_ref()?.as_ref().ok()?;
        candidates.iter().find(|c| c.device.id as u32 == id)
    }
    pub(super) fn radio_form(&self, ctx: &Context<Self>) -> Html {
        let candidates = match &self.radio_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(candidates)) => candidates,
        };
        let Some(own) = self.own_candidate() else {
            return html!(<p>{t::NOT_A_RADIO_BRIDGE}</p>);
        };
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let roles = vec![
            NetEntry { value: true, text: t::RADIO_AP.to_string() },
            NetEntry { value: false, text: t::RADIO_STATION.to_string() },
        ];
        let role = roles.iter().find(|r| r.value == self.radio.own_ap).cloned();
        let peers: Vec<Choice> = candidates
            .iter()
            .filter(|c| c.device.id != own.device.id)
            .map(|c| Choice {
                id: c.device.id as u32,
                text: t::radio_candidate(
                    &c.device.name,
                    &c.radio.name,
                    c.peer.as_ref().map(|p| p.name.as_str()),
                ),
            })
            .collect();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let routers: Vec<Choice> = ports
            .topology
            .all_devices
            .iter()
            .filter(|device| self.candidate(device.id as u32).is_none())
            .map(|device| Choice {
                id: device.id as u32,
                text: t::link_device(&device.name, device.site.as_ref().map(|s| s.name.as_str())),
            })
            .collect();
        let router_ports = |side: usize| -> Vec<Choice> {
            self.radio.router_device[side]
                .and_then(|id| ports.topology.all_devices.iter().find(|d| d.id as u32 == id))
                .map(|device| {
                    device
                        .link_ports
                        .iter()
                        .map(|port| Choice {
                            id: port.id as u32,
                            text: t::link_port(
                                &port.name,
                                port.label.as_deref(),
                                port.connected
                                    .first()
                                    .map(|far| (far.device.name.as_str(), far.name.as_str())),
                                port.cabled,
                                port.ipv4.as_deref(),
                                port.bridge.as_deref(),
                            ),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let peer = self.radio.peer.and_then(|id| self.candidate(id));
        let side = |index: usize, bridge: Option<&Candidate>, ap: bool| {
            let cabled = bridge.and_then(|b| b.router_port.as_ref());
            let placeholder = match cabled {
                Some(port) => format!("{}: {} {}", t::CABLED_ROUTER, port.device.name, port.name),
                None => t::CHOOSE_DEVICE.to_string(),
            };
            let address_placeholder = bridge
                .and_then(|b| b.address.clone())
                .unwrap_or_else(|| t::BRIDGE_ADDRESS_PLACEHOLDER.to_string());
            let title = bridge
                .map(|b| t::radio_side(&b.device.name, ap))
                .unwrap_or_default();
            let port_entries = router_ports(index);
            // from the router port: router and port are given
            let fixed = index == 0 && self.fixed_port;
            html! {<>
                <h4>{title}</h4>
                if !fixed {
                <FormGroup label={t::ROUTER_DEVICE}>
                    <Select<Choice>
                        placeholder={placeholder}
                        selected={selected(&routers, self.radio.router_device[index])}
                        entries={routers.clone()}
                        onselect={ctx.link().callback(move |c| RadioMsg::RouterDevice(index, c))}
                    />
                </FormGroup>
                if self.radio.router_device[index].is_some() {
                    <FormGroup label={t::ROUTER_PORT}>
                        <Select<Choice>
                            placeholder={t::CHOOSE_PORT}
                            selected={selected(&port_entries, self.radio.router_port[index])}
                            entries={port_entries.clone()}
                            onselect={ctx.link().callback(move |c| RadioMsg::RouterPort(index, c))}
                        />
                    </FormGroup>
                }
                }
                <FormGroup label={t::BRIDGE_ADDRESS}>
                    <TextInput
                        value={self.radio.address[index].clone()}
                        placeholder={address_placeholder}
                        onchange={ctx.link().callback(move |a| RadioMsg::Address(index, a))}
                    />
                </FormGroup>
            </>}
        };
        html! {
            <Form>
                <FormGroup label={t::RADIO_ROLE}>
                    <Select<NetEntry<bool>>
                        selected={role}
                        entries={roles}
                        onselect={ctx.link().callback(|entry: NetEntry<bool>| RadioMsg::OwnAp(entry.value))}
                    />
                </FormGroup>
                <FormGroup label={t::RADIO_PEER}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_DEVICE}
                        selected={selected(&peers, self.radio.peer)}
                        entries={peers.clone()}
                        onselect={ctx.link().callback(RadioMsg::Peer)}
                    />
                </FormGroup>
                <FormGroup label={t::SSID}>
                    <TextInput
                        value={self.radio.ssid.clone()}
                        placeholder={t::SSID_PLACEHOLDER}
                        onchange={ctx.link().callback(RadioMsg::Ssid)}
                    />
                </FormGroup>
                { side(0, Some(own), self.radio.own_ap) }
                if peer.is_some() {
                    { side(1, peer, !self.radio.own_ap) }
                }
            </Form>
        }
    }
}

pub(super) fn fetch_radio_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<RadioBridgeCandidates, _>(
            scope.clone(),
            radio_bridge_candidates::Variables { id: id as i64 },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .map(|device| device.radio_bridge_candidates)
                .unwrap_or_default()
        });
        scope.send_message(RadioMsg::Options(options));
    });
}

pub(super) type Candidate =
    radio_bridge_candidates::RadioBridgeCandidatesTopologyDeviceByIdRadioBridgeCandidates;
