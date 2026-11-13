//! Scope: The texts of the network map; nothing else.
pub const TITLE: &str = "Netz-Karte";
pub const INTRO: &str = "Die Geräte der Site und ihre Kabel und Funkstrecken aus NetBox, ihr Zustand aus der OSPF-Datenbank eines Routers; Geräte ohne OSPF werden nicht dargestellt. Liest nur, alle paar Sekunden neu.";
pub const INTRO_ALL: &str = "Alle Geräte des OSPF-Netzes über alle Sites, ihre Kabel und Funkstrecken aus NetBox, ihr Zustand aus der OSPF-Datenbank eines Routers; Geräte ohne OSPF werden nicht dargestellt. Liest nur, alle paar Sekunden neu.";
pub const NO_ROUTER: &str =
    "Die Site hat keinen OSPF-Router, von dem sich die OSPF-Datenbank lesen liesse.";
pub const NO_ROUTER_ALL: &str =
    "Kein Gerät hat einen OSPF-Router, von dem sich die OSPF-Datenbank lesen liesse.";
pub fn read_from(device: &str, age: u32) -> String {
    format!("Gelesen von {device}, vor {age} s")
}
pub fn unreadable(error: &str) -> String {
    format!("Kein Router antwortet: {error}")
}
pub fn outside(count: u32) -> String {
    match count {
        1 => "ausserhalb (1 Router)".into(),
        _ => format!("ausserhalb ({count} Router)"),
    }
}
pub const NODE_REACHED: &str = "in OSPF";
pub const NODE_ISOLATED: &str =
    "abgeschnitten: noch in der OSPF-Datenbank, aber keine Strecke oben";
pub const NODE_MISSING: &str = "fehlt in OSPF";
pub const NODE_NOT_OSPF: &str = "kein OSPF-Router: trägt die Strecke zwischen zwei Nachbarn";
pub const EDGE_UP: &str = "oben (OSPFv2 und v3)";
pub const EDGE_V3_DOWN: &str = "OSPFv2 oben, OSPFv3 nicht (IPv6 aus?)";
pub const EDGE_DOWN: &str = "unten";
pub const EDGE_NOT_OSPF: &str = "ohne OSPF: OSPF sagt nichts darüber";
pub const RADIO: &str = "Funkstrecke";
pub fn router_missing(device: &str) -> String {
    format!("{device} fehlt in der OSPF-Datenbank")
}
pub const NO_SEGMENT: &str = "kein Nachbar im Netz des Ports";
pub fn not_on_segment(device: &str) -> String {
    format!("{device} ist nicht im Netz des Ports")
}
pub const LEGEND: &str = "Legende";
/// Not judged, only shown (`docs/concepts/network-map.md`, step 3): memory and disk as MiB,
/// for readability, nothing more.
pub fn resources(
    uptime_seconds: u64,
    cpu_load: i32,
    free_memory: u64,
    total_memory: u64,
    free_hdd_space: u64,
    version: &str,
) -> String {
    let mib = |bytes: u64| bytes / (1024 * 1024);
    let minutes = uptime_seconds / 60;
    let hours = minutes / 60;
    let uptime = match hours {
        0 => format!("{minutes} min"),
        1..24 => format!("{hours} h {} min", minutes % 60),
        _ => format!("{} T {} h", hours / 24, hours % 24),
    };
    format!(
        "{version}, seit {uptime}, CPU {cpu_load} %, RAM {} von {} MiB frei, Disk {} MiB frei",
        mib(free_memory),
        mib(total_memory),
        mib(free_hdd_space)
    )
}
pub fn health_row(name: &str, value: &str, unit: Option<&str>) -> String {
    match unit {
        Some(unit) => format!("{name}: {value} {unit}"),
        None => format!("{name}: {value}"),
    }
}
pub const LOADING: &str = "Lädt …";
pub const EDGE_POE: &str = "PoE";
pub const EDGE_POE_PROBLEM: &str = "PoE: Soll und Ist stimmen nicht";
pub const POE_NO_POE_OUT: &str = "speist laut NetBox, der Port hat kein PoE-Out";
pub const POE_NO_LOAD: &str = "speist laut NetBox, am anderen Ende bezieht nichts Strom";
pub const POE_PD_MISSING: &str = "speist, die Gegenseite ist in NetBox nicht als gespeist erfasst";
pub const POE_FEEDS_INSTEAD: &str = "in NetBox gespeist, speist aber selbst";
pub const POE_UNDOCUMENTED: &str = "speist, ohne PoE-Modus in NetBox";
/// Not judged beyond `problem` (`docs/concepts/network-map.md`, step 4): `status` as
/// `poe-out-status`, `power` as `poe-out-power` in watts.
pub fn poe_edge(
    pse: &str,
    pd: &str,
    status: Option<&str>,
    power: Option<f64>,
    problem: Option<&str>,
) -> String {
    let power = power.map(|w| format!(", {w} W")).unwrap_or_default();
    let status = status.map(|s| format!(" ({s})")).unwrap_or_default();
    let problem = problem.map(|p| format!("\n{p}")).unwrap_or_default();
    format!("{pse} → {pd}: PoE{power}{status}{problem}")
}
pub const DRAG_HINT: &str = "Knoten lassen sich verschieben; die eigene Anordnung bleibt in diesem Browser, bis sie veröffentlicht oder zurückgesetzt wird.";
pub const RESET: &str = "Eigene Anordnung zurücksetzen";
pub const PUBLISH: &str = "Anordnung veröffentlichen";
pub const PUBLISHING: &str = "Veröffentlicht …";
pub fn published(count: u32) -> String {
    format!("{count} Position(en) veröffentlicht.")
}
pub fn publish_failed(error: &str) -> String {
    format!("Veröffentlichen fehlgeschlagen: {error}")
}
pub const EXPORT_SVG: &str = "Als SVG exportieren";
pub const EXPORT_PNG: &str = "Als PNG exportieren";
