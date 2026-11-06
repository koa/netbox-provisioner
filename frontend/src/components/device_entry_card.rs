//! A device of a site as a card on the site page: its name linking to the device page, and
//! whether it answers a ping.
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::list_devices::{PingDevice, PingDeviceVariables, PingResult as Pinged},
        request_response,
    },
    i18n,
    routes::AppRoute,
};
use cynic::QueryBuilder;
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize};
use std::net::IpAddr;
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};
use yew_nested_router::components::Link;

/// A device as the site page lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceRow {
    pub site: u32,
    pub id: u32,
    pub name: Box<str>,
    pub address: Option<IpAddr>,
    pub serial: Option<Box<str>>,
    /// how many findings it has, `None` without
    pub findings: Option<String>,
}

#[derive(Debug)]
pub struct DeviceEntryCard {
    device: DeviceRow,
    ping_result: PingResult,
}
#[derive(Debug)]
enum PingResult {
    Pending,
    None,
    Success {
        ping_result: Pinged,
        device_type: Box<str>,
        serial: Box<str>,
    },
    Failed(FrontendError),
}
#[derive(Debug, Clone, Properties, PartialEq)]
pub struct DeviceEntryCardProps {
    pub device: DeviceRow,
}
pub enum DeviceEntryCardMsg {
    PingResult {
        ping_result: Pinged,
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

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
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

    fn view(&self, _ctx: &Context<Self>) -> Html {
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
                html!({ i18n::pages::ping(ping_result.duration) }),
                html!({ device_type.as_ref() }),
                Some(html!(<div class="device-detected-serial">{serial.as_ref()}</div>)),
            ),

            PingResult::Failed(e) => (e.to_html(), Html::default(), None),
            PingResult::None => Default::default(),
        };
        let to = AppRoute::device(device.site, device.id);
        let serial = device
            .serial
            .as_deref()
            .map(|serial| html!(<div class="device-serial">{serial}</div>));
        let findings = device
            .findings
            .clone()
            .map(|count| html!(<div class="device-findings">{count}</div>));

        html! {
            <Card>
                <CardHeader><CardTitle><Link<AppRoute> {to}>{title}</Link<AppRoute>></CardTitle></CardHeader>
                <CardBody>
                    <div class="device-address">{address}</div>
                    <div class="device-ping">{ping_result_data}</div>
                    <div class="device-detected-model">{type_description}</div>
                    {findings}
                    {serial} {detected_serial}
                </CardBody>
            </Card>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            let id = self.device.id as i32;
            spawn_local(async move {
                let ping = PingDevice::build(PingDeviceVariables { id });
                match request_response(scope.clone(), ping).await {
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
