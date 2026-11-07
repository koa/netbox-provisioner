//! Scope: The login: leaving for the provider and whether the server takes the token; not the
//! screens for it (`components::frame`).
//!
//! The login: leaving for the provider ([`start_login`]), and whether the server takes the token
//! (E20): the requests (`graphql::post`) and the WebSocket (`graphql::live`) [`report`] it, the
//! note `components::frame::login_expired` [`watch`]es it.
use std::cell::{Cell, RefCell};
use yew::{Callback, Component, html::Scope};
use yew_oauth2::{agent::OAuth2Operations, components::context::Agent, openid::Client};

/// Leaves for the provider's login, which comes back to the current page; the error if it
/// can't start. Also for a session whose token the server refuses: without a refresh token
/// (Pocket-ID without `offline_access`) `yew-oauth2` keeps an expired one as authenticated.
pub(crate) fn start_login<C: Component>(scope: &Scope<C>) -> Option<String> {
    scope
        .context::<Agent<Client>>(Callback::noop())
        .and_then(|(agent, _)| agent.start_login().err())
        .map(|error| error.to_string())
}

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

/// Whether the server refuses the token now.
pub fn refused() -> bool {
    REFUSED.get()
}

/// `watcher` gets each change of [`refused`] until [`unwatch`] with the id given back.
pub fn watch(watcher: Callback<bool>) -> u64 {
    let id = NEXT.replace(NEXT.get() + 1);
    WATCHERS.with_borrow_mut(|watchers| watchers.push((id, watcher)));
    id
}

pub fn unwatch(id: u64) {
    WATCHERS.with_borrow_mut(|watchers| watchers.retain(|(watcher, _)| *watcher != id));
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
