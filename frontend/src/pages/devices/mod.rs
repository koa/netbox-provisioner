use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{ListDevices, PingDevice, list_devices, ping_device},
        query_authenticated, query_authenticated_response,
    },
    pages::routes::{AppRoute, DeviceView, RouteDevices},
};
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize};
use std::{net::IpAddr, str::FromStr};
use yew::{Component, Context, Html, Properties, ToHtml, html, platform::spawn_local};
use yew_nested_router::components::Link;
pub mod show;
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
    serial: Option<Box<str>>,
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
                                        .and_then(|ip| ip.address)
                                        .and_then(|a| IpAddr::from_str(a.ip.as_str()).ok()),
                                    serial: device.serial.map(|s| s.into_boxed_str()),
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
    ping_result: PingResult,
}
#[derive(Debug)]
enum PingResult {
    Pending,
    None,
    Success {
        ping_result: ping_device::PingDeviceTopologyDeviceByIdAccessPing,
        device_type: Box<str>,
        serial: Box<str>,
    },
    Failed(FrontendError),
}
#[derive(Debug, Clone, Properties, PartialEq)]
struct DeviceEntryCardProps {
    device: DeviceRow,
}
enum DeviceEntryCardMsg {
    Data,
    PingResult {
        ping_result: ping_device::PingDeviceTopologyDeviceByIdAccessPing,
        device_type: Box<str>,
        serial: Box<str>,
    },
    NoPing,
    PingError(FrontendError),
}

impl Component for DeviceEntryCard {
    type Message = DeviceEntryCardMsg;
    type Properties = DeviceEntryCardProps;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            device: ctx.props().device.clone(),
            ping_result: PingResult::Pending,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DeviceEntryCardMsg::Data => false,
            DeviceEntryCardMsg::PingResult {
                ping_result,
                device_type,
                serial,
            } => {
                self.ping_result = PingResult::Success {
                    ping_result,
                    device_type,
                    serial,
                };
                true
            }
            DeviceEntryCardMsg::NoPing => {
                self.ping_result = PingResult::None;
                true
            }
            DeviceEntryCardMsg::PingError(e) => {
                self.ping_result = PingResult::Failed(e);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let device = &self.device;
        let title = device.name.as_ref();
        let address = device
            .address
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let (ping_result_data, type_description, detected_serial) = match &self.ping_result {
            PingResult::Pending => (
                html! {<Spinner size={SpinnerSize::Sm}/>},
                Html::default(),
                None,
            ),
            PingResult::Success {
                ping_result,
                device_type,
                serial,
            } => (
                format!("{:.2}ms", ping_result.duration as f32 / 1000.0 / 1000.0).into_html(),
                device_type.into_html(),
                Some(html!(<div class="device-detected-serial">{serial.as_ref()}</div>)),
            ),

            PingResult::Failed(e) => (e.into_html(), Html::default(), None),
            PingResult::None => Default::default(),
        };
        let to = AppRoute::Devices(RouteDevices::Device {
            id: device.id,
            view: DeviceView::Show,
        });
        let serial = device
            .serial
            .as_deref()
            .map(|serial| html!(<div class="device-serial">{serial}</div>));

        html! {
            <Card>
                <CardHeader><CardTitle><Link<AppRoute> {to}>{title}</Link<AppRoute>></CardTitle></CardHeader>
                <CardBody>
                    <div class="device-address">{address}</div>
                    <div class="device-ping">{ping_result_data}</div>
                    <div class="device-detected-model">{type_description}</div>
                    {serial} {detected_serial}
                </CardBody>
            </Card>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            let id = self.device.id as i64;
            spawn_local(async move {
                match query_authenticated_response::<PingDevice, _>(
                    scope.clone(),
                    ping_device::Variables { id },
                )
                .await
                {
                    Ok(result) => {
                        let msg =
                            result
                                .data
                                .and_then(|data| {
                                    data.topology.device_by_id.and_then(|d| d.access).and_then(
                                        |d| {
                                            d.ping.into_iter().next().map(|pr| {
                                                DeviceEntryCardMsg::PingResult {
                                                    ping_result: pr,
                                                    device_type: d
                                                        .device_stats
                                                        .routerboard
                                                        .device_type
                                                        .into_boxed_str(),
                                                    serial: d
                                                        .device_stats
                                                        .routerboard
                                                        .serial_number
                                                        .into_boxed_str(),
                                                }
                                            })
                                        },
                                    )
                                })
                                .or(result.errors.filter(|e| !e.is_empty()).map(|e| {
                                    DeviceEntryCardMsg::PingError(FrontendError::Graphql(e))
                                }))
                                .unwrap_or(DeviceEntryCardMsg::NoPing);
                        scope.send_message(msg);
                    }
                    Err(e) => scope.send_message(DeviceEntryCardMsg::PingError(e)),
                }
            });
        }
    }
}
