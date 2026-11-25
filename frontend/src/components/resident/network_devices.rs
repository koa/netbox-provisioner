//! Scope: The devices in one of a resident's networks, read from the devices on a click: their
//! names, addresses and WLAN; not the network card around it (`components::resident`).
use super::network_device::NetworkDeviceItem;
use crate::{
    error::FrontendError,
    graphql::{
        request_resident,
        resident::live::{NetworkDevices, NetworkDevicesQuery, NetworkDevicesVariables},
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
                            { for read.devices.iter().map(|device| html! {
                                <NetworkDeviceItem key={device.mac_address.clone()} device={device.clone()}/>
                            }) }
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
