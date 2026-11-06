//! Subscriptions over one WebSocket per tab (`graphql-transport-ws` to `/graphql/ws`,
//! `backend::graphql::live`): the server answers again whenever what an answer showed changed,
//! whoever changed it. The connection is opened with the first subscription and comes back by
//! itself after it broke (with the steps of [`RETRY`]), the subscriptions started again: their
//! first answer brings each view up to date. The server ends every operation at the expiry of
//! the token the connection was opened with (`TOKEN_EXPIRED`), so that answer opens it again
//! too, with the token as it is now, instead of going to the view.
use crate::{
    components::{job::RETRY, login_expired},
    error::FrontendError,
    graphql::{Response, host, token},
};
use cynic::StreamingOperation;
use futures::{SinkExt, StreamExt, channel::mpsc};
use gloo::net::websocket::{Message, WebSocketError, futures::WebSocket};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
use yew::{
    Component,
    html::Scope,
    platform::{spawn_local, time::sleep},
};

/// A subscription running; dropping it ends it.
pub struct Subscription {
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        CONNECTION.with(|connection| {
            connection.queries.borrow_mut().remove(&self.id);
            connection.send(json!({"type": "complete", "id": self.id.to_string()}));
        });
    }
}

/// Subscribes to `subscription` (cynic), each answer going to `deliver`.
pub fn watch<T: DeserializeOwned + 'static, V: Serialize, S: Component>(
    scope: &Scope<S>,
    subscription: StreamingOperation<T, V>,
    deliver: impl Fn(Result<Response<T>, FrontendError>) + 'static,
) -> Subscription {
    start(
        scope,
        serde_json::to_value(subscription).unwrap_or_default(),
        deliver,
    )
}

/// [`watch`] taking the data as `request` does: without data, the errors.
pub fn watch_data<T: DeserializeOwned + 'static, V: Serialize, S: Component>(
    scope: &Scope<S>,
    subscription: StreamingOperation<T, V>,
    deliver: impl Fn(Result<T, FrontendError>) + 'static,
) -> Subscription {
    watch(scope, subscription, move |response| deliver(data(response)))
}

fn data<T>(response: Result<Response<T>, FrontendError>) -> Result<T, FrontendError> {
    response.and_then(|response| {
        response
            .data
            .ok_or_else(|| FrontendError::Graphql(response.errors.unwrap_or_default()))
    })
}

fn start<T: DeserializeOwned + 'static, S: Component>(
    scope: &Scope<S>,
    payload: Value,
    deliver: impl Fn(Result<Response<T>, FrontendError>) + 'static,
) -> Subscription {
    let deliver: Rc<dyn Fn(Value)> = Rc::new(move |answer| {
        deliver(serde_json::from_value(answer).map_err(FrontendError::from));
    });
    CONNECTION.with(|connection| {
        let scope = scope.clone();
        *connection.token.borrow_mut() = Some(Rc::new(move || token(&scope)));
        let id = connection.next.get() + 1;
        connection.next.set(id);
        connection.send(subscribe_message(id, &payload));
        connection
            .queries
            .borrow_mut()
            .insert(id, Query { payload, deliver });
        if !connection.running.replace(true) {
            spawn_local(run());
        }
        Subscription { id }
    })
}

/// When something was read that is `age` seconds old (milliseconds since the epoch): ages come
/// from the server, times are the browser's.
pub fn read_at(age: i32) -> f64 {
    js_sys::Date::now() - f64::from(age) * 1000.0
}

/// The time `new` of an answer again, but `before` where it is the same reading: an age comes
/// in whole seconds, so [`read_at`] of one reading moves by up to a second between answers.
pub fn same_reading(before: Option<f64>, new: f64) -> f64 {
    before
        .filter(|before| (before - new).abs() < 1500.0)
        .unwrap_or(new)
}

/// `HH:MM:SS` of `millis` since the epoch, in the browser's time zone.
pub fn local_time(millis: f64) -> String {
    let date = js_sys::Date::new(&millis.into());
    format!(
        "{:02}:{:02}:{:02}",
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds()
    )
}

struct Query {
    payload: Value,
    deliver: Rc<dyn Fn(Value)>,
}

type Token = Rc<dyn Fn() -> Option<String>>;

#[derive(Default)]
struct Connection {
    queries: RefCell<HashMap<u64, Query>>,
    /// to the open connection, `None` while there is none
    outgoing: RefCell<Option<mpsc::UnboundedSender<String>>>,
    /// gives the user's token as it is now (renewed while the page is open), from the
    /// component of the latest subscription started
    token: RefCell<Option<Token>>,
    next: Cell<u64>,
    /// [`run`] keeps the connection
    running: Cell<bool>,
}

impl Connection {
    /// Sends `message` if connected; else it goes when connected (a subscription) or needn't
    /// (its end).
    fn send(&self, message: Value) {
        if let Some(outgoing) = self.outgoing.borrow().as_ref() {
            let _ = outgoing.unbounded_send(message.to_string());
        }
    }
}

