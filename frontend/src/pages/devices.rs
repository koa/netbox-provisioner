use crate::error::FrontendError;
use crate::graphql::authenticated::{ListDevices, list_devices};
use crate::graphql::query_authenticated;
use patternfly_yew::prelude::{Card, CardHeader, CardTitle, Spinner};
use yew::platform::spawn_local;
use yew::{Component, Context, Html, ToHtml, html};

pub struct Devices {
    state: DeviceState,
    error_state: Option<FrontendError>,
}
enum DeviceState {
    Loading,
    Data(Box<[DeviceRow]>),
}
#[derive(Debug)]
struct DeviceRow {
    name: Box<str>,
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
                        <Card>
                            <CardHeader><CardTitle>{row.name.as_ref()}</CardTitle></CardHeader>
                        </Card>
                    }
                });
                html! {
                    for cards
                }
            }
        };
        html! {
            <>
            {error_msg}
            {data}
            </>
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
                            data.devices
                                .into_iter()
                                .map(|device| DeviceRow {
                                    name: device.name.into_boxed_str(),
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
