pub const TITLE: &str = "Abweichung von NetBox";
pub const INTRO: &str = "Rechnet für jedes Gerät, was ein Anwenden ändern würde, wie der Diff auf der Geräteseite. Läuft im Hintergrund weiter, auch wenn die Seite zu ist; auf den Geräten ändert sich nichts.";
pub const START: &str = "Alle Geräte prüfen";
pub const AGAIN: &str = "Nochmals prüfen";
pub const IN_LINE: &str = "entspricht NetBox";
pub const LOST: &str = "Der Provisioner wurde neu gestartet, die Prüfung ist verloren.";
pub const CANCELLED: &str = "abgebrochen";

/// how many of the devices are read
pub fn progress(read: usize, all: usize) -> String {
    format!("{read} von {all} Geräten gelesen")
}
/// how many devices are as NetBox has them
pub fn in_line_count(in_line: usize, all: usize) -> String {
    format!("{in_line} von {all} entsprechen NetBox")
}
pub fn changes(count: u32) -> String {
    match count {
        1 => "1 Änderung".into(),
        count => format!("{count} Änderungen"),
    }
}
/// the changes of one menu: added, changed, removed
pub fn menu(menu: &str, added: i64, changed: i64, removed: i64) -> String {
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
    format!("/{menu} {}", parts.join(" "))
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
pub fn unreadable(error: &str) -> String {
    format!("nicht gelesen: {error}")
}
