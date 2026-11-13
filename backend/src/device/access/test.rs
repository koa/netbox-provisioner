//! The facades over a fake device: what a [`Reader`] with an age keeps, and the refusals.
use super::{Reader, Scope, generation, is_missing_menu};
use crate::{facts::Freshness, topology::DeviceId};
use mikrotik_api::prelude::{Connection, TrapCategory};
use mikrotik_model::{model::SystemIdentityCfg, resource::SingleResource};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

/// One API word: its length (below 0x80 a single byte), then the word.
fn word(text: &str) -> Vec<u8> {
    let mut bytes = vec![text.len() as u8];
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// The next sentence the provisioner sent, as words; none once it closed.
async fn sentence(device: &mut DuplexStream) -> Option<Vec<String>> {
    let mut words = Vec::new();
    loop {
        let length = device.read_u8().await.ok()? as usize;
        if length == 0 {
            return Some(words);
        }
        let mut word = vec![0; length];
        device.read_exact(&mut word).await.ok()?;
        words.push(String::from_utf8_lossy(&word).into_owned());
    }
}

/// A device answering every command with its identity `lab`, counting the commands after the
/// login.
async fn fake_device() -> (Connection, Arc<AtomicUsize>, DeviceId) {
    static NEXT: AtomicUsize = AtomicUsize::new(9000);
    let device = DeviceId(NEXT.fetch_add(1, Ordering::SeqCst) as u32);
    let (client, mut fake) = tokio::io::duplex(4096);
    let commands = Arc::new(AtomicUsize::new(0));
    let counted = commands.clone();
    tokio::spawn(async move {
        let mut first = true;
        while let Some(words) = sentence(&mut fake).await {
            let tag = words
                .iter()
                .find_map(|word| word.strip_prefix(".tag="))
                .unwrap_or("0")
                .to_owned();
            let mut answer = Vec::new();
            if !first {
                counted.fetch_add(1, Ordering::SeqCst);
                answer = [
                    word("!re"),
                    word(&format!(".tag={tag}")),
                    word("=name=lab"),
                    vec![0],
                ]
                .concat();
            }
            first = false;
            answer.extend([word("!done"), word(&format!(".tag={tag}")), vec![0]].concat());
            fake.write_all(&answer).await.unwrap();
        }
    });
    let connection =
        Connection::connect_with_stream(client, b"admin", Some(b""), generation(device))
            .await
            .expect("login");
    (connection, commands, device)
}

fn scope(credentials: &str) -> Scope {
    Scope {
        credentials: credentials.into(),
        way: "routed",
    }
}

async fn identity(reader: &Reader) -> Box<str> {
    <SystemIdentityCfg as SingleResource>::fetch(reader)
        .await
        .expect("read")
        .expect("an identity")
        .name
        .to_string()
        .into()
}

/// Read again within its age, a menu comes from the facts; a reader without an age asks the
/// device each time.
#[tokio::test]
async fn a_menu_read_within_its_age_is_asked_once() {
    let (connection, commands, device) = fake_device().await;
    let live = Reader::new(connection, Some(device), scope("credentials lab"));
    let kept = live.at(Freshness::MaxAge(Duration::from_secs(60)));
    assert_eq!(&*identity(&kept).await, "lab");
    assert_eq!(&*identity(&kept.clone()).await, "lab");
    assert_eq!(commands.load(Ordering::SeqCst), 1);
    assert_eq!(&*identity(&live).await, "lab");
    assert_eq!(commands.load(Ordering::SeqCst), 2, "without an age: asked");
}

/// A write to the device on any connection since makes what was read stale.
#[tokio::test]
async fn a_write_since_asks_again() {
    let (connection, commands, device) = fake_device().await;
    let kept = Reader::new(connection, Some(device), scope("credentials lab"))
        .at(Freshness::MaxAge(Duration::from_secs(60)));
    identity(&kept).await;
    generation(device).advance();
    identity(&kept).await;
    identity(&kept).await;
    assert_eq!(commands.load(Ordering::SeqCst), 2);
}

/// Other credentials may see other things: what they read is a fact of its own.
#[tokio::test]
async fn other_credentials_read_for_themselves() {
    let (connection, commands, device) = fake_device().await;
    let age = Freshness::MaxAge(Duration::from_secs(60));
    let lab = Reader::new(connection.clone(), Some(device), scope("credentials lab")).at(age);
    let typed = Reader::new(connection, Some(device), scope("typed in 0123")).at(age);
    identity(&lab).await;
    identity(&typed).await;
    identity(&lab).await;
    assert_eq!(commands.load(Ordering::SeqCst), 2);
}

/// A missing package's menu has no rows; another refusal stays an error.
#[test]
fn a_missing_menu_is_told_apart() {
    assert!(is_missing_menu(None, b"no such command prefix"));
    assert!(is_missing_menu(
        Some(TrapCategory::MissingItemOrCommand),
        b""
    ));
    assert!(!is_missing_menu(None, b"not enough permissions (9)"));
}
