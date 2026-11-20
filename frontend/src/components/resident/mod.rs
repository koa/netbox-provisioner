//! Scope: What a resident sees of its network, in tabs: its sockets and devices, its networks;
//! not loading it (`pages::residents`, `app`), nor the admins' choice of a resident.
use crate::graphql::{
    request_resident,
    resident::{
        live::{SeenUplink, SeenUplinksQuery, SeenUplinksVariables},
        residents::{FarEnd, PortKind, Resident, ResidentNetwork, ResidentPort},
    },
};
use cynic::QueryBuilder;
use device_link_edit::DeviceLinkEdit;
use i18n::resident as t;
use network_devices::NetworkDevicesView;
use port_vlan_edit::PortVlanEdit;
use wlan_edit::WlanEdit;

mod change_flow;
mod device_link_edit;
mod network_devices;
mod port_vlan_edit;
mod wlan_edit;
use patternfly_yew::prelude::{
    Card, CardBody, CardHeader, CardSize, CardTitle, Content, Level, Tab, Tabs, Title,
};
use yew::{
    Callback, Component, Context, Html, Properties, html, html_nested, platform::spawn_local,
};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ResidentViewProps {
    pub resident: Resident,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Ports,
    Networks,
}

/// A resident's sockets and devices, and its networks; where the routers see its devices
/// plugged in, read once the view is shown and again with each loaded resident.
pub struct ResidentView {
    view: View,
    /// the devices whose cable is in another port than their Anschluss; empty while read or
    /// where the routers didn't answer (the view offers nothing then)
    seen: Vec<SeenUplink>,
}

pub enum ResidentViewMsg {
    Show(View),
    Seen(Vec<SeenUplink>),
}

impl Component for ResidentView {
    type Message = ResidentViewMsg;
    type Properties = ResidentViewProps;

    fn create(ctx: &Context<Self>) -> Self {
        read_seen(ctx);
        ResidentView {
            view: View::Ports,
            seen: Vec::new(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ResidentViewMsg::Show(view) => self.view = view,
            ResidentViewMsg::Seen(seen) => self.seen = seen,
        }
        true
    }

    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        // a new callback each time the page renders is no reason to ask the routers
        if old.resident != ctx.props().resident {
            read_seen(ctx);
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let resident = &props.resident;
        let onselect = ctx.link().callback(ResidentViewMsg::Show);
        html! {
            <div class="resident-view">
                <Tabs<View> selected={self.view} {onselect}>
                    <Tab<View> index={View::Ports} title={t::TAB_PORTS}>
                        {ports(resident, &self.seen, &props.on_saved)}
                    </Tab<View>>
                    <Tab<View> index={View::Networks} title={t::TAB_NETWORKS}>
                        {networks(resident, &props.on_saved)}
                    </Tab<View>>
                </Tabs<View>>
            </div>
        }
    }
}

/// Asks where the routers see the resident's devices plugged in.
fn read_seen(ctx: &Context<ResidentView>) {
    let scope = ctx.link().clone();
    let variables = SeenUplinksVariables {
        resident: ctx.props().resident.id,
    };
    spawn_local(async move {
        let seen = request_resident(scope.clone(), SeenUplinksQuery::build(variables))
            .await
            .map(|data| data.seen_uplinks.devices)
            .unwrap_or_default();
        scope.send_message(ResidentViewMsg::Seen(seen));
    });
}

/// The ports of one site and location, as the backend sorts them (site, location, device,
/// port).
struct Group<'a> {
    site: Option<&'a str>,
    location: &'a [String],
    ports: Vec<&'a ResidentPort>,
}

/// The ports by site, and in it by location (a floor, a flat, a room).
fn ports(resident: &Resident, seen: &[SeenUplink], on_saved: &Callback<()>) -> Html {
    let ports: &[ResidentPort] = &resident.ports;
    if ports.is_empty() {
        return html!(<Content><p>{t::NO_PORTS}</p></Content>);
    }
    let mut groups: Vec<Group> = Vec::new();
    for port in ports {
        let site = port.site.as_ref().map(|site| site.name.as_str());
        match groups.last_mut() {
            Some(last) if last.site == site && last.location == port.location.as_slice() => {
                last.ports.push(port)
            }
            _ => groups.push(Group {
                site,
                location: &port.location,
                ports: vec![port],
            }),
        }
    }
    let mut shown_site = None;
    groups
        .into_iter()
        .map(|group| {
            // the site once, above its first location
            let site = (shown_site != Some(group.site)).then(|| {
                shown_site = Some(group.site);
                group
                    .site
                    .map(|site| html!(<Title level={Level::H3}>{site}</Title>))
            });
            let location = (!group.location.is_empty()).then(|| {
                let path = t::location_path(group.location);
                html!(<Title level={Level::H4}>{path}</Title>)
            });
            html! {
                <section class="resident-site">
                    {site.flatten()}
                    {location}
                    { for group.ports.into_iter().map(|port| port_row(resident, port, seen, on_saved)) }
                </section>
            }
        })
        .collect()
}

