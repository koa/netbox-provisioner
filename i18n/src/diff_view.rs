//! Scope: The texts of the views of a diff (`components::diff_view`), its summary by table
//! too; nothing else.

pub const SUMMARY: &str = "Übersicht";
pub const SCRIPT: &str = "Skript";
pub const REVERT: &str = "Zurück";
pub const FIELDS: &str = "Felder";
pub const REVERT_HINT: &str = "So ginge es von Hand zurück; ausgeführt wird es nie, nach einem \
    gescheiterten Anwenden lädt der Fallback die Sicherung. Eine geänderte Reihenfolge stellt \
    es nicht wieder her.";
pub const FIELD: &str = "Feld";
pub const BEFORE: &str = "Vorher";
pub const AFTER: &str = "Nachher";
/// a field without a value
pub const NONE: &str = "–";

/// What a change does to its entry (`EntryChangeKind`).
pub enum Kind<'a> {
    Add,
    Set,
    Remove,
    Move,
    Other(&'a str),
}

pub fn kind(kind: Kind) -> String {
    match kind {
        Kind::Add => "neu".into(),
        Kind::Set => "geändert".into(),
        Kind::Remove => "entfernt".into(),
        Kind::Move => "umgestellt".into(),
        Kind::Other(other) => other.into(),
    }
}

pub fn changes(count: u32) -> String {
    match count {
        1 => "1 Änderung".into(),
        count => format!("{count} Änderungen"),
    }
}
/// the changes of one table: added, changed, removed
pub fn table(table: &str, added: i64, changed: i64, removed: i64) -> String {
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("+{added}"));
    }
    if changed > 0 {
        parts.push(format!("~{changed}"));
    }
    if removed > 0 {
        parts.push(format!("−{removed}"));
    }
    format!("/{table} {}", parts.join(" "))
}
/// the parts that need a closer look
pub fn removes_addresses(count: i64) -> String {
    match count {
        1 => "entfernt 1 Adresse".into(),
        count => format!("entfernt {count} Adressen"),
    }
}
pub fn removes_leases(count: i64) -> String {
    match count {
        1 => "entfernt 1 statischen Lease".into(),
        count => format!("entfernt {count} statische Leases"),
    }
}
pub const TOUCHES_OSPF: &str = "ändert OSPF";
pub const TOUCHES_BRIDGE_PORTS: &str = "ändert Bridge-Ports";
