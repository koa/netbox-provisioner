//! Scope: What a resident sees of its network, in tabs: its sockets and devices, its networks;
//! not loading it (`pages::residents`, `app`), nor the admins' choice of a resident.
use crate::graphql::resident::residents::{FarEnd, Resident, ResidentNetwork, ResidentPort};
use i18n::resident as t;
use patternfly_yew::prelude::{
    Card, CardBody, CardHeader, CardSize, CardTitle, Content, Level, Tab, Tabs, Title,
};
use yew::{Html, Properties, function_component, html, html_nested, use_state};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ResidentViewProps {
    pub resident: Resident,
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
                    {networks(&resident.networks)}
                </Tab<View>>
            </Tabs<View>>
        </div>
    }
}

/// The ports by site, as the backend sorts them (site, device, port).
fn ports(ports: &[ResidentPort]) -> Html {
    if ports.is_empty() {
        return html!(<Content><p>{t::NO_PORTS}</p></Content>);
    }
    let mut groups: Vec<(Option<&str>, Vec<&ResidentPort>)> = Vec::new();
    for port in ports {
        let site = port.site.as_ref().map(|site| site.name.as_str());
        match groups.last_mut() {
            Some((last, members)) if *last == site => members.push(port),
            _ => groups.push((site, vec![port])),
        }
    }
    groups
        .into_iter()
        .map(|(site, members)| {
            let title = site.map(|site| html!(<Title level={Level::H3}>{site}</Title>));
            html! {
                <section class="resident-site">
                    {title}
                    { for members.into_iter().map(port_row) }
                </section>
            }
        })
        .collect()
}

/// A socket: left the port, a cable, right what is at the cable's other end, or its network.
fn port_row(port: &ResidentPort) -> Html {
    let title = port.label.clone().unwrap_or_else(|| port.name.clone());
    let sub = if port.on_own_device {
        t::own_port(&port.name, &port.device.name)
    } else {
        t::socket_of(&port.device.name)
    };
    let left = html! {
        <Card size={CardSize::Compact} class="sock-card">
            <CardHeader><CardTitle>{title}</CardTitle></CardHeader>
            <CardBody><small>{sub}</small></CardBody>
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
        <div class="sock-row" key={port.id}>
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
    let (text, class) = match (&port.network, port.foreign_network) {
        (Some(network), _) => (network.name.clone(), "sock-net"),
        (None, true) => (t::FOREIGN_NETWORK.to_string(), "sock-net"),
        (None, false) => (t::NO_NETWORK.to_string(), "sock-net sock-none"),
    };
    html!(<div class={class}>{text}</div>)
}

/// The networks, each with its addresses and WLAN.
fn networks(networks: &[ResidentNetwork]) -> Html {
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
                    let text = t::wlan(&wlan.ssid, wlan.access_points);
                    html!(<li key={wlan.id}>{text}</li>)
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
