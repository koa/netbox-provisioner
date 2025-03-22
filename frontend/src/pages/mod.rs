use crate::data::UserSessionData;
use crate::error::FrontendError;
use crate::graphql::{anonymous::settings, anonymous::Settings, query_anonymous};
use gloo::timers::callback::Timeout;
use google_signin_client::{
    initialize, prompt_async, render_button, ButtonType, DismissedReason,
    GsiButtonConfiguration, IdConfiguration, NotDisplayedReason, PromptResult,
};
use log::{error, info, warn};
use patternfly_yew::prelude::{
    Alert, AlertGroup, AlertType, BackdropViewer, Nav, NavItem, NavRouterItem, Page, PageSidebar,
    ToastViewer,
};
use std::time::Duration;
use web_sys::{HtmlElement, MouseEvent};
use yew::{
    function_component, html, html_nested, platform::spawn_local, Callback, Context, ContextProvider, Html,
    NodeRef, Properties,
};
use yew_nested_router::{prelude::Switch as RouterSwitch, Router, Target};

#[derive(Debug)]
pub struct App {
    client_id: Option<String>,
    user_session: UserSessionData,
    error_state: Option<ErrorState>,
    login_button_ref: NodeRef,
    running_timeout: Option<Timeout>,
}
#[derive(Debug)]
enum ErrorState {
    FrontendError(FrontendError),
    PromptResult(PromptResult),
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Target)]
pub enum AppRoute {
    NotFound,
    #[default]
    Home,
}
fn switch_main(switch: AppRoute) -> Html {
    match switch {
        AppRoute::Home => html! {<h1>{"Home"}</h1>},
        AppRoute::NotFound => html! {<h1>{"Not Found"}</h1>},
    }
}
#[derive(Properties, PartialEq)]
pub struct Props {}

