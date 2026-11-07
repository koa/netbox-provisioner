pub const NOT_LOADED: &str = "Nicht geladen";
pub const OVERVIEW: &str = "Übersicht";
pub const ALL_DEVICES: &str = "Alle Geräte";
/// the network map of every site, an entry of the sites' dropdown and its title there
pub const ALL_SITES_MAP: &str = "Netz-Karte (alle Sites)";
/// the rollout over several sites, an entry of the sites' dropdown and its title there
pub const ROLLOUT: &str = "Netz aktualisieren";
/// the site's network map, an entry of the devices' dropdown and its title there
pub const SITE_MAP: &str = "Netz-Karte";

/// `name`: the site shown, `None` with `known` an id NetBox doesn't have, none without
pub fn site(name: Option<&str>, known: bool) -> String {
    format!(
        "Site: {}",
        name.unwrap_or(if known { "unbekannt" } else { "-" })
    )
}

pub fn device(name: Option<&str>, known: bool) -> String {
    format!(
        "Gerät: {}",
        name.unwrap_or(if known { "unbekannt" } else { "-" })
    )
}
