pub const PLAN: &str = "Plan";
pub const PREVIEW: &str = "Vorschau";
pub const WRITE: &str = "In NetBox schreiben";
pub const APPLY: &str = "Anwenden";
pub const APPLY_NOW: &str = "Jetzt anwenden";
pub const LATER: &str = "Später";
pub const CLOSE: &str = "Schliessen";
/// „Funkbrücke“ from a free router port: the bridge device to cable there
pub const BRIDGE_DEVICE: &str = "Brückengerät";
pub const CHOOSE_BRIDGE_DEVICE: &str = "Brückengerät wählen";
pub const NO_BRIDGE_DEVICES: &str =
    "Kein Gerät mit genau einem Radio und ohne Kabel zu einem Router.";
/// a bridge device in the select: name, radio, the other bridge of its link
pub fn bridge_device(name: &str, radio: &str, peer: Option<&str>) -> String {
    match peer {
        Some(peer) => format!("{name} ({radio}, Gegenstelle {peer})"),
        None => format!("{name} ({radio})"),
    }
}