/// A socket: left the port, a cable, right what is at the cable's other end, or its network.
/// Its network can change where nothing but the resident's devices hangs at it.
fn port_row(
    resident: &Resident,
    port: &ResidentPort,
    seen: &[SeenUplink],
    on_saved: &Callback<()>,
) -> Html {
    let socket = port.kind == PortKind::Socket;
    let title = match &port.label {
        Some(label) => label.clone(),
        // a socket's device is the wall it is in, named after the room
        None if socket => t::socket_name(&port.device.name, &port.name),
        None => port.name.clone(),
    };
    let sub = match port.kind {
        PortKind::Socket if port.label.is_none() => t::SOCKET.to_string(),
        PortKind::Socket => t::socket_at(&port.device.name, &port.name),
        _ if port.on_own_device => t::own_port(&port.name, &port.device.name),
        _ => t::port_of(&port.name, &port.device.name),
    };
    // a socket's network is behind the wall, whatever is plugged in: shown with the socket
    let network = (socket && !port.far_ends.is_empty())
        .then(|| html!(<><br/><small>{network_text(port)}</small></>));
    // a device the provisioner links hangs there: its link, not a network (`DeviceLinkEdit`)
    let linked = port.far_ends.iter().any(|end| {
        resident
            .devices
            .iter()
            .any(|device| device.id == end.device.id && device.routeros)
    });
    let edit = (!linked && port.far_ends.iter().all(|end| end.own_device)).then(|| {
        html! {
            <PortVlanEdit
                resident={resident.id}
                port={port.clone()}
                networks={resident.networks.clone()}
                on_saved={on_saved.clone()}
            />
        }
    });
    let link = html! {
        <DeviceLinkEdit
            resident={resident.id}
            port={port.clone()}
            devices={resident.devices.clone()}
            seen={seen.to_vec()}
            on_saved={on_saved.clone()}
        />
    };
    let left = html! {
        <Card size={CardSize::Compact} class="sock-card">
            <CardHeader><CardTitle>{title}</CardTitle></CardHeader>
            <CardBody><small>{sub}</small>{network}</CardBody>
        </Card>
    };
    let right = if port.far_ends.is_empty() {
        network_card(port)
    } else {
        port.far_ends.iter().map(far_end_card).collect()
    };
    let cable = (!port.far_ends.is_empty()).then(|| {
        html! {
            <div class="sock-conn">
                <div class="sock-line"></div>
                <div class="sock-text"><small>{t::CABLE}</small></div>
            </div>
        }
    });
    let gap = port
        .far_ends
        .is_empty()
        .then(|| html!(<div class="sock-conn"></div>));
    html! {
        <div class="sock-item" key={format!("{:?}-{}", port.kind, port.id)}>
            <div class="sock-row">
                <div class="sock-side">{left}</div>
                {cable}{gap}
                <div class="sock-side">{right}</div>
            </div>
            {edit}
            {link}
        </div>
    }
}

fn far_end_card(end: &FarEnd) -> Html {
    let own = end
        .own_device
        .then(|| html!(<><br/><small>{t::YOUR_DEVICE}</small></>));
    html! {
        <Card size={CardSize::Compact} class="sock-card">
            <CardHeader><CardTitle>{end.device.name.clone()}</CardTitle></CardHeader>
            <CardBody><small>{t::far_port(&end.port)}</small>{own}</CardBody>
        </Card>
    }
}

/// A port without cable: its network, the operator's, or none (dashed).
fn network_card(port: &ResidentPort) -> Html {
    let class = if port.network.is_none() && !port.foreign_network {
        "sock-net sock-none"
    } else {
        "sock-net"
    };
    html!(<div class={class}>{network_text(port)}</div>)
}

/// The port's network: the resident's by name, the operator's, or none.
fn network_text(port: &ResidentPort) -> String {
    match (&port.network, port.foreign_network) {
        (Some(network), _) => network.name.clone(),
        (None, true) => t::FOREIGN_NETWORK.to_string(),
        (None, false) => t::NO_NETWORK.to_string(),
    }
}

/// The networks, each with its addresses and WLAN.
fn networks(resident: &Resident, on_saved: &Callback<()>) -> Html {
    let networks: &[ResidentNetwork] = &resident.networks;
    if networks.is_empty() {
        return html!(<Content><p>{t::NO_NETWORKS}</p></Content>);
    }
    let cards = networks.iter().map(|network| {
        let ssid = network.wlans.first().map(|wlan| wlan.ssid.as_str());
        let title = t::network_title(&network.name, ssid);
        let subnets = if network.subnets.is_empty() {
            t::NO_SUBNET.to_string()
        } else {
            network.subnets.join(", ")
        };
        let wlans = if network.wlans.is_empty() {
            html!(<li>{t::NO_WLAN}</li>)
        } else {
            network
                .wlans
                .iter()
                .map(|wlan| {
                    let text = t::wlan(&wlan.ssid, wlan.access_points, wlan.enabled);
                    html! {
                        <li key={wlan.id}>
                            {text}
                            <WlanEdit
                                resident={resident.id}
                                wlan={wlan.clone()}
                                on_saved={on_saved.clone()}
                            />
                        </li>
                    }
                })
                .collect()
        };
        html_nested! {
            <Card key={network.id}>
                <CardHeader><CardTitle>{title}</CardTitle></CardHeader>
                <CardBody>
                    <ul class="resident-facts">
                        <li>{t::vid(network.vid)}</li>
                        <li>{subnets}</li>
                        {wlans}
                    </ul>
                    <NetworkDevicesView resident={resident.id} vlan={network.id}/>
                </CardBody>
            </Card>
        }
    });
    html!(<div class="resident-networks">{ for cards }</div>)
}