#[derive(Debug)]
pub enum AppMessage {
    ClientIdReceived(String),
    TokenReceived(String),
    ClientError(FrontendError),
    LoginFailed(PromptResult),
    CheckSession,
}
impl yew::Component for App {
    type Message = AppMessage;
    type Properties = ();
    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            client_id: None,
            user_session: Default::default(),
            error_state: None,
            login_button_ref: NodeRef::default(),
            running_timeout: None,
        }
    }
    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            AppMessage::TokenReceived(token) => {
                self.user_session = UserSessionData::from_token(token);
                self.error_state = None;
                self.user_session.is_token_valid()
            }
            AppMessage::ClientIdReceived(client_id) => {
                self.client_id = Some(client_id);
                self.error_state = None;
                true
            }
            AppMessage::ClientError(error) => {
                self.error_state = Some(ErrorState::FrontendError(error));
                true
            }
            AppMessage::LoginFailed(prompt) => {
                self.error_state = Some(ErrorState::PromptResult(prompt));
                true
            }
            AppMessage::CheckSession => self.user_session.is_token_valid(),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let user_session = &self.user_session;
        if let (Some(client_id), false) = (&self.client_id, user_session.is_token_valid()) {
            if self.error_state.is_none() {
                let mut configuration = IdConfiguration::new(client_id.clone());
                //configuration.set_auto_select(true);
                let link = ctx.link().clone();
                configuration.set_callback(Box::new(move |response| {
                    link.send_message(AppMessage::TokenReceived(response.credential().to_string()));
                }));
                let link = ctx.link().clone();
                initialize(configuration);
                spawn_local(async move {
                    let result = prompt_async().await;
                    if result != PromptResult::Dismissed(DismissedReason::CredentialReturned) {
                        link.send_message(AppMessage::LoginFailed(result))
                    }
                });
                //prompt(Some(Box::new(|not| info!("Notification: {not:?}"))))
            }
        }
        let context = user_session.clone();
        if context.is_token_valid() {
            html! {
            <ContextProvider<UserSessionData> {context}>
                <Router<AppRoute> default={AppRoute::Home}>
                    <MainPage/>
                </Router<AppRoute>>
            </ContextProvider<UserSessionData>>
            }
        } else if let Some(error) = &self.error_state {
            let error_message = match error {
                ErrorState::FrontendError(FrontendError::JS(js_error)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Javascript Error" r#type={AlertType::Danger}>{js_error.to_string()}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::FrontendError(FrontendError::Serde(serde_error)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Serialization Error" r#type={AlertType::Danger}>{serde_error.to_string()}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::FrontendError(FrontendError::Graphql(graphql_error)) => {
                    let graphql_error = graphql_error.clone();
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Error from Server" r#type={AlertType::Danger}>
                                <ul>
                            {
                              graphql_error.iter().map(|error| {
                                    let message=&error.message;
                                    if let Some(path) = error
                                        .path.as_ref()
                                        .map(|p|
                                            p.iter()
                                                .map(|path| path.to_string())
                                                .collect::<Vec<String>>()
                                                .join("/")
                                        )
                                    {
                                        html!{<li>{message}{" at "}{path}</li>}
                                    }else{
                                        html!{<li>{message}</li>}
                                    }
                                }).collect::<Html>()
                            }
                                </ul>
                            </Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::FrontendError(FrontendError::Reqwest(reqwest_error)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Cannot call Server" r#type={AlertType::Danger}>{reqwest_error.to_string()}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::FrontendError(FrontendError::InvalidHeader(header_error)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Header Error" r#type={AlertType::Danger}>{header_error.to_string()}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::PromptResult(PromptResult::NotDisplayed(
                    NotDisplayedReason::SuppressedByUser,
                )) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Not Displayed" r#type={AlertType::Danger}>{"Suppressed by user"}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::PromptResult(PromptResult::Skipped(skipped_reason)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Skipped" r#type={AlertType::Danger}>{format!("{skipped_reason:?}")}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::PromptResult(PromptResult::Dismissed(reason)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Dismissed" r#type={AlertType::Danger}>{format!("{reason:?}")}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::PromptResult(PromptResult::NotDisplayed(reason)) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Not Displayed" r#type={AlertType::Danger}>{format!("{reason:?}")}</Alert>
                        </AlertGroup>
                    }
                }
            };
            html! {
                <>
                    {error_message}
                    <div ref={self.login_button_ref.clone()}></div>
                </>
            }
        } else {
            html! {
                <h1>{"Logging in"}</h1>
            }
        }
    }
    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if let Some(login_button_ref) = self.login_button_ref.cast::<HtmlElement>() {
            render_button(
                login_button_ref,
                GsiButtonConfiguration::new(ButtonType::Standard),
            );
        }
        if self.user_session.is_token_valid() {
            if let Some(valid_until) = self.user_session.valid_until() {
                if let Some(timer) = self.running_timeout.take() {
                    timer.cancel();
                };
                self.running_timeout = {
                    let link = ctx.link().clone();
                    let valid_until =
                        wasm_timer::SystemTime::UNIX_EPOCH + Duration::new(*valid_until, 0);

                    if let Ok(valid_time) =
                        valid_until.duration_since(wasm_timer::SystemTime::now())
                    {
                        let duration = valid_time.as_millis();
                        Some(Timeout::new((duration as u32).max(2000), move || {
                            link.send_message(AppMessage::CheckSession)
                        }))
                    } else {
                        None
                    }
                };
                /*
                self.timeout = Some(handle);

                self.messages.clear();

                self.messages.push("Timer started!");
                self.console_timer = Some(Timer::new("Timer"));*/
            }
        }
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                let result = query_anonymous::<Settings>(settings::Variables {}).await;
                match result {
                    Ok(settings::ResponseData {
                        authentication: settings::SettingsAuthentication { client_id },
                    }) => {
                        scope.send_message(AppMessage::ClientIdReceived(client_id));
                    }
                    Err(err) => {
                        warn!("Error on server {err:?}");
                        scope.send_message(AppMessage::ClientError(err));
                    }
                }
            });
        }
    }
}
#[function_component(MainPage)]
fn main_page() -> Html {
    html! {
        <BackdropViewer>
            <ToastViewer>
                    <Page sidebar={html_nested! {<PageSidebar><Sidebar/></PageSidebar>}}>
                        <RouterSwitch<AppRoute>
                            render = { switch_main}
                        />
                    </Page>
            </ToastViewer>
        </BackdropViewer>
    }
}
#[function_component(Sidebar)]
pub fn sidebar() -> Html {
    html! {
        <Nav>
            <NavRouterItem<AppRoute> to={AppRoute::Home}>{"Start"}</NavRouterItem<AppRoute>>
        </Nav>
    }
}
