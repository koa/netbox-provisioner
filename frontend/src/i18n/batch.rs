pub const OPEN: &str = "Gemeinsam mit anderen Geräten anwenden …";
pub const TITLE: &str = "Gemeinsam anwenden";
pub const EXPLANATION: &str = "Alle ausgewählten Geräte und dieses bekommen ihre Änderungen \
    mit einem gemeinsamen Fallback: er wird erst weggenommen, wenn alle wieder erreichbar \
    sind, sonst laden alle die Konfiguration von vorher. Für die beiden Enden einer \
    Funkstrecke, die nur zusammen wieder eine Verbindung haben.";
pub const NO_CANDIDATES: &str = "Keine anderen RouterOS-Geräte an diesem Standort oder am \
    anderen Ende einer Funkstrecke.";
pub const PARTNER: &str = "Funkstrecke";
pub const IN_LINE: &str = "Keine Änderungen";
pub const NO_ACCESS: &str = "Keine Adresse oder Zugangsdaten";
pub const NOT_READY: &str = "Erst wenn alle ausgewählten Geräte erreichbar sind und ihre \
    Änderungen geladen sind.";
pub fn unreachable(problem: &str) -> String {
    format!("Nicht erreichbar: {problem}")
}
/// `count` devices, this one included
pub fn apply(count: usize) -> String {
    format!("{count} Geräte gemeinsam anwenden")
}
