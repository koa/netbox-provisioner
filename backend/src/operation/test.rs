//! Scope: Tests of `operation`; nothing else.
use super::{counted, note};
use crate::{
    facts::{self, FactKey, FactKind, Freshness},
    topology::DeviceId,
};
use std::time::Duration;

/// A fact is asked in a task of its own (`facts::get`): what it sends counts for the
/// operation that asked, twice the same path twice.
#[tokio::test]
async fn a_fact_asked_counts_for_the_operation() {
    let ((), sent) = counted(async {
        let key = FactKey::new(FactKind::Table, Some(DeviceId(7)), "");
        let fact = facts::get(key, Freshness::MaxAge(Duration::ZERO), async {
            note(DeviceId(7), b"/system/resource/print");
            note(DeviceId(7), b"/system/resource/print");
            note(DeviceId(7), b"/system/health/print");
            Ok(1u32)
        })
        .await;
        assert!(fact.value.is_ok());
    })
    .await;
    assert_eq!(
        sent,
        [
            (DeviceId(7), "/system/health/print".into(), 1),
            (DeviceId(7), "/system/resource/print".into(), 2),
        ]
    );
}

/// Within an operation the topology is loaded once and the same for every ask; outside
/// (a job) each ask loads.
#[tokio::test]
async fn an_operation_keeps_the_topology_it_got_first() {
    use super::topology;
    use crate::topology::builder::TopologyBuilder;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };
    let loads = AtomicU32::new(0);
    let load = || async {
        loads.fetch_add(1, Ordering::SeqCst);
        Some(Arc::new(TopologyBuilder::default().build()))
    };
    let ((first, second, other), _) = counted(async {
        let first = topology(1, load()).await;
        let second = topology(1, load()).await;
        // another holder's is loaded, not kept
        let other = topology(2, load()).await;
        (first, second, other)
    })
    .await;
    assert!(Arc::ptr_eq(
        first.as_ref().unwrap(),
        second.as_ref().unwrap()
    ));
    assert!(other.is_some());
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    topology(1, load()).await;
    topology(1, load()).await;
    assert_eq!(loads.load(Ordering::SeqCst), 4);
}

/// NetBox is asked once per URL within an operation, again after a failure; outside one
/// each time.
#[tokio::test]
async fn an_operation_reads_each_url_once() {
    use super::netbox;
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicU32, Ordering};
    let counter = AtomicU32::new(0);
    let loads = &counter;
    let load = |ok: bool| async move {
        loads.fetch_add(1, Ordering::SeqCst);
        match ok {
            true => Ok(json!({ "id": 1 })),
            false => Err(()),
        }
    };
    let (answers, _) = counted(async {
        let failed = netbox("dcim/devices/1/", load(false)).await;
        let first = netbox("dcim/devices/1/", load(true)).await;
        let second = netbox("dcim/devices/1/", load(true)).await;
        let other = netbox("dcim/devices/2/", load(true)).await;
        [failed, first, second, other]
    })
    .await;
    let ok = Ok::<Value, ()>(json!({ "id": 1 }));
    assert_eq!(answers, [Err(()), ok.clone(), ok.clone(), ok]);
    assert_eq!(loads.load(Ordering::SeqCst), 3);
    netbox("dcim/devices/1/", load(true)).await.unwrap();
    assert_eq!(loads.load(Ordering::SeqCst), 4);
}

/// Outside any operation nothing is noted (a job).
#[tokio::test]
async fn outside_nothing_counts() {
    note(DeviceId(7), b"/system/resource/print");
    let ((), sent) = counted(async {}).await;
    assert!(sent.is_empty());
}
