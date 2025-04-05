use crate::components::adjust_target::{AdjustTarget, SelectedCredentials, SelectedTarget};
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{device_overview, DeviceOverview},
        query_authenticated_response,
    },
};
use log::info;
use patternfly_yew::prelude::ExpandableSection;
use yew::{html, html::Scope, platform::spawn_local, Component, Context, Html, Properties, ToHtml};

pub struct ShowDevice {
    id: u32,
    error: Option<FrontendError>,
    data: Option<ShowDeviceData>,
    alternate_target: SelectedTarget,
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
    AdjustTarget(SelectedTarget),
}

impl Component for ShowDevice {
    type Message = ShowDeviceMessage;
    type Properties = ShowDeviceProps;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            id: ctx.props().id,
            error: None,
            data: None,
            alternate_target: Default::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        info!("Msg: {:?}", msg);
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
            ShowDeviceMessage::AdjustTarget(t) => {
                if self.alternate_target != t {
                    self.alternate_target = t;
                    fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
                    true
                } else {
                    false
                }
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
            <ExpandableSection toggle_text_hidden="Change Target" toggle_text_expanded="Hide Target Selection">
                <AdjustTarget onchange={ctx.link().callback(ShowDeviceMessage::AdjustTarget)} value={self.alternate_target.clone()} />
            </ExpandableSection>
            {error}
            {data}
            </>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
        }
    }
}

fn fetch_overview(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) = match target.credentials {
            SelectedCredentials::Default => (None, None),
            SelectedCredentials::Named(name) => (Some(name.into_string()), None),
            SelectedCredentials::Adhoc { username, password } => (
                None,
                Some(device_overview::AdhocCredentials {
                    username: Some(username.to_string()),
                    password: Some(password.to_string()),
                }),
            ),
        };
        match query_authenticated_response::<DeviceOverview, _>(
            scope.clone(),
            device_overview::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
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
