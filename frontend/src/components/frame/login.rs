//! The screen without a session: it starts the login, or shows why it failed with a button to
//! try again.
use crate::{auth::start_login, i18n};
use patternfly_yew::prelude::{Alert, AlertType, Bullseye, Button, ButtonVariant, Spinner};
use yew::{Component, Context, ContextHandle, Html, html};
use yew_oauth2::context::OAuth2Context;

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
            LoginMsg::Start => self.start_error = start_login(ctx.link()),
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
                <Alert inline=true title={i18n::login::failed(&failure)} r#type={AlertType::Danger}>
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={i18n::login::AGAIN}
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
