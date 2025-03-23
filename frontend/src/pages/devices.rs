use crate::graphql::authenticated::{ping_device, PingDevice};
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{list_devices, ListDevices},
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner};
use std::{net::IpAddr, str::FromStr};
use yew::{html, platform::spawn_local, Component, Context, Html, Properties, ToHtml};

pub struct Devices {
    state: DeviceState,
    error_state: Option<FrontendError>,
}
enum DeviceState {
    Loading,
    Data(Box<[DeviceRow]>),
}
#[derive(Debug, Clone, PartialEq)]
struct DeviceRow {
    id: u32,
    name: Box<str>,
    address: Option<IpAddr>,
}
#[derive(Debug)]
pub enum DevicesMsg {
    Data(Box<[DeviceRow]>),
    Error(FrontendError),
}
impl Component for Devices {
    type Message = DevicesMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            state: DeviceState::Loading,
            error_state: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DevicesMsg::Data(data) => {
                self.state = DeviceState::Data(data);
                self.error_state = None;
                true
            }
            DevicesMsg::Error(error) => {
                self.error_state = Some(error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let error_msg = self.error_state.as_ref().map(|e| FrontendError::to_html(e));
        let data = match &self.state {
            DeviceState::Loading => {
                html! {<Spinner/>}
            }
            DeviceState::Data(rows) => {
                let cards = rows.iter().map(|row| {
                    html! {
                        <DeviceEntryCard device={row.clone()}/>
                    }
                });
                html! {
                    for cards
                }
            }
        };
        html! {
            <div class="device-list">
                {error_msg}
                {data}
            </div>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                match query_authenticated::<ListDevices, _>(
                    scope.clone(),
                    list_devices::Variables {},
                )
                .await
                {
                    Ok(data) => {
                        scope.send_message(DevicesMsg::Data(
                            data.topology
                                .all_devices
                                .into_iter()
                                .map(|device| DeviceRow {
                                    id: device.id as u32,
                                    name: device.name.into_boxed_str(),
                                    address: device
                                        .management_address
                                        .and_then(|s| IpAddr::from_str(s.as_str()).ok()),
                                })
                                .collect(),
                        ));
                    }
                    Err(e) => {
                        scope.send_message(DevicesMsg::Error(e));
                    }
                }
            })
        }
    }
}

#[derive(Debug)]
struct DeviceEntryCard {
    device: DeviceRow,
    ping_result: Option<ping_device::PingDeviceTopologyDeviceByIdAccessPing>,
}
#[derive(Debug, Clone, Properties, PartialEq)]
struct DeviceEntryCardProps {
    device: DeviceRow,
}
enum DeviceEntryCardMsg {
    Data,
    PingResult(ping_device::PingDeviceTopologyDeviceByIdAccessPing),
}

impl Component for DeviceEntryCard {
    type Message = DeviceEntryCardMsg;
    type Properties = DeviceEntryCardProps;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            device: ctx.props().device.clone(),
            ping_result: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DeviceEntryCardMsg::Data => false,
            DeviceEntryCardMsg::PingResult(r) => {
                self.ping_result = Some(r);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let title = self.device.name.as_ref();
        let address = self
            .device
            .address
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let ping_result = self.ping_result.as_ref().map(|r| {
            let text = format!("{:.2}ms", r.duration as f32 / 1000.0 / 1000.0);
            html!(<div class="device-ping">{text}</div>)
        });

        html! {
            <Card>
                <CardHeader><CardTitle>{title}</CardTitle></CardHeader>
                <CardBody>
                    <div class="device-address">{address}</div>
                    {ping_result}
                </CardBody>
            </Card>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            let id = self.device.id as i64;
            spawn_local(async move {
                match query_authenticated::<PingDevice, _>(
                    scope.clone(),
                    ping_device::Variables { id },
                )
                .await
                {
                    Ok(result) => {
                        if let Some(ping_result) = result
                            .topology
                            .device_by_id
                            .and_then(|d| d.access)
                            .and_then(|d| d.ping.into_iter().next())
                        {
                            scope.send_message(DeviceEntryCardMsg::PingResult(ping_result));
                        }
                    }
                    Err(_) => {}
                }
            });
        }
    }
}
