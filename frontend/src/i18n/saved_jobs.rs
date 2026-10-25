pub fn title(device: &str) -> String {
    format!("Gespeicherter Auftrag für {device} nicht weitergeführt")
}
pub fn title_unreadable(device_id: i64) -> String {
    format!("Gespeicherter Auftrag für Gerät {device_id} nicht lesbar")
}
pub const APPLY: &str = "Anwenden";
pub const UPDATE: &str = "Update";
pub fn saved(kind: &str, state: &str, seconds_ago: u64) -> String {
    let ago = match seconds_ago {
        0..120 => format!("{seconds_ago} s"),
        120..7200 => format!("{} min", seconds_ago / 60),
        _ => format!("{} h", seconds_ago / 3600),
    };
    format!("{kind} im Zustand „{state}“, gespeichert vor {ago}.")
}
pub const DEVICE_GONE: &str = "Das Gerät ist nicht mehr in NetBox.";
pub const BATCH_INCOMPLETE: &str = "Gemeinsames Anwenden, von dem nicht alle Geräte gespeichert oder in NetBox sind: \
    nicht weitergeführt, die Fallbacks nehmen es auf allen zurück.";
pub fn unreadable(why: &str) -> String {
    format!("Die Datei liess sich nicht lesen: {why}")
}
pub const LEFT_ON_DEVICE: &str =
    "Was der Auftrag auf dem Gerät hinterliess (Fallback, Watchdogs), bleibt dort; „Liegengebliebenes“ des Geräts zeigt es.";
pub const DISCARD: &str = "Verwerfen";
