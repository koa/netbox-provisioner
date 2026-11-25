//! Scope: The words of a diff's tables and entries (`TableChanges`, `EntryChangeKind`); nothing
//! else.
use crate::graphql::authenticated::types::{EntryChangeKind, TableChanges};
use ::i18n::diff_view as t;

/// Each table's count.
pub fn table_list(tables: &[TableChanges]) -> String {
    tables
        .iter()
        .map(|m| t::table(&m.table, m.added.into(), m.changed.into(), m.removed.into()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What happens to an entry.
pub fn kind(kind: &EntryChangeKind) -> String {
    t::kind(match kind {
        EntryChangeKind::Add => t::Kind::Add,
        EntryChangeKind::Set => t::Kind::Set,
        EntryChangeKind::Remove => t::Kind::Remove,
        EntryChangeKind::Move => t::Kind::Move,
        EntryChangeKind::Other(other) => t::Kind::Other(other),
    })
}
