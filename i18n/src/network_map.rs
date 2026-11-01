pub const TITLE: &str = "Netz-Karte";
pub const OPEN: &str = "Netz-Karte";
pub const OPEN_ALL: &str = "Netz-Karte (alle Sites)";
pub const INTRO: &str = "Die Geräte der Site und ihre Kabel und Funkstrecken aus NetBox, ihr Zustand aus der OSPF-Datenbank eines Routers; Geräte ohne OSPF werden nicht dargestellt. Liest nur, alle paar Sekunden neu.";
pub const INTRO_ALL: &str = "Alle Geräte des OSPF-Netzes über alle Sites, ihre Kabel und Funkstrecken aus NetBox, ihr Zustand aus der OSPF-Datenbank eines Routers; Geräte ohne OSPF werden nicht dargestellt. Liest nur, alle paar Sekunden neu.";
pub const NO_ROUTER: &str = "Die Site hat keinen OSPF-Router, von dem sich die OSPF-Datenbank lesen liesse.";
pub const NO_ROUTER_ALL: &str = "Kein Gerät hat einen OSPF-Router, von dem sich die OSPF-Datenbank lesen liesse.";
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
pub const NODE_ISOLATED: &str = "abgeschnitten: noch in der OSPF-Datenbank, aber keine Strecke oben";
pub const NODE_MISSING: &str = "fehlt in OSPF";
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
