//! Scope: The devices in one of a resident's networks, read from the devices on a click: their
//! names, addresses and WLAN; not the network card around it (`components::resident`).
use crate::{
    error::FrontendError,
    graphql::{
        request_resident,
        resident::live::{
            NetworkDevice, NetworkDevices, NetworkDevicesQuery, NetworkDevicesVariables, WlanBand,
            WlanFrequency, WlanStandard,
        },
    },
};
use cynic::QueryBuilder;
use i18n::resident as t;
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct NetworkDevicesProps {
    pub resident: i32,
    /// the network (VLAN)
    pub vlan: i32,
}

pub enum NetworkDevicesView {
    Closed,
    Reading,
    Read(NetworkDevices),
    Failed(FrontendError),
}

pub enum NetworkDevicesMsg {
    Read,
    Close,
    Answer(Result<NetworkDevices, FrontendError>),
}

impl Component for NetworkDevicesView {
    type Message = NetworkDevicesMsg;
    type Properties = NetworkDevicesProps;

    fn create(_ctx: &Context<Self>) -> Self {
        NetworkDevicesView::Closed
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        *self = match msg {
            NetworkDevicesMsg::Read => {
                let scope = ctx.link().clone();
                let variables = NetworkDevicesVariables {
                    resident: ctx.props().resident,
                    vlan: ctx.props().vlan,
                };
                spawn_local(async move {
                    let answer =
                        request_resident(scope.clone(), NetworkDevicesQuery::build(variables))
                            .await
                            .map(|data| data.network_devices);
                    scope.send_message(NetworkDevicesMsg::Answer(answer));
                });
                NetworkDevicesView::Reading
            }
            NetworkDevicesMsg::Close => NetworkDevicesView::Closed,
            NetworkDevicesMsg::Answer(Ok(devices)) => NetworkDevicesView::Read(devices),
            NetworkDevicesMsg::Answer(Err(error)) => NetworkDevicesView::Failed(error),
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let again = html! {
            <Button
                variant={ButtonVariant::Link}
                label={t::READ_AGAIN}
                onclick={link.callback(|_| NetworkDevicesMsg::Read)}
            />
        };
        match self {
            NetworkDevicesView::Closed => html! {
                <Button
                    variant={ButtonVariant::Link}
                    label={t::SHOW_DEVICES}
                    onclick={link.callback(|_| NetworkDevicesMsg::Read)}
                />
            },
            NetworkDevicesView::Reading => html!(<p>{t::READING_DEVICES}</p>),
            NetworkDevicesView::Failed(error) => html!(<>{error.to_html()}{again}</>),
            NetworkDevicesView::Read(read) => {
                let unreachable = (!read.unreachable.is_empty()).then(|| {
                    let names = read
                        .unreachable
                        .iter()
                        .map(|device| device.name.as_str())
                        .collect::<Vec<_>>();
                    html! {
                        <Alert
                            inline=true
                            r#type={AlertType::Warning}
                            title={t::devices_unreachable(&names)}
                        />
                    }
                });
                let list = if read.devices.is_empty() {
                    html!(<p>{t::NO_DEVICES}</p>)
                } else {
                    html! {
                        <ul class="resident-facts">
                            { for read.devices.iter().map(device) }
                        </ul>
                    }
                };
                html! {
                    <div class="resident-edit">
                        {unreachable}
                        {list}
                        <div class="resident-edit-actions">
                            {again}
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::CLOSE}
                                onclick={link.callback(|_| NetworkDevicesMsg::Close)}
                            />
                        </div>
                    </div>
                }
            }
        }
    }
}

/// A device: its name, its address with the MAC as tooltip, and how it is connected.
fn device(device: &NetworkDevice) -> Html {
    let name = device.hostname.as_deref().unwrap_or(t::NO_HOSTNAME);
    let address = device.address.as_deref().unwrap_or(&device.mac_address);
    let wlan = device
        .wlan
        .as_ref()
        .map(|wlan| {
            let band = wlan.band.as_ref().map(band).unwrap_or_default();
            t::wlan_connection(&wlan.access_point, &band, wlan.signal)
        })
        .unwrap_or_default();
    html! {
        <li key={device.mac_address.clone()}>
            {name}{" "}
            <span title={device.mac_address.clone()}>{address}</span>
            {" "}<small>{wlan}</small>
        </li>
    }
}

/// „5 GHz (ax)“; empty for a band of a newer server.
fn band(band: &WlanBand) -> String {
    let ghz = match band.frequency {
        WlanFrequency::Ghz2 => "2,4",
        WlanFrequency::Ghz5 => "5",
        WlanFrequency::Ghz6 => "6",
        WlanFrequency::Other => return String::new(),
    };
    let standard = band.standard.and_then(|standard| match standard {
        WlanStandard::A => Some("a"),
        WlanStandard::B => Some("b"),
        WlanStandard::G => Some("g"),
        WlanStandard::N => Some("n"),
        WlanStandard::Ac => Some("ac"),
        WlanStandard::Ax => Some("ax"),
        WlanStandard::Be => Some("be"),
        WlanStandard::Other => None,
    });
    t::wlan_band(ghz, standard)
}
