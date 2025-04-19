use crate::{
    data::UserSessionData,
    error::FrontendError,
    graphql::{
        anonymous::{Settings, settings},
        query_anonymous,
    },
};
use gloo::timers::callback::Timeout;
use google_signin_client::{
    ButtonType, DismissedReason, GsiButtonConfiguration, IdConfiguration, NotDisplayedReason,
    PromptResult, initialize, prompt_async, render_button,
};
use log::{info, warn};
use patternfly_yew::prelude::{
    Alert, AlertGroup, AlertType, BackdropViewer, Nav, NavItem, NavRouterItem, Page, PageSidebar,
    ToastViewer,
};
use routes::{AppRoute, Sidebar};
use std::time::Duration;
use web_sys::HtmlElement;
use yew::{
    Context, ContextProvider, Html, NodeRef, Properties, ToHtml, function_component, html,
    html_nested, platform::spawn_local,
};
use yew_nested_router::{Router, prelude::Switch as RouterSwitch};

pub mod devices;
pub mod routes;

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
                    info!("Got token response: {}", response.credential());
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
                <Router<AppRoute> default={AppRoute::default()}>
                    <MainPage/>
                </Router<AppRoute>>
            </ContextProvider<UserSessionData>>
            }
        } else if let Some(error) = &self.error_state {
            let error_message = match error {
                ErrorState::FrontendError(e) => e.to_html(),
                ErrorState::PromptResult(PromptResult::NotDisplayed) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Not Displayed" r#type={AlertType::Danger}>{"Prompt was not displayed to the user"}</Alert>
                        </AlertGroup>
                    }
                }
                ErrorState::PromptResult(PromptResult::Skipped) => {
                    html! {
                        <AlertGroup>
                            <Alert inline=true title="Skipped" r#type={AlertType::Danger}>{"Prompt skipped by the user"}</Alert>
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
                            render = { AppRoute::content}
                        />
                    </Page>
            </ToastViewer>
        </BackdropViewer>
    }
}
