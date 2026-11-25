//! Scope: The texts of the page „Zustand“; nothing else.
use crate::network_map::uptime;

pub const TITLE: &str = "Zustand";
pub const INTRO: &str = "Was im Netz zu kontrollieren oder zu warten ist, das Dringendste zuerst: Router, die der Provisioner nicht erreicht, Strecken unten, Uplinks mit Verlust, Neustarts der letzten 24 h, PoE-Ports ohne Last. Liest beim Öffnen die OSPF-Datenbank, jeden Router und den Ping zum Gateway des Uplinks. Health wird gezeigt, nicht beurteilt; Alarme gehören in Prometheus.";
pub const RELOAD: &str = "Neu lesen";
pub const LOADING: &str = "Liest die Router";
pub const NOTHING: &str = "Nichts zu tun: alle Router erreichbar, alle Strecken oben.";
pub const DEVICES: &str = "Router";
pub const UNREACHABLE: &str = "nicht erreichbar";
pub const NO_DEFAULT_ROUTE: &str = "Uplink ohne aktive Default-Route";
pub const UNKNOWN: &str = "unbekannter Befund (Frontend zu alt?)";
pub fn lsdb_error(error: &str) -> String {
    format!("Keine OSPF-Datenbank lesbar, die Strecken sind unbekannt: {error}")
}
/// `radio`: a wireless link, else a cable
pub fn link_down(radio: bool, a: &str, b: &str) -> String {
    let what = if radio { "Funkstrecke" } else { "Kabel" };
    format!("{what} unten: {a} – {b}")
}
pub fn link_ipv6_down(a: &str, b: &str) -> String {
    format!("nur OSPFv2 oben, OSPFv3 nicht (IPv6 aus?): {a} – {b}")
}
pub fn uplink_loss(loss: f64, gateway: &str, rtt_ms: Option<f64>) -> String {
    let percent = (loss * 100.0).round();
    match rtt_ms {
        Some(rtt) => format!("Uplink: {percent} % Verlust zum Gateway {gateway}, {rtt:.1} ms"),
        None => format!("Uplink: Gateway {gateway} antwortet nicht"),
    }
}
pub fn rebooted(uptime_seconds: u64) -> String {
    format!("vor {} neu gestartet", uptime(uptime_seconds))
}
pub fn poe_no_load(port: &str) -> String {
    format!("PoE an {port}: nichts zieht Strom")
}
