use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{DeviceOverview, device_overview},
        query_authenticated_response,
    },
};
use yew::{Component, Context, Html, Properties, ToHtml, html, html::Scope, platform::spawn_local};

pub struct ShowDevice {
    id: u32,
    error: Option<FrontendError>,
    data: Option<ShowDeviceData>,
}
#[derive(Debug, PartialEq)]
pub struct ShowDeviceData {
    configured_name: Box<str>,
    current_name: Box<str>,
}
#[derive(Debug, Properties, Clone, PartialEq)]
pub struct ShowDeviceProps {
    pub id: u32,
}

#[derive(Debug)]
pub enum ShowDeviceMessage {
    Error(FrontendError),
    Data {
        data: ShowDeviceData,
        error: Option<FrontendError>,
    },
}

impl Component for ShowDevice {
    type Message = ShowDeviceMessage;
    type Properties = ShowDeviceProps;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            id: ctx.props().id,
            error: None,
            data: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ShowDeviceMessage::Error(e) => {
                self.error = Some(e);
                true
            }
            ShowDeviceMessage::Data { data, error } => {
                self.data = Some(data);
                self.error = error;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let error = self.error.as_ref().map(|e| e.to_html());
        let data = self.data.as_ref().map(|data| {
            html! {<dl>
            <dt>{"Name"}</dt><dd>{ data.configured_name.as_ref() }</dd>
            <dt>{"Current Name"}</dt><dd>{ data.current_name.as_ref() }</dd>
            </dl>}
        });
        html! {
            <>
            {error}
            {data}
            </>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            fetch_overview(ctx.link().clone(), self.id);
        }
    }
}

fn fetch_overview(scope: Scope<ShowDevice>, id: u32) {
    spawn_local(async move {
        match query_authenticated_response::<DeviceOverview, _>(
            scope.clone(),
            device_overview::Variables { id: id as i64 },
        )
        .await
        {
            Ok(response) => {
                if let Some(data) = response.data {
                    let error = response
                        .errors
                        .filter(|e| !e.is_empty())
                        .map(FrontendError::Graphql);
                    let device = data.topology.device_by_id;
                    let (configured_name, current_name) = device
                        .map(|d| {
                            (
                                d.name.into_boxed_str(),
                                d.access
                                    .map(|a| {
                                        a.device_stats.routerboard.device_type.into_boxed_str()
                                    })
                                    .unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default();

                    scope.send_message(ShowDeviceMessage::Data {
                        data: ShowDeviceData {
                            configured_name,
                            current_name,
                        },
                        error,
                    });
                } else if let Some(errors) = response.errors {
                    scope.send_message(ShowDeviceMessage::Error(FrontendError::Graphql(errors)))
                } else {
                    scope.send_message(ShowDeviceMessage::Error(FrontendError::MissingData))
                }
            }
            Err(e) => {
                scope.send_message(ShowDeviceMessage::Error(e));
            }
        }
    });
}
