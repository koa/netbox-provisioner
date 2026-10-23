pub const NOT_LOADED: &str = "Nicht geladen";
pub const OVERVIEW: &str = "Übersicht";
pub const ALL_DEVICES: &str = "Alle Geräte";

/// `name`: the site shown, `None` with `known` an id NetBox doesn't have, none without
pub fn site(name: Option<&str>, known: bool) -> String {
    format!("Site: {}", name.unwrap_or(if known { "unbekannt" } else { "-" }))
}

pub fn device(name: Option<&str>, known: bool) -> String {
    format!("Gerät: {}", name.unwrap_or(if known { "unbekannt" } else { "-" }))
}
