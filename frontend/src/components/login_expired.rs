//! The note on top of every page once the server refuses the token (E20, Z34): without a
//! refresh token (Pocket-ID without `offline_access`) `yew-oauth2` keeps an expired one as
//! authenticated, and every request gets `403`. "Erneut anmelden" leaves for the provider and
//! comes back to the page; nothing redirects by itself, so an open form stays as it is until
//! the user decides. The requests (`graphql::post`) and the WebSocket (`graphql::live`) [`report`]
//! whether the server took the token; the note goes away once it does again.
use crate::{
    i18n::login::{AGAIN, EXPIRED, EXPIRED_TEXT, failed},
    pages::start_login,
};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use std::cell::{Cell, RefCell};
use yew::{Callback, Component, Context, Html, html};

/// What the server answers a user it knows but doesn't let in (`require_allowed_user`): not a
/// login that expired, logging in again changes nothing.
const NOT_ALLOWED: &str = "user not in auth_users";

/// The close code async-graphql ends a WebSocket with whose `connection_init` it refused
/// (`graphql-transport-ws`), the error as the reason.
const WS_INIT_REFUSED: u16 = 1002;

thread_local! {
    static REFUSED: Cell<bool> = const { Cell::new(false) };
    static WATCHERS: RefCell<Vec<(u64, Callback<bool>)>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Whether a refusal with `text` (a request's `401`/`403`, the WebSocket's close reason) is the
/// server refusing the token, rather than the user.
pub fn refuses_token(text: &str) -> bool {
    text.trim() != NOT_ALLOWED
}

/// Whether a WebSocket closed with `code` and `reason` before its `connection_ack` was the
/// server refusing the token: `token refused …` or `token without exp` (`authenticate`), not a
/// user it doesn't let in.
pub fn ws_refuses_token(code: u16, reason: &str) -> bool {
    code == WS_INIT_REFUSED && reason.starts_with("token ")
}

/// Whether the server refused the token (`true`) or took it (`false`); the note follows.
pub fn report(refused: bool) {
    if REFUSED.replace(refused) == refused {
        return;
    }
    let watchers = WATCHERS.with_borrow(|watchers| {
        watchers
            .iter()
            .map(|(_, watcher)| watcher.clone())
            .collect::<Vec<_>>()
    });
    for watcher in watchers {
        watcher.emit(refused);
    }
}

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
        let id = NEXT.replace(NEXT.get() + 1);
        let watcher = ctx.link().callback(LoginExpiredMessage::Refused);
        WATCHERS.with_borrow_mut(|watchers| watchers.push((id, watcher)));
        Self {
            id,
            refused: REFUSED.get(),
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
        let id = self.id;
        WATCHERS.with_borrow_mut(|watchers| watchers.retain(|(watcher, _)| *watcher != id));
    }
}

#[cfg(test)]
mod test {
    use super::{refuses_token, ws_refuses_token};

    #[test]
    fn a_user_not_let_in_is_no_expired_login() {
        assert!(refuses_token("Token does not have sufficient rights"));
        assert!(refuses_token("token refused"));
        assert!(refuses_token(""));
        assert!(!refuses_token("user not in auth_users"));
        assert!(ws_refuses_token(1002, "token refused"));
        assert!(ws_refuses_token(1002, "token refused: InvalidSignature"));
        assert!(!ws_refuses_token(1002, "user not in auth_users"));
        assert!(!ws_refuses_token(1006, ""));
    }
}
