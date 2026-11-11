//! Scope: RouterOS commands no mikrotik-model type covers, sent as they are; not commands a model
//! covers.
//!
//! Commands sent as they are over the API, where no model of mikrotik-model fits: actions
//! (`/system/package/uninstall`, `/interface/wifi/radio/provision`), monitors and the few
//! menus read only for one field. A refusal is [`Error::Refused`] naming the command.
use crate::Error;
use async_graphql::futures_util::StreamExt;
use async_graphql::futures_util::future::join_all;
use mikrotik_api::{
    prelude::{MikrotikDevice as RawDevice, TrapCategory},
    simple::SimpleResult,
};
use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    time::Duration,
};

/// One answer of a command, by field name.
pub(crate) type Row = HashMap<Box<str>, Option<Box<str>>>;

/// Runs `path` with `attributes`; the rows it answers.
pub(crate) async fn command(
    device: &RawDevice<SimpleResult>,
    path: &str,
    attributes: &[(&str, &str)],
) -> Result<Vec<Row>, Error> {
    answer(device, path, attributes, Missing::Refused).await
}

/// [`command`], with no rows where the device hasn't the menu (its package missing,
/// [`is_missing_menu`]).
pub(crate) async fn command_if_present(
    device: &RawDevice<SimpleResult>,
    path: &str,
    attributes: &[(&str, &str)],
) -> Result<Vec<Row>, Error> {
    answer(device, path, attributes, Missing::Empty).await
}

/// What a menu the device hasn't answers.
#[derive(Clone, Copy, PartialEq)]
enum Missing {
    Refused,
    Empty,
}

/// RouterOS's answer to a menu it hasn't: `no such command prefix` without a category (the
/// package missing), or a missing item or command.
pub(crate) fn is_missing_menu(category: Option<TrapCategory>, message: &[u8]) -> bool {
    match category {
        None => message == b"no such command prefix",
        Some(category) => category == TrapCategory::MissingItemOrCommand,
    }
}

async fn answer(
    device: &RawDevice<SimpleResult>,
    path: &str,
    attributes: &[(&str, &str)],
    missing: Missing,
) -> Result<Vec<Row>, Error> {
    let mut stream = device
        .send_command(
            path.as_bytes(),
            |cmd| {
                attributes.iter().fold(cmd, |cmd, (key, value)| {
                    cmd.attribute(key.as_bytes(), value.as_bytes())
                })
            },
            (),
        )
        .await;
    let mut rows = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(row) => rows.push(row),
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { category, message } => {
                if missing == Missing::Empty && is_missing_menu(category, message.as_bytes()) {
                    // the other trap of the pair follows; the menu has no rows
                    continue;
                }
                return Err(Error::Refused(format!("{path}: {message}").into()));
            }
        }
    }
    Ok(rows)
}

/// The rows of `path` without attributes (a `print`).
pub(crate) async fn rows(device: &RawDevice<SimpleResult>, path: &str) -> Result<Vec<Row>, Error> {
    command(device, path, &[]).await
}

pub(crate) fn field<'r>(row: &'r Row, name: &str) -> Option<&'r str> {
    row.get(name).and_then(|v| v.as_deref())
}

/// The ones of `addresses` that answer a ping from the device, all asked at once.
pub(crate) async fn answering(
    raw: &RawDevice<SimpleResult>,
    addresses: &[IpAddr],
) -> BTreeSet<IpAddr> {
    let pings = addresses.iter().map(|address| async move {
        let text = address.to_string();
        let attributes = [
            ("address", text.as_str()),
            ("count", "3"),
            ("interval", "0.3"),
        ];
        let ping = command(raw, "/ping", &attributes);
        let answered = tokio::time::timeout(Duration::from_secs(10), ping)
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| field(row, "received").is_some_and(|received| received != "0"))
            });
        answered.then_some(*address)
    });
    join_all(pings).await.into_iter().flatten().collect()
}
