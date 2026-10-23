use yew::{Html, html};

pub const TITLE: &str = "Vergleich mit den Geräten";
pub const INTRO: &str = "Liest auf jedem Gerät, wen es an seinen Ports sieht (/ip/neighbor), und vergleicht das mit den Kabeln in NetBox. Auf den Geräten ändert sich nichts.";
pub const START: &str = "Nachbarn lesen";
pub const AGAIN: &str = "Nochmals lesen";
pub const IN_LINE: &str = "passt zu NetBox";
pub const UNREACHABLE: &str = "nicht erreichbar";
pub const TAKE_OVER: &str = "Als Verbindung übernehmen";

/// what the device sees: its entries, separated
pub fn seen(entries: Vec<Html>) -> Html {
    let mut parts = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        if index > 0 {
            parts.push(html!({", "}));
        }
        parts.push(entry);
    }
    html!(<>{for parts}</>)
}
/// a neighbor NetBox doesn't know: its identity, port and board
pub fn unknown(identity: &str, interface: &str, board: &str) -> String {
    format!("{identity} {interface} ({board})")
}
pub fn cable_missing(port: Html, seen: Html) -> Html {
    html!(<>{"An "}{port}{" hat NetBox kein Kabel, das Gerät sieht dort "}{seen}{"."}</>)
}
pub fn cable_elsewhere(port: Html, far_end: Html, seen: Html) -> Html {
    html!(<>{"Das Kabel an "}{port}{" geht laut NetBox zu "}{far_end}{", das Gerät sieht dort aber "}{seen}{"."}</>)
}
pub fn other_far_port(port: Html, far_end: Html, seen: Html) -> Html {
    html!(<>{"An "}{port}{" hängt "}{seen}{", laut NetBox "}{far_end}{"."}</>)
}
pub fn not_seen(port: Html, far_end: Html) -> Html {
    html!(<>{"An "}{port}{" sieht das Gerät niemanden, laut NetBox hängt dort "}{far_end}{"."}</>)
}
pub fn discovery_off(list: &str) -> String {
    format!("Die Discovery des Geräts ist aus: /ip/neighbor/discovery-settings verweist auf die Interface-Liste {list}, die es nicht gibt. Es sieht deshalb niemanden.")
}
pub fn not_in_netbox(port: Html, seen: Html) -> Html {
    html!(<>{"An "}{port}{" sieht das Gerät "}{seen}{", die NetBox nicht kennt."}</>)
}
