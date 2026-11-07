//! Scope: The note on every page once the server refuses the token; nothing else.
//!
//! The note on top of every page once the server refuses the token (E20, Z34): without a
//! refresh token (Pocket-ID without `offline_access`) `yew-oauth2` keeps an expired one as
//! authenticated, and every request gets `403`. "Erneut anmelden" leaves for the provider and
//! comes back to the page; nothing redirects by itself, so an open form stays as it is until
//! the user decides. Whether the server takes the token comes from `auth` (its `report`); the
//! note goes away once it does again.
use crate::{
    auth::{self, start_login},
    i18n::login::{AGAIN, EXPIRED, EXPIRED_TEXT, failed},
};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use yew::{Component, Context, Html, html};

/// The note, shown while the server refuses the token.
pub struct LoginExpired {
    id: u64,
    refused: bool,
    /// starting the login failed, e.g. storing its state
    login_error: Option<String>,
}

pub enum LoginExpiredMessage {
    Refused(bool),
    Login,
}

impl Component for LoginExpired {
    type Message = LoginExpiredMessage;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            id: auth::watch(ctx.link().callback(LoginExpiredMessage::Refused)),
            refused: auth::refused(),
            login_error: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            LoginExpiredMessage::Refused(refused) => self.refused = refused,
            LoginExpiredMessage::Login => self.login_error = start_login(ctx.link()),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        if !self.refused {
            return Html::default();
        }
        html! {
            <div class="login-expired">
                <Alert inline=true title={EXPIRED} r#type={AlertType::Warning}>
                    <p>{EXPIRED_TEXT}</p>
                    <Button
                        variant={ButtonVariant::Primary}
                        label={AGAIN}
                        onclick={ctx.link().callback(|_| LoginExpiredMessage::Login)}
                    />
                    if let Some(failure) = &self.login_error { <p>{failed(failure)}</p> }
                </Alert>
            </div>
        }
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        auth::unwatch(self.id);
    }
}
