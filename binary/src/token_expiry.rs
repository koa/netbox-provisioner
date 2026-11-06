//! A WebSocket connection checks its token once, at `connection_init`, and could then go on
//! for as long as the tab stays open: queries and mutations too, the socket carries every
//! operation (`graphql-transport-ws`). [`Expiring`] holds each operation to the token's `exp`
//! ([`TokenExpiry`], from the connection's data): one started after it is answered with
//! [`TOKEN_EXPIRED`] only, a running one gets that answer at `exp` and ends. The frontend opens
//! the connection again on it, with its renewed token, and starts its subscriptions again.

use actix_web::rt::time::sleep;
use async_graphql::{
    Data, ErrorExtensionValues, Executor, Request, Response, ServerError,
    futures_util::{
        StreamExt,
        future::{Either, select},
        stream::{self, BoxStream},
    },
};
use chrono::{DateTime, Utc};
use std::{any::TypeId, future::Future, sync::Arc};

/// The code of the error an operation gets once the connection's token has expired.
pub const TOKEN_EXPIRED: &str = "TOKEN_EXPIRED";

/// When the token a WebSocket connection was opened with stops being valid: its `exp` and the
/// clock leeway the token check allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenExpiry(pub DateTime<Utc>);

/// `E`, its operations held to the [`TokenExpiry`] in their session data.
#[derive(Clone)]
pub struct Expiring<E>(pub E);

impl<E: Executor> Executor for Expiring<E> {
    fn execute(&self, request: Request) -> impl Future<Output = Response> + Send {
        self.0.execute(request)
    }

    fn execute_stream(&self, request: Request, session_data: Option<Arc<Data>>) -> BoxStream<'static, Response> {
        let expiry = session_data.as_deref().and_then(|data| {
            data.get(&TypeId::of::<TokenExpiry>())
                .and_then(|expiry| expiry.downcast_ref::<TokenExpiry>())
                .copied()
        });
        // a connection without one wasn't let in by `on_connection_init`
        let Some(TokenExpiry(expiry)) = expiry else {
            return stream::once(async { expired() }).boxed();
        };
        let Ok(left) = (expiry - Utc::now()).to_std() else {
            return stream::once(async { expired() }).boxed();
        };
        let answers = self.0.execute_stream(request, session_data);
        let deadline = Box::pin(sleep(left));
        stream::unfold(Some((answers, deadline)), |state| async move {
            let (mut answers, deadline) = state?;
            match select(answers.next(), deadline).await {
                Either::Left((Some(answer), deadline)) => Some((answer, Some((answers, deadline)))),
                Either::Left((None, _)) => None,
                Either::Right(_) => Some((expired(), None)),
            }
        })
        .boxed()
    }
}

fn expired() -> Response {
    let mut error = ServerError::new("token expired", None);
    let mut extensions = ErrorExtensionValues::default();
    extensions.set("code", TOKEN_EXPIRED);
    error.extensions = Some(extensions);
    Response::from_errors(vec![error])
}

#[cfg(test)]
mod test {
    use super::*;
    use async_graphql::{EmptyMutation, Object, Schema, Subscription};
    use std::time::Duration;

    struct Query;

    #[Object]
    impl Query {
        async fn value(&self) -> u32 {
            1
        }
    }

    struct Ticks;

    #[Subscription]
    impl Ticks {
        /// a number every 100 ms, without end
        async fn ticks(&self) -> impl async_graphql::futures_util::Stream<Item = u32> {
            stream::unfold(0, |n| async move {
                sleep(Duration::from_millis(100)).await;
                Some((n, n + 1))
            })
        }
    }

    fn executor() -> Expiring<Schema<Query, EmptyMutation, Ticks>> {
        Expiring(Schema::new(Query, EmptyMutation, Ticks))
    }

    fn session(expiry: Option<DateTime<Utc>>) -> Option<Arc<Data>> {
        let mut data = Data::default();
        if let Some(expiry) = expiry {
            data.insert(TokenExpiry(expiry));
        }
        Some(Arc::new(data))
    }

    fn is_expired(response: &Response) -> bool {
        response.errors.iter().any(|error| {
            error
                .extensions
                .as_ref()
                .and_then(|extensions| extensions.get("code"))
                .is_some_and(|code| *code == async_graphql::Value::from(TOKEN_EXPIRED))
        })
    }

    #[actix_web::test]
    async fn an_operation_after_the_expiry_gets_token_expired_only() {
        let past = Utc::now() - chrono::Duration::seconds(1);
        for query in ["{ value }", "subscription { ticks }"] {
            let answers: Vec<_> = executor().execute_stream(Request::new(query), session(Some(past))).collect().await;
            assert_eq!(answers.len(), 1, "{query}");
            assert!(is_expired(&answers[0]), "{query}: {:?}", answers[0]);
        }
    }

    #[actix_web::test]
    async fn a_connection_without_expiry_gets_token_expired() {
        let answers: Vec<_> = executor().execute_stream(Request::new("{ value }"), session(None)).collect().await;
        assert_eq!(answers.len(), 1);
        assert!(is_expired(&answers[0]));
    }

    #[actix_web::test]
    async fn a_query_before_the_expiry_is_answered() {
        let future = Utc::now() + chrono::Duration::seconds(60);
        let answers: Vec<_> = executor().execute_stream(Request::new("{ value }"), session(Some(future))).collect().await;
        assert_eq!(answers.len(), 1);
        assert!(answers[0].errors.is_empty(), "{:?}", answers[0]);
    }

    #[actix_web::test]
    async fn a_subscription_ends_with_token_expired_at_the_expiry() {
        let soon = Utc::now() + chrono::Duration::milliseconds(450);
        let answers: Vec<_> =
            executor().execute_stream(Request::new("subscription { ticks }"), session(Some(soon))).collect().await;
        let (last, ticks) = answers.split_last().expect("answers");
        assert!(is_expired(last), "{last:?}");
        assert!((2..=5).contains(&ticks.len()), "{} ticks before the expiry", ticks.len());
        assert!(ticks.iter().all(|tick| tick.errors.is_empty()));
    }
}
