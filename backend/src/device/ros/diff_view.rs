//! Scope: What a diff changes, entry by entry with the values before and after, and the script
//! back; not the script there (`cfg`) nor its counts by table (`site_diff`).
//!
//! Both come from the sorted mutations of the script and what each says was there before
//! (`ResourceMutation::before`, mikrotik-model). The script back is what the operator would
//! type to undo the apply by hand; nothing runs it (the fallback loads the backup instead).
//! It can't put a moved entry back in its place, and says so in a comment.
use super::cfg::in_script;
use crate::device::site_diff::TableChanges;
use async_graphql::{Enum, SimpleObject};
use mikrotik_model::{
    generator::Generator,
    resource::{ResourceMutation, ResourceMutationOperation},
    value::KeyValuePair,
};
use std::borrow::Cow;

/// A diff in each of its views: by table, entry by entry, the script and the script back.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct CfgDiff {
    /// what the script changes, by table (`site_diff::table_changes`), with what an apply
    /// changes outside it (the password, WebFig's certificate)
    pub tables: Box<[TableChanges]>,
    /// what it changes, entry by entry with the values before and after
    pub entries: Box<[EntryChange]>,
    /// empty if the device is in line already
    pub script: Box<str>,
    /// the script undoing it by hand: nothing runs it, the fallback loads the backup
    pub revert_script: Box<str>,
}

/// What a change does to its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum EntryChangeKind {
    Add,
    Set,
    Remove,
    /// the entries are put in another order (firewall rules)
    Move,
}

/// One field of a changed entry; no value is a field without one (unset, or not there).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DiffField {
    pub field: Box<str>,
    pub before: Option<Box<str>>,
    pub after: Option<Box<str>>,
}

/// One entry an apply changes.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct EntryChange {
    /// `interface/bridge/port`
    pub table: Box<str>,
    pub kind: EntryChangeKind,
    /// how the script finds it (`name=ether1`), the moved entries for a move; none for a table
    /// of one entry or an entry the add makes
    pub entry: Option<Box<str>>,
    pub fields: Box<[DiffField]>,
}

/// RouterOS' bytes as text (Latin-1, as the script writes them).
fn text(value: &[u8]) -> Box<str> {
    value.iter().map(|&byte| char::from(byte)).collect()
}

/// A field's name without the `!` that unsets it.
fn bare(key: &[u8]) -> &[u8] {
    key.strip_prefix(b"!").unwrap_or(key)
}

/// Its value, none where it is unset.
fn value_of(field: &KeyValuePair) -> Option<Box<str>> {
    (!field.key.starts_with(b"!") && !field.value.is_empty()).then(|| text(&field.value))
}

fn key_text(key: &KeyValuePair) -> Box<str> {
    format!("{}={}", text(key.key), text(&key.value)).into()
}

fn set_fields(fields: &[KeyValuePair], before: &[KeyValuePair]) -> Box<[DiffField]> {
    fields
        .iter()
        .map(|field| DiffField {
            field: text(bare(field.key)),
            before: before
                .iter()
                .find(|old| bare(old.key) == bare(field.key))
                .and_then(value_of),
            after: value_of(field),
        })
        .collect()
}

fn added_fields<'a>(fields: impl Iterator<Item = &'a KeyValuePair<'a>>) -> Box<[DiffField]> {
    fields
        .map(|field| DiffField {
            field: text(bare(field.key)),
            before: None,
            after: value_of(field),
        })
        .collect()
}

/// The entries the script changes, in its order.
pub fn entry_changes(mutations: &[ResourceMutation]) -> Box<[EntryChange]> {
    mutations
        .iter()
        .filter(|mutation| in_script(mutation))
        .map(|mutation| {
            let (kind, entry, fields) = match &mutation.operation {
                ResourceMutationOperation::Add => (
                    EntryChangeKind::Add,
                    None,
                    added_fields(mutation.fields.iter()),
                ),
                ResourceMutationOperation::AddThenSet { key, fields } => (
                    EntryChangeKind::Add,
                    Some(key_text(key)),
                    added_fields(mutation.fields.iter().chain(fields.iter())),
                ),
                ResourceMutationOperation::RemoveByKey(key) => (
                    EntryChangeKind::Remove,
                    Some(key_text(key)),
                    mutation
                        .before
                        .iter()
                        .map(|field| DiffField {
                            field: text(bare(field.key)),
                            before: value_of(field),
                            after: None,
                        })
                        .collect(),
                ),
                ResourceMutationOperation::UpdateSingle => (
                    EntryChangeKind::Set,
                    None,
                    set_fields(&mutation.fields, &mutation.before),
                ),
                ResourceMutationOperation::UpdateByKey(key) => (
                    EntryChangeKind::Set,
                    Some(key_text(key)),
                    set_fields(&mutation.fields, &mutation.before),
                ),
                ResourceMutationOperation::Move { entries, .. } => (
                    EntryChangeKind::Move,
                    Some(
                        entries
                            .iter()
                            .map(key_text)
                            .collect::<Vec<_>>()
                            .join(", ")
                            .into(),
                    ),
                    Box::default(),
                ),
            };
            EntryChange {
                table: text(mutation.resource),
                kind,
                entry,
                fields,
            }
        })
        .collect()
}

/// A mutation of the way back.
fn back<'a>(
    resource: &'static [u8],
    operation: ResourceMutationOperation<'a>,
    fields: &[KeyValuePair<'a>],
) -> ResourceMutation<'a> {
    ResourceMutation {
        resource,
        operation,
        fields: fields.into(),
        before: Box::new([]),
        depends: Box::new([]),
        provides: Box::new([]),
        ordering: Default::default(),
    }
}

/// The script undoing `mutations` (sorted, as the script runs them): each the other way, the
/// last first. An entry renamed by its update is found by its new name.
pub fn revert_script(mutations: &[ResourceMutation]) -> Result<Box<str>, std::fmt::Error> {
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in mutations
        .iter()
        .rev()
        .filter(|mutation| in_script(mutation))
    {
        let back = |operation, fields| back(mutation.resource, operation, fields);
        match &mutation.operation {
            ResourceMutationOperation::Add => {
                generator.append_remove_where(mutation.resource, &mutation.fields)?
            }
            ResourceMutationOperation::AddThenSet { key, .. } => generator.append_mutation(
                &back(ResourceMutationOperation::RemoveByKey(key.clone()), &[]),
            )?,
            ResourceMutationOperation::RemoveByKey(_) if mutation.before.is_empty() => generator
                .append_comment(mutation.resource, "removed entry not known, not added back")?,
            ResourceMutationOperation::RemoveByKey(_) => generator
                .append_mutation(&back(ResourceMutationOperation::Add, &mutation.before))?,
            ResourceMutationOperation::UpdateSingle => generator.append_mutation(&back(
                ResourceMutationOperation::UpdateSingle,
                &mutation.before,
            ))?,
            ResourceMutationOperation::UpdateByKey(key) => {
                let found = mutation
                    .fields
                    .iter()
                    .find(|field| field.key == key.key)
                    .map_or_else(
                        || key.clone(),
                        |renamed| KeyValuePair {
                            key: key.key,
                            value: Cow::Borrowed(renamed.value.as_ref()),
                        },
                    );
                generator.append_mutation(&back(
                    ResourceMutationOperation::UpdateByKey(found),
                    &mutation.before,
                ))?
            }
            ResourceMutationOperation::Move { .. } => {
                generator.append_comment(mutation.resource, "order not restored")?
            }
        }
    }
    Ok(script.into())
}

#[cfg(test)]
mod test;
