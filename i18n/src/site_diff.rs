//! Scope: The texts of how far each device of a site is from NetBox; nothing else.
pub const TITLE: &str = "Abweichung von NetBox";
pub const INTRO: &str = "Rechnet für jedes Gerät, was ein Anwenden ändern würde, wie der Diff auf der Geräteseite. Läuft im Hintergrund weiter, auch wenn die Seite zu ist; auf den Geräten ändert sich nichts.";
pub const START: &str = "Alle Geräte prüfen";
pub const AGAIN: &str = "Nochmals prüfen";
pub const IN_LINE: &str = "entspricht NetBox";
pub const LOST: &str = "Der Provisioner wurde neu gestartet, die Prüfung ist verloren.";
pub const CANCELLED: &str = "abgebrochen";
/// opens a device's fields, script and script back
pub const DETAILS: &str = "Details";
pub const CLOSE: &str = "Schliessen";
/// the title of a device's details
pub fn details_title(device: &str) -> String {
    format!("Abweichung von {device}")
}
/// a device whose config or NetBox changed after it was checked, not read since
pub const CHANGED_SINCE: &str =
    "seit der Prüfung verändert (Anwenden oder NetBox), noch nicht neu gelesen";

/// how many of the devices are read
pub fn progress(read: usize, all: usize) -> String {
    format!("{read} von {all} Geräten gelesen")
}
/// how many devices are as NetBox has them
pub fn in_line_count(in_line: usize, all: usize) -> String {
    format!("{in_line} von {all} entsprechen NetBox")
}
pub fn unreadable(error: &str) -> String {
    format!("nicht gelesen: {error}")
}
