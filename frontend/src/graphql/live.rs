//! Subscriptions over one WebSocket per tab (`graphql-transport-ws` to `/graphql/ws`,
//! `backend::graphql::live`): the server answers again whenever what an answer showed changed,
//! whoever changed it. The connection is opened with the first subscription and comes back by
//! itself after it broke (with the steps of [`RETRY`]), the subscriptions started again: their
//! first answer brings each view up to date.
use crate::{components::job::RETRY, error::FrontendError, graphql::host};
use futures::{SinkExt, StreamExt, channel::mpsc};
use gloo::net::websocket::{Message, futures::WebSocket};
use graphql_client::{GraphQLQuery, Response};
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
use yew_oauth2::prelude::{Authentication, OAuth2Context};

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

/// Subscribes to `Q` with `variables`, each answer going to `deliver`.
pub fn subscribe<Q: GraphQLQuery + 'static, S: Component>(
    scope: &Scope<S>,
    variables: Q::Variables,
    deliver: impl Fn(Result<Response<Q::ResponseData>, FrontendError>) + 'static,
) -> Subscription {
    let payload = serde_json::to_value(Q::build_query(variables)).unwrap_or_default();
    let deliver: Rc<dyn Fn(Value)> = Rc::new(move |answer| {
        deliver(serde_json::from_value(answer).map_err(FrontendError::from));
    });
    CONNECTION.with(|connection| {
        let scope = scope.clone();
        *connection.token.borrow_mut() = Some(Rc::new(move || token(&scope)));
        let id = connection.next.get() + 1;
        connection.next.set(id);
        connection.send(subscribe_message(id, &payload));
        connection.queries.borrow_mut().insert(id, Query { payload, deliver });
        if !connection.running.replace(true) {
            spawn_local(run());
        }
        Subscription { id }
    })
}

/// [`subscribe`] taking the data as `query_authenticated` does: without data, the errors.
pub fn subscribe_data<Q: GraphQLQuery + 'static, S: Component>(
    scope: &Scope<S>,
    variables: Q::Variables,
    deliver: impl Fn(Result<Q::ResponseData, FrontendError>) + 'static,
) -> Subscription {
    subscribe::<Q, S>(scope, variables, move |response| {
        deliver(response.and_then(|response| {
            response
                .data
                .ok_or_else(|| FrontendError::Graphql(response.errors.unwrap_or_default()))
        }))
    })
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

/// The token the backend takes (as `query_authenticated_response`): the ID token, which
/// carries the user's name, else the access token.
fn token<S: Component>(scope: &Scope<S>) -> Option<String> {
    match scope.context::<OAuth2Context>(Default::default()) {
        Some((OAuth2Context::Authenticated(Authentication { access_token, id_token, .. }), _)) => {
            Some(id_token.unwrap_or(access_token))
        }
        _ => None,
    }
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
    let token = connection.token.borrow().as_ref().and_then(|token| token()).unwrap_or_default();
    let init = json!({"type": "connection_init", "payload": {"token": token}});
    if write.send(Message::Text(init.to_string())).await.is_err() {
        return false;
    }
    match read.next().await {
        Some(Ok(Message::Text(text))) if message_type(&text).as_deref() == Some("connection_ack") => {}
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
        match message["type"].as_str() {
            Some("next") => {
                let id = message["id"].as_str().and_then(|id| id.parse::<u64>().ok());
                let deliver = id.and_then(|id| connection.queries.borrow().get(&id).map(|q| q.deliver.clone()));
                if let Some(deliver) = deliver {
                    deliver(message["payload"].clone());
                }
            }
            // a subscription the server refused (not valid): as a response with its errors
            Some("error") => {
                let id = message["id"].as_str().and_then(|id| id.parse::<u64>().ok());
                let deliver = id.and_then(|id| connection.queries.borrow().get(&id).map(|q| q.deliver.clone()));
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

fn message_type(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text).ok()?["type"].as_str().map(String::from)
}
