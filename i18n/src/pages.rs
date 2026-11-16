//! Scope: The texts of the sites and devices pages; not those of a card with a module of its own.
pub const NOT_FOUND: &str = "Seite nicht gefunden";
pub const WWW_CA: &str = "Root-CA von WebFig über HTTPS herunterladen";
pub const WWW_CA_HINT: &str = "Einmal im Browser als Zertifizierungsstelle importieren; die \
     Geräte liefern ihr Zertifikat und die Zwischenzertifikate.";

pub fn device_count(count: usize) -> String {
    match count {
        1 => "1 Gerät".into(),
        n => format!("{n} Geräte"),
    }
}

/// a ping's round trip
pub fn ping(nanos: u64) -> String {
    format!("{:.2} ms", nanos as f32 / 1_000_000.0)
}
