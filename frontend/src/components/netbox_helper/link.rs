//! „Verbindung“ and „Verbindung abräumen“: the ports of the device and of the routers
//! (`linkPorts`), the ends chosen, and a button to take a link in the way out of NetBox.
use super::{Choice, HelperKind, NetboxHelper, t};
use crate::{
    components::menu::popup::Select,
    error::FrontendError,
    graphql::{
        authenticated::{
            LinkPorts,
            link_ports::{self, LinkPortFields},
        },
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Button, ButtonVariant, Form, FormGroup, Spinner, SpinnerSize, TextInput};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

pub enum LinkMsg {
    Ports(Result<link_ports::ResponseData, FrontendError>),
    Port(Choice),
    PeerDevice(Choice),
    Peer(Choice),
    Mtu(String),
    /// „Verbindung abräumen“ with this port
    DisconnectPort(u32),
}

/// The ends of a link being chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkChoice {
    pub(super) port: Option<u32>,
    pub(super) peer_device: Option<u32>,
    pub(super) peer: Option<u32>,
    pub(super) mtu: String,
}

impl NetboxHelper {
    pub(super) fn update_link(&mut self, ctx: &Context<Self>, msg: LinkMsg) -> bool {
        match msg {
            LinkMsg::Ports(ports) => self.ports = Some(ports),
            LinkMsg::Port(port) => {
                self.link.port = Some(port.id);
                // a cable to a router: its other end
                let far = self
                    .port(port.id)
                    .and_then(|p| p.connected.first())
                    .map(|far| (far.device.id as u32, far.id as u32));
                if let Some((device, far)) = far {
                    self.link.peer_device = Some(device);
                    self.link.peer = Some(far);
                }
                self.choose(ctx);
            }
            LinkMsg::PeerDevice(device) => {
                if self.link.peer_device != Some(device.id) {
                    self.link.peer = None;
                }
                self.link.peer_device = Some(device.id);
                self.choose(ctx);
            }
            LinkMsg::Peer(port) => {
                self.link.peer = Some(port.id);
                self.choose(ctx);
            }
            LinkMsg::Mtu(mtu) => {
                self.link.mtu = mtu;
                self.choose(ctx);
            }
            LinkMsg::DisconnectPort(port) => {
                self.kind = HelperKind::Disconnect;
                self.disconnect = Some(port);
                self.choose(ctx);
            }
        }
        true
    }
    pub(super) fn ports(&self) -> Option<&link_ports::ResponseData> {
        self.ports.as_ref().and_then(|ports| ports.as_ref().ok())
    }
    pub(super) fn port(&self, id: u32) -> Option<&LinkPortFields> {
        self.ports()?
            .topology
            .device_by_id
            .as_ref()?
            .link_ports
            .iter()
            .find(|port| port.id as u32 == id)
    }
    /// A port of any router, with its device's name.
    pub(super) fn any_port(&self, id: u32) -> Option<(&str, &LinkPortFields)> {
        self.ports()?.topology.all_devices.iter().find_map(|device| {
            device
                .link_ports
                .iter()
                .find(|port| port.id as u32 == id)
                .map(|port| (device.name.as_str(), port))
        })
    }
    /// The chosen ends of „Verbindung“ whose link is in the way: a cable elsewhere, or an IPv4
    /// not in a network with the other end.
    pub(super) fn in_the_way(&self) -> Vec<(String, u32)> {
        let (Some(own), Some(peer)) = (self.link.port, self.link.peer) else {
            return Vec::new();
        };
        let ends = [(own, peer), (peer, own)];
        ends.iter()
            .filter_map(|&(end, other)| {
                let (device, port) = self.any_port(end).or_else(|| {
                    // the device's own port (the device may not be a router in the list)
                    let device = self.ports()?.topology.device_by_id.as_ref()?;
                    self.port(end).map(|port| (device.name.as_str(), port))
                })?;
                let other_port = self.any_port(other).map(|(_, p)| p).or_else(|| self.port(other));
                let cabled_elsewhere = port.cabled
                    && !port.connected.iter().any(|far| far.id as u32 == other);
                let addressed_elsewhere = port.ipv4.as_deref().is_some_and(|own| {
                    !other_port
                        .and_then(|other| other.ipv4.as_deref())
                        .is_some_and(|other| same_network(own, other))
                });
                (cabled_elsewhere || addressed_elsewhere)
                    .then(|| (t::disconnect_port(device, &port.name), end))
            })
            .collect()
    }
    pub(super) fn disconnect_form(&self, ctx: &Context<Self>) -> Html {
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let mut entries: Vec<Choice> = ports
            .topology
            .device_by_id
            .iter()
            .flat_map(|device| &device.link_ports)
            .filter(|port| port.cabled || port.ipv4.is_some())
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
            .collect();
        // a port at the far end, from „Verbindung“
        if let Some(id) = self.disconnect
            && !entries.iter().any(|entry| entry.id == id)
            && let Some((device, port)) = self.any_port(id)
        {
            entries.push(Choice {
                id,
                text: format!("{device} {}", port.name),
            });
        }
        let selected = self
            .disconnect
            .and_then(|id| entries.iter().find(|entry| entry.id == id).cloned());
        html! {
            <Form>
                <FormGroup label={t::PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PORT}
                        selected={selected}
                        entries={entries}
                        onselect={ctx.link().callback(|port: Choice| LinkMsg::DisconnectPort(port.id))}
                    />
                </FormGroup>
            </Form>
        }
    }
    pub(super) fn link_form(&self, ctx: &Context<Self>) -> Html {
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let choices = |ports: &[LinkPortFields]| -> Vec<Choice> {
            ports
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
        };
        let own = ports
            .topology
            .device_by_id
            .as_ref()
            .map(|device| choices(&device.link_ports))
            .unwrap_or_default();
        let devices: Vec<Choice> = ports
            .topology
            .all_devices
            .iter()
            .filter(|device| device.id as u32 != ctx.props().id)
            .map(|device| Choice {
                id: device.id as u32,
                text: t::link_device(&device.name, device.site.as_ref().map(|s| s.name.as_str())),
            })
            .collect();
        let peer_ports = self
            .link
            .peer_device
            .and_then(|id| ports.topology.all_devices.iter().find(|d| d.id as u32 == id))
            .map(|device| choices(&device.link_ports))
            .unwrap_or_default();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let mtu = self.link.mtu.clone();
        let in_the_way = self.in_the_way();
        html! {
            <Form>
                <FormGroup label={t::PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PORT}
                        selected={selected(&own, self.link.port)}
                        entries={own.clone()}
                        onselect={ctx.link().callback(LinkMsg::Port)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_DEVICE}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_DEVICE}
                        selected={selected(&devices, self.link.peer_device)}
                        entries={devices.clone()}
                        onselect={ctx.link().callback(LinkMsg::PeerDevice)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_PORT}>
                    <Select<Choice>
                        placeholder={t::CHOOSE_PEER_PORT}
                        selected={selected(&peer_ports, self.link.peer)}
                        entries={peer_ports.clone()}
                        onselect={ctx.link().callback(LinkMsg::Peer)}
                    />
                </FormGroup>
                <FormGroup label={t::MTU}>
                    <TextInput
                        value={mtu}
                        placeholder={t::MTU_PLACEHOLDER}
                        onchange={ctx.link().callback(LinkMsg::Mtu)}
                    />
                </FormGroup>
                if !in_the_way.is_empty() {
                    <div class="helper-actions">
                        { for in_the_way.into_iter().map(|(label, port)| html! {
                            <Button
                                variant={ButtonVariant::Secondary}
                                label={label}
                                onclick={ctx.link().callback(move |_| LinkMsg::DisconnectPort(port))}
                            />
                        })}
                    </div>
                }
            </Form>
        }
    }
}

pub(super) fn fetch_ports(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let ports = query_authenticated::<LinkPorts, _>(
            scope.clone(),
            link_ports::Variables { id: id as i64 },
        )
        .await;
        scope.send_message(LinkMsg::Ports(ports));
    });
}

/// Whether two IPv4 addresses with their length (`172.17.1.41/30`) are in one network.
pub(super) fn same_network(a: &str, b: &str) -> bool {
    let parse = |text: &str| -> Option<(u32, u32)> {
        let (address, len) = text.split_once('/')?;
        let address: std::net::Ipv4Addr = address.parse().ok()?;
        let len: u32 = len.parse().ok()?;
        let mask = u32::MAX.checked_shl(32 - len).unwrap_or(0);
        Some((u32::from(address) & mask, len))
    };
    matches!((parse(a), parse(b)), (Some(a), Some(b)) if a == b)
}
