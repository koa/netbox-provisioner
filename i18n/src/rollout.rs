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
/// download and free space in bytes; `in_ram`: the free space is memory (the root directory
/// is in RAM on devices with 16 MiB of flash)
pub fn space(download: Option<u64>, free: Option<u64>, in_ram: bool) -> String {
    let mib = |bytes: u64| format!("{:.1}", bytes as f64 / (1024.0 * 1024.0));
    let free = free.map(|free| match in_ram {
        true => format!("{} MiB RAM frei", mib(free)),
        false => format!("{} MiB frei", mib(free)),
    });
    match (download, free) {
        (Some(download), Some(free)) => format!("{} MiB, {free}", mib(download)),
        (Some(download), None) => format!("{} MiB", mib(download)),
        (None, Some(free)) => free,
        (None, None) => String::new(),
    }
}

pub const PILOT: &str = "Pilot";
pub const START: &str = "Rollout starten";
pub const NOT_ALLOWED: &str = "Schreiben auf Geräte ist abgeschaltet (allow_apply)";
/// the job reads the devices again before it starts
pub const PLANNING_AGAIN: &str = "Der Rollout liest die Geräte noch einmal …";
pub const STATE: &str = "Stand";
pub const CONTINUE: &str = "Weiter";
pub const END: &str = "Rollout beenden";
pub const CANCEL: &str = "Rollout abbrechen";
pub const SKIP: &str = "Überspringen";
pub const SHOW_JOB: &str = "Ablauf";
pub const HIDE_JOB: &str = "Ablauf ausblenden";
pub fn job_of(device: &str) -> String {
    format!("Ablauf von {device}")
}
pub const PAUSED_AFTER_PILOTS: &str = "Die Piloten sind durch. Prüfe sie auf ihren Seiten; \
    „Weiter“ aktualisiert die übrigen Geräte.";
pub fn paused_failed(device: &str) -> String {
    format!(
        "{device} ist fehlgeschlagen und bleibt, wie es ist; die laufenden Updates sind zu \
         Ende. „Weiter“ macht mit den übrigen Geräten weiter."
    )
}
pub const STATE_PENDING: &str = "wartet auf seine Reihe";
pub const STATE_WAITING: &str = "wartet auf einen anderen Job des Geräts";
pub const STATE_STARTED: &str = "Update läuft";
/// The device is offline: it reboots, or `by` does, which it is behind or powered by.
pub fn offline(by: Option<&str>) -> String {
    match by {
        None => "offline: startet neu".into(),
        Some(by) => format!("offline: {by} startet neu"),
    }
}
pub const STATE_DONE: &str = "aktualisiert";
pub const STATE_FAILED: &str = "fehlgeschlagen";
pub const STATE_CANCELLED: &str = "abgebrochen";
pub const STATE_SKIPPED: &str = "übersprungen";
pub const CANCELLED: &str = "Der Rollout wurde abgebrochen; nicht gestartete Geräte bleiben, wie \
    sie sind.";
pub const LOST: &str = "Der Server kennt den Rollout nicht mehr (Neustart). Ein neuer Rollout \
    lässt aus, was schon aktuell ist.";
pub fn done(updated: i32, skipped: i32, failed: i32) -> String {
    format!(
        "Rollout fertig: {updated} aktualisiert, {skipped} übersprungen, {failed} fehlgeschlagen"
    )
}
