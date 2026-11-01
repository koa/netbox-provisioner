use super::{Came, Change, FactKey, FactKind, Freshness, LIMIT, Reading, get, invalidate, reading};
use crate::{Error, topology::DeviceId};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::time::{Instant, sleep};

/// A key of its own per test: the store is shared by all of them.
fn key(device: u32) -> FactKey {
    FactKey::new(FactKind::Config, Some(DeviceId(device)), "")
}

/// Counts its askings and how many ran at once at most.
#[derive(Default, Clone)]
struct Asker {
    asked: Arc<AtomicU32>,
    running: Arc<AtomicU32>,
    most: Arc<AtomicU32>,
}

impl Asker {
    fn ask(&self, took: Duration, answer: Result<u32, Error>) -> impl Future<Output = Result<u32, Error>> + Send + 'static {
        let asker = self.clone();
        async move {
            asker.asked.fetch_add(1, Ordering::SeqCst);
            let now = asker.running.fetch_add(1, Ordering::SeqCst) + 1;
            asker.most.fetch_max(now, Ordering::SeqCst);
            sleep(took).await;
            asker.running.fetch_sub(1, Ordering::SeqCst);
            answer
        }
    }
    fn asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }
}

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);

#[tokio::test(start_paused = true)]
async fn a_second_user_joins_the_asking() {
    let asker = Asker::default();
    let (first, second) = tokio::join!(
        get(key(1), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(7))),
        get(key(1), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(8))),
    );
    assert_eq!(asker.asked(), 1);
    assert_eq!(*first.value.unwrap(), 7);
    assert_eq!(*second.value.unwrap(), 7);
    assert_eq!((first.came, second.came), (Came::Asked, Came::Joined));
}

#[tokio::test(start_paused = true)]
async fn a_fact_young_enough_comes_from_the_store() {
    let asker = Asker::default();
    get(key(2), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(1))).await;
    sleep(30 * SECOND).await;
    let fact = get(key(2), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(2))).await;
    assert_eq!((*fact.value.unwrap(), fact.came), (1, Came::Hit));
    // the user decides: younger than the fact is
    let fact = get(key(2), Freshness::MaxAge(10 * SECOND), asker.ask(SECOND, Ok(3))).await;
    assert_eq!((*fact.value.unwrap(), fact.came), (3, Came::Asked));
    assert_eq!(asker.asked(), 2);
}

/// the age counts from the start of the asking: what happened meanwhile it may have missed
#[tokio::test(start_paused = true)]
async fn the_age_counts_from_the_start() {
    let asker = Asker::default();
    let start = Instant::now();
    let fact = get(key(3), Freshness::MaxAge(MINUTE), asker.ask(10 * SECOND, Ok(1))).await;
    assert_eq!(fact.as_of, start);
    assert_eq!(fact.age(), 10 * SECOND);
}

/// `Since` doesn't take an asking begun before: it waits for its end and asks then, never two
/// at once
#[tokio::test(start_paused = true)]
async fn since_waits_for_an_older_asking() {
    let asker = Asker::default();
    let older = tokio::spawn(get(key(4), Freshness::MaxAge(MINUTE), asker.ask(5 * SECOND, Ok(1))));
    sleep(SECOND).await;
    let since = Instant::now();
    let fact = get(key(4), Freshness::Since(since), asker.ask(5 * SECOND, Ok(2))).await;
    assert_eq!(*older.await.unwrap().value.unwrap(), 1);
    assert_eq!((*fact.value.unwrap(), fact.came), (2, Came::Asked));
    assert!(fact.as_of >= since);
    assert_eq!(asker.most.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn failures_are_facts_too() {
    let asker = Asker::default();
    let first = get(key(5), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Err(Error::Timeout([10, 0, 0, 1].into())))).await;
    assert!(matches!(first.result().unwrap_err().root(), Error::Timeout(_)));
    let again = get(key(5), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(1))).await;
    assert_eq!(again.came, Came::Hit);
    assert!(again.value.is_err());
    assert_eq!(asker.asked(), 1);
}

/// a job on the device ended while it was asked: the answer may be from before, so it goes to
/// who waits but not into the store
#[tokio::test(start_paused = true)]
async fn an_answer_from_before_a_job_ended_isnt_kept() {
    let asker = Asker::default();
    let asking = tokio::spawn(get(key(6), Freshness::MaxAge(MINUTE), asker.ask(5 * SECOND, Ok(1))));
    sleep(SECOND).await;
    invalidate(DeviceId(6), &[FactKind::Config]);
    assert_eq!(*asking.await.unwrap().value.unwrap(), 1);
    let fact = get(key(6), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(2))).await;
    assert_eq!((*fact.value.unwrap(), fact.came), (2, Came::Asked));
}

#[tokio::test(start_paused = true)]
async fn invalidating_drops_the_kinds_named() {
    let asker = Asker::default();
    let routeros = FactKey::new(FactKind::RouterosState, Some(DeviceId(7)), "");
    get(key(7), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(1))).await;
    get(routeros.clone(), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(1))).await;
    invalidate(DeviceId(7), &[FactKind::Config]);
    let config = get(key(7), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(2))).await;
    let kept = get(routeros, Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(2))).await;
    assert_eq!((config.came, kept.came), (Came::Asked, Came::Hit));
}

#[tokio::test(start_paused = true)]
async fn nothing_is_kept_over_the_limit() {
    let asker = Asker::default();
    get(key(8), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(1))).await;
    sleep(LIMIT + MINUTE).await;
    let fact = get(key(8), Freshness::MaxAge(Duration::from_secs(3600)), asker.ask(SECOND, Ok(2))).await;
    assert_eq!(fact.came, Came::Asked);
}

/// a user going away leaves the asking to the others and the store
#[tokio::test(start_paused = true)]
async fn the_asking_outlives_its_first_user() {
    let asker = Asker::default();
    let first = tokio::spawn(get(key(9), Freshness::MaxAge(MINUTE), asker.ask(5 * SECOND, Ok(1))));
    sleep(SECOND).await;
    first.abort();
    sleep(10 * SECOND).await;
    let fact = get(key(9), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(2))).await;
    assert_eq!((*fact.value.unwrap(), fact.came), (1, Came::Hit));
}

#[tokio::test(start_paused = true)]
async fn a_live_query_notes_what_it_read() {
    let asker = Asker::default();
    get(key(10), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(1))).await;
    sleep(10 * SECOND).await;
    let read = Arc::new(Mutex::<Reading>::default());
    let now = Instant::now();
    reading(read.clone(), async {
        // from the store, 10 s old: outdated in 50 s
        get(key(10), Freshness::MaxAge(MINUTE), asker.ask(SECOND, Ok(2))).await;
        // asked now, taken at 30 s: outdated in 30 s, the sooner
        get(key(11), Freshness::MaxAge(30 * SECOND), asker.ask(SECOND, Ok(3))).await;
        // a moment: no running again for it
        get(key(12), Freshness::MaxAge(Duration::ZERO), asker.ask(SECOND, Ok(4))).await;
    })
    .await;
    let read = read.lock().unwrap();
    assert_eq!(read.keys.len(), 3);
    assert_eq!(read.outdated, Some(now + 30 * SECOND));
    assert!(read.concerns(&Change::Job(DeviceId(11))));
    assert!(read.concerns(&Change::Invalidated(DeviceId(10), &[FactKind::Config])));
    assert!(!read.concerns(&Change::Invalidated(DeviceId(10), &[FactKind::Login])));
    assert!(!read.concerns(&Change::Job(DeviceId(13))));
}
