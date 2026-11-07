pub const TITLE: &str = "Netz aktualisieren";
pub const INTRO: &str = "Bringt die RouterOS-Geräte der gewählten Sites auf das neueste Release \
    eines Kanals, mit einem normalen Update pro Gerät: zuerst je ein Pilot pro Architektur, dann \
    von aussen nach innen. Ein Gerät startet erst neu, wenn keines, das nur über es erreichbar \
    ist, gerade ein Update macht. Der Plan liest die Geräte nur.";
pub const SITES: &str = "Sites";
pub const CHANNEL: &str = "Kanal";
/// the channel of the config, when none is chosen
pub const DEFAULT_CHANNEL: &str = "Standard";
pub const PLAN: &str = "Plan erstellen";
pub const PLAN_AGAIN: &str = "Neu planen";
pub const NO_SITE: &str = "Mindestens eine Site wählen";

pub const DEVICE: &str = "Gerät";
pub const SITE: &str = "Site";
pub const VERSION: &str = "Version";
pub const ARCHITECTURE: &str = "Architektur";
pub const PACKAGES: &str = "Pakete";
pub const FALLBACK: &str = "Fallback";
pub const POSITION: &str = "Reihenfolge";

pub const FALLBACK_PARTITION: &str = "Partition";
pub const FALLBACK_NEIGHBOR: &str = "über Nachbarn";
pub const FALLBACK_NONE: &str = "keiner";

pub const SKIP_UP_TO_DATE: &str = "aktuell";
pub const SKIP_UNREACHABLE: &str = "nicht erreichbar";
pub const SKIP_LEFTOVERS: &str = "ausgelassen: Reste eines Jobs, auf der Geräteseite klären";
pub const SKIP_NO_FALLBACK: &str =
    "ausgelassen: kein Weg zurück, nur auf der Geräteseite zu aktualisieren";
pub const SKIP_NO_SPACE: &str = "ausgelassen: zu wenig freier Speicher für die Pakete";

pub fn target(channel: &str, version: &str) -> String {
    format!("Ziel: {channel} {version}")
}
pub fn summary(updated: usize, left_out: usize) -> String {
    format!("{updated} Geräte werden aktualisiert, {left_out} ausgelassen")
}
pub fn version_change(from: &str, to: &str) -> String {
    format!("{from} → {to}")
}
/// `position`: the device's place in the order, from 1
pub fn position(position: usize, pilot: bool, depth: Option<i32>) -> String {
    let why = match (pilot, depth) {
        (true, _) => "Pilot seiner Architektur".to_string(),
        (false, Some(1)) => "1 Schritt von der Wurzel".to_string(),
        (false, Some(depth)) => format!("{depth} Schritte von der Wurzel"),
        (false, None) => "abseits der Wege aus NetBox".to_string(),
    };
    format!("{position}. ({why})")
}
/// download and free space in bytes
pub fn space(download: Option<u64>, free: Option<u64>) -> String {
    let mib = |bytes: u64| format!("{:.1}", bytes as f64 / (1024.0 * 1024.0));
    match (download, free) {
        (Some(download), Some(free)) => format!("{} MiB, {} MiB frei", mib(download), mib(free)),
        (Some(download), None) => format!("{} MiB", mib(download)),
        (None, Some(free)) => format!("{} MiB frei", mib(free)),
        (None, None) => String::new(),
    }
}