thread_local! {
    static CONNECTION: Rc<Connection> = Rc::default();
}

fn subscribe_message(id: u64, payload: &Value) -> Value {
    json!({"type": "subscribe", "id": id.to_string(), "payload": payload})
}

/// Keeps the connection while there are subscriptions, opening it again after it broke.
async fn run() {
    let connection = CONNECTION.with(Rc::clone);
    let mut failed = 0;
    while !connection.queries.borrow().is_empty() {
        if session(&connection).await {
            failed = 0;
        }
        *connection.outgoing.borrow_mut() = None;
        if connection.queries.borrow().is_empty() {
            break;
        }
        sleep(RETRY[failed.min(RETRY.len() - 1)]).await;
        failed += 1;
    }
    connection.running.set(false);
}

/// One connection, until it breaks; whether it was accepted.
async fn session(connection: &Rc<Connection>) -> bool {
    let url = format!("{}/graphql/ws", host().replacen("http", "ws", 1));
    let Ok(socket) = WebSocket::open_with_protocol(&url, "graphql-transport-ws") else {
        return false;
    };
    let (mut write, mut read) = socket.split();
    let token = connection
        .token
        .borrow()
        .as_ref()
        .and_then(|token| token())
        .unwrap_or_default();
    let init = json!({"type": "connection_init", "payload": {"token": token}});
    if write.send(Message::Text(init.to_string())).await.is_err() {
        return false;
    }
    match read.next().await {
        Some(Ok(Message::Text(text)))
            if message_type(&text).as_deref() == Some("connection_ack") =>
        {
            login_expired::report(false);
        }
        // the token refused (`on_connection_init`): tried again, the note asks to log in
        Some(Err(WebSocketError::ConnectionClose(close)))
            if login_expired::ws_refuses_token(close.code, &close.reason) =>
        {
            login_expired::report(true);
            return false;
        }
        _ => return false,
    }
    let (outgoing, mut sending) = mpsc::unbounded::<String>();
    spawn_local(async move {
        while let Some(message) = sending.next().await {
            if write.send(Message::Text(message)).await.is_err() {
                break;
            }
        }
    });
    for (id, query) in connection.queries.borrow().iter() {
        let _ = outgoing.unbounded_send(subscribe_message(*id, &query.payload).to_string());
    }
    *connection.outgoing.borrow_mut() = Some(outgoing);
    while let Some(Ok(Message::Text(text))) = read.next().await {
        let Ok(message) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if token_expired(&message) {
            break;
        }
        match message["type"].as_str() {
            Some("next") => {
                let id = message["id"].as_str().and_then(|id| id.parse::<u64>().ok());
                let deliver = id.and_then(|id| {
                    connection
                        .queries
                        .borrow()
                        .get(&id)
                        .map(|q| q.deliver.clone())
                });
                if let Some(deliver) = deliver {
                    deliver(message["payload"].clone());
                }
            }
            // a subscription the server refused (not valid): as a response with its errors
            Some("error") => {
                let id = message["id"].as_str().and_then(|id| id.parse::<u64>().ok());
                let deliver = id.and_then(|id| {
                    connection
                        .queries
                        .borrow()
                        .get(&id)
                        .map(|q| q.deliver.clone())
                });
                if let Some(deliver) = deliver {
                    deliver(json!({"data": null, "errors": message["payload"]}));
                }
            }
            Some("ping") => connection.send(json!({"type": "pong"})),
            _ => {}
        }
    }
    true
}

/// Whether `message` is the server's answer to an operation after the connection's token
/// expired: errors with the code `TOKEN_EXPIRED`, in a `next` (a response) or an `error` (the
/// errors alone).
fn token_expired(message: &Value) -> bool {
    let errors = match message["type"].as_str() {
        Some("next") => &message["payload"]["errors"],
        Some("error") => &message["payload"],
        _ => return false,
    };
    errors.as_array().is_some_and(|errors| {
        errors
            .iter()
            .any(|error| error["extensions"]["code"] == "TOKEN_EXPIRED")
    })
}

fn message_type(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text).ok()?["type"]
        .as_str()
        .map(String::from)
}

#[cfg(test)]
mod test {
    use super::token_expired;
    use serde_json::json;

    #[test]
    fn the_answer_after_the_token_expired_is_told_apart() {
        let expired =
            json!([{"message": "token expired", "extensions": {"code": "TOKEN_EXPIRED"}}]);
        assert!(token_expired(
            &json!({"type": "next", "id": "1", "payload": {"data": null, "errors": expired}})
        ));
        assert!(token_expired(
            &json!({"type": "error", "id": "1", "payload": expired})
        ));
        let other = json!([{"message": "no device", "extensions": {"code": "NOT_FOUND"}}]);
        assert!(!token_expired(
            &json!({"type": "next", "id": "1", "payload": {"data": null, "errors": other}})
        ));
        assert!(!token_expired(
            &json!({"type": "next", "id": "1", "payload": {"data": {"x": 1}}})
        ));
    }
}
