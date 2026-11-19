//! Scope: What a resident sees of its network, in tabs: its sockets and devices, its networks;
//! not loading it (`pages::residents`, `app`), nor the admins' choice of a resident.
use crate::graphql::resident::residents::{
    FarEnd, PortKind, Resident, ResidentNetwork, ResidentPort,
};
use i18n::resident as t;
use wlan_edit::WlanEdit;

mod wlan_edit;
use patternfly_yew::prelude::{
    Card, CardBody, CardHeader, CardSize, CardTitle, Content, Level, Tab, Tabs, Title,
};
use yew::{Callback, Html, Properties, function_component, html, html_nested, use_state};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ResidentViewProps {
    pub resident: Resident,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Ports,
    Networks,
}

/// A resident's sockets and devices, and its networks.
#[function_component]
pub fn ResidentView(props: &ResidentViewProps) -> Html {
    let view = use_state(|| View::Ports);
    let onselect = {
        let view = view.clone();
        move |shown| view.set(shown)
    };
    let resident = &props.resident;
    html! {
        <div class="resident-view">
            <Tabs<View> selected={*view} {onselect}>
                <Tab<View> index={View::Ports} title={t::TAB_PORTS}>
                    {ports(&resident.ports)}
                </Tab<View>>
                <Tab<View> index={View::Networks} title={t::TAB_NETWORKS}>
                    {networks(resident, &props.on_saved)}
                </Tab<View>>
            </Tabs<View>>
        </div>
    }
}

/// The ports of one site and location, as the backend sorts them (site, location, device,
/// port).
struct Group<'a> {
    site: Option<&'a str>,
    location: &'a [String],
    ports: Vec<&'a ResidentPort>,
}

/// The ports by site, and in it by location (a floor, a flat, a room).
fn ports(ports: &[ResidentPort]) -> Html {
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
                    { for group.ports.into_iter().map(port_row) }
                </section>
            }
        })
        .collect()
}

/// A socket: left the port, a cable, right what is at the cable's other end, or its network.
fn port_row(port: &ResidentPort) -> Html {
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
        <div class="sock-row" key={format!("{:?}-{}", port.kind, port.id)}>
            <div class="sock-side">{left}</div>
            {cable}{gap}
            <div class="sock-side">{right}</div>
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
                </CardBody>
            </Card>
        }
    });
    html!(<div class="resident-networks">{ for cards }</div>)
}
