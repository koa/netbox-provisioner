use crate::{
    error::FrontendError,
    graphql::{
        anonymous::{Settings, settings},
        query_anonymous,
    },
};
use patternfly_yew::prelude::{
    Alert, AlertType, BackdropViewer, Bullseye, Button, ButtonVariant, Spinner, ToastViewer,
};
use routes::AppRoute;
use yew::{
    Callback, Component, Context, ContextHandle, Html, Properties, function_component, html,
    platform::spawn_local,
};
use yew_nested_router::{Router, prelude::Switch as RouterSwitch};
use yew_oauth2::{
    agent::OAuth2Operations,
    components::context::Agent,
    context::OAuth2Context,
    openid::{Client, Config, OAuth2},
    prelude::Authenticated,
};

pub mod devices;
pub mod routes;
pub mod sites;

/// Where the provider for the login is, from the backend (`authentication`).
#[derive(Debug, Clone, PartialEq)]
pub struct AuthenticationData {
    client_id: String,
    issuer_url: String,
    scopes: Vec<String>,
}

#[derive(Debug)]
pub enum App {
    Pending,
    Loaded(AuthenticationData),
    Failed(FrontendError),
}
#[derive(Debug)]
pub enum AppMessage {
    AuthenticationData(AuthenticationData),
    Error(FrontendError),
}

impl Component for App {
    type Message = AppMessage;
    type Properties = ();
    fn create(_ctx: &Context<Self>) -> Self {
        App::Pending
    }
    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        *self = match msg {
            AppMessage::AuthenticationData(config) => App::Loaded(config),
            AppMessage::Error(e) => App::Failed(e),
        };
        true
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        match self {
            App::Failed(error) => html!(<Bullseye>{error.to_html()}</Bullseye>),
            App::Loaded(config) => html!(<MainOAuth2 config={config.clone()}/>),
            App::Pending => html!(<Bullseye><Spinner/></Bullseye>),
        }
    }
    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                scope.send_message(
                    match query_anonymous::<Settings>(settings::Variables {}).await {
                        Ok(settings::ResponseData {
                            authentication:
                                settings::SettingsAuthentication {
                                    client_id,
                                    issuer_url,
                                    scopes,
                                },
                        }) => AppMessage::AuthenticationData(AuthenticationData {
                            client_id,
                            issuer_url,
                            scopes,
                        }),
                        Err(error) => AppMessage::Error(error),
                    },
                );
            });
        }
    }
}
#[derive(Properties, Clone, PartialEq, Debug)]
pub struct MainOAuth2Props {
    config: AuthenticationData,
}
#[function_component(MainOAuth2)]
pub fn main_oauth2(props: &MainOAuth2Props) -> Html {
    let config = Config::new(props.config.client_id.clone(), props.config.issuer_url.clone());
    let scopes = props.config.scopes.clone();
    html! {
     <OAuth2 {config} {scopes}>
        <Authenticated>
            <Router<AppRoute> default={AppRoute::default()}>
                <MainPage/>
            </Router<AppRoute>>
        </Authenticated>
        <Login/>
      </OAuth2>
    }
}

/// Starts the login when there is no session. A failed login is shown with a button to try
/// again instead of starting the next login right away, which would redirect forever.
pub struct Login {
    auth: Option<OAuth2Context>,
    _auth_handle: Option<ContextHandle<OAuth2Context>>,
    /// Starting the login failed, e.g. storing its state
    start_error: Option<String>,
}

pub enum LoginMsg {
    Auth(OAuth2Context),
    Start,
}

impl Component for Login {
    type Message = LoginMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let (auth, auth_handle) = ctx
            .link()
            .context::<OAuth2Context>(ctx.link().callback(LoginMsg::Auth))
            .unzip();
        let login = Self {
            auth,
            _auth_handle: auth_handle,
            start_error: None,
        };
        login.start_if_needed(ctx);
        login
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            LoginMsg::Auth(auth) => {
                self.auth = Some(auth);
                self.start_if_needed(ctx);
            }
            LoginMsg::Start => {
                self.start_error = ctx
                    .link()
                    .context::<Agent<Client>>(Callback::noop())
                    .and_then(|(agent, _)| agent.start_login().err())
                    .map(|error| error.to_string());
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let failure = match (&self.start_error, &self.auth) {
            (Some(error), _) => error.clone(),
            (None, Some(OAuth2Context::Failed(error))) => error.clone(),
            (None, Some(OAuth2Context::NotAuthenticated { .. })) => {
                return html!(<Bullseye><Spinner/></Bullseye>);
            }
            _ => return Html::default(),
        };
        html! {
            <Bullseye>
                <Alert inline=true title={format!("Anmeldung fehlgeschlagen: {failure}")} r#type={AlertType::Danger}>
                    <Button
                        variant={ButtonVariant::Secondary}
                        label="Erneut anmelden"
                        onclick={ctx.link().callback(|_| LoginMsg::Start)}
                    />
                </Alert>
            </Bullseye>
        }
    }
}

impl Login {
    /// Only without a session: after a failure the user decides when to try again.
    fn start_if_needed(&self, ctx: &Context<Self>) {
        if matches!(self.auth, Some(OAuth2Context::NotAuthenticated { .. })) {
            ctx.link().send_message(LoginMsg::Start);
        }
    }
}
#[function_component(MainPage)]
fn main_page() -> Html {
    html! {
        <BackdropViewer>
            <ToastViewer>
                    <RouterSwitch<AppRoute> render={AppRoute::content}/>
            </ToastViewer>
        </BackdropViewer>
    }
}
