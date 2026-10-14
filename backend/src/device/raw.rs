//! Commands sent as they are over the API, where no model of mikrotik-model fits: actions
//! (`/system/package/uninstall`, `/interface/wifi/radio/provision`), monitors and the few
//! menus read only for one field. A refusal is [`Error::Refused`] naming the command.
use crate::Error;
use async_graphql::futures_util::StreamExt;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::collections::HashMap;

/// One answer of a command, by field name.
pub(crate) type Row = HashMap<Box<str>, Option<Box<str>>>;

/// Runs `path` with `attributes`; the rows it answers.
pub(crate) async fn command(
    device: &RawDevice<SimpleResult>,
    path: &str,
    attributes: &[(&str, &str)],
) -> Result<Vec<Row>, Error> {
    let mut stream = device
        .send_command(
            path.as_bytes(),
            |cmd| {
                attributes
                    .iter()
                    .fold(cmd, |cmd, (key, value)| cmd.attribute(key.as_bytes(), value.as_bytes()))
            },
            (),
        )
        .await;
    let mut rows = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(row) => rows.push(row),
            SimpleResult::Error(error) => return Err(error.into()),
            SimpleResult::Trap { message, .. } => {
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
