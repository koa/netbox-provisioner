pub const NOT_FOUND: &str = "Seite nicht gefunden";

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
