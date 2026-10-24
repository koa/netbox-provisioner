use yew::{Html, html};

pub const TITLE: &str = "Befunde in NetBox";
pub const NETWORK_TITLE: &str = "Befunde zum ganzen Netz";
pub const NONE: &str = "Keine Befunde: NetBox ist für diese Site vollständig, soweit der Provisioner das prüft.";
pub const ERROR: &str = "Fehler";
pub const WARNING: &str = "Warnung";
pub const HINT: &str = "Hinweis";

/// on a device or site card: how many findings, how many of them errors
pub fn count(all: usize, errors: usize) -> Option<String> {
    let all_text = match all {
        0 => return None,
        1 => "1 Befund".to_string(),
        n => format!("{n} Befunde"),
    };
    Some(match errors {
        0 => all_text,
        1 => format!("{all_text}, davon 1 Fehler"),
        n => format!("{all_text}, davon {n} Fehler"),
    })
}

/// the tag of a prefix pool
pub fn pool(loopback: bool) -> &'static str {
    if loopback { "loopback" } else { "transfer" }
}
pub fn version(v6: bool) -> &'static str {
    if v6 { "IPv6" } else { "IPv4" }
}

pub fn pool_missing(pool: Html, version: &str) -> Html {
    html!(<>{"Kein Prefix mit dem Tag "}{pool}{" für "}{version}{": Der Helfer hat nichts, woraus er vergeben kann."}</>)
}
pub fn pool_nearly_full(pool: Html, version: &str, free: i64, size: i64) -> Html {
    html!(<>{"Der Pool "}{pool}{format!(" ({version}) ist fast voll: {free} von {size} Adressen frei.")}</>)
}
pub fn no_loopback(device: Html) -> Html {
    html!(<>{device}{" hat kein Interface „lo“ (Typ Virtual) für die Loopback."}</>)
}
pub fn no_primary_ipv4(device: Html) -> Html {
    html!(<>{device}{" hat keine Primary IPv4: keine OSPF-Router-ID, und keine Adresse aus NetBox, unter der es erreichbar ist."}</>)
}
pub fn primary_not_on_loopback(device: Html, address: Html, interface: Option<Html>) -> Html {
    match interface {
        Some(interface) => html!(<>{"Die Primary IPv4 "}{address}{" von "}{device}{" liegt nicht auf „lo“, sondern auf "}{interface}{"."}</>),
        None => html!(<>{"Die Primary IPv4 "}{address}{" von "}{device}{" liegt auf keinem Interface."}</>),
    }
}
pub fn address_without_prefix(interface: Html, address: Html, dhcp: bool) -> Html {
    let consequence = if dhcp {
        ": Der DHCP-Server dort entsteht nicht."
    } else {
        "."
    };
    html!(<>{address}{" an "}{interface}{" hat kein Prefix gleicher Länge"}{consequence}</>)
}
pub fn dhcp_pool_empty(interface: Html, address: Html, prefix: Option<Html>) -> Html {
    match prefix {
        Some(prefix) => html!(<>{interface}{" hat das Tag „dhcp“, aber im Prefix "}{prefix}{" von "}{address}{" ist keine Adresse für einen Pool frei: Der DHCP-Server dort entsteht nicht."}</>),
        None => html!(<>{interface}{" hat das Tag „dhcp“, aber "}{address}{" ist eine einzelne Adresse ohne Netz für Clients: Der DHCP-Server dort entsteht nicht."}</>),
    }
}
/// the link text of a VLAN
pub fn vlan_name(vid: i64, name: &str) -> String {
    format!("VLAN {vid} ({name})")
}
/// items joined by commas, the last by „und“
pub fn list(items: Vec<Html>) -> Html {
    let count = items.len();
    html! {<>{ for items.into_iter().enumerate().map(|(index, item)| html! {<>
        if index > 0 { { if index + 1 == count { " und " } else { ", " } } }
        {item}
    </>}) }</>}
}
pub fn vlan_with_several_gateways(vlan: Html, interfaces: Html) -> Html {
    html!(<>{vlan}{" hat mehr als einen L3-Übergang im selben Netz: "}{interfaces}{". Genau ein Gerät soll eine Adresse darin haben, die übrigen geben das VLAN nur auf L2 weiter."}</>)
}
pub fn vlan_without_gateway(vlan: Html, interfaces: Option<Html>) -> Html {
    html! {<>
        {vlan}{" hat in seinem Netz keinen L3-Übergang: Kein Gerät hat eine Adresse darin, die Clients erreichen nichts."}
        if let Some(interfaces) = interfaces { {" Untagged an "}{interfaces}{"."} }
    </>}
}
pub fn tagged_vlan_on_port(interface: Html, vlans: Html) -> Html {
    html!(<>{interface}{" führt tagged "}{vlans}{". Tagged VLANs an Ports sind nicht vorgesehen: Ein VLAN kommt über VXLAN zu einem anderen Gerät („vxlan_vni“ an der VLAN-Gruppe), eigene APs und Switches über eine Direktverbindung."}</>)
}
pub const RADIO_ROUTER_CABLE: &str = "das Kabel zum Router";
pub const RADIO_WIRELESS_LINK: &str = "der Wireless Link zum anderen Brückengerät";
pub const RADIO_SSID: &str = "die SSID des Wireless Links";
pub const RADIO_PEER_ROUTER_CABLE: &str = "das Kabel vom anderen Brückengerät zum Router";
pub fn radio_path_incomplete(radio: Html, missing: Vec<&'static str>) -> Html {
    html!(<>{"Die Richtfunk-Strecke von "}{radio}{" ist unvollständig, in NetBox fehlt "}{list(missing.into_iter().map(Html::from).collect())}{": Der Provisioner richtet den Weg zwischen den Routern nicht ein."}</>)
}
pub fn label_not_far_end(interface: Html, label: Option<&str>, far_device: Html) -> Html {
    html! {<>
        { match label {
            Some(label) => html!(<>{interface}{format!(" heisst „{label}“, am anderen Ende ist aber ")}</>),
            None => html!(<>{interface}{" hat kein Label, am anderen Ende ist "}</>),
        } }
        {far_device}{". Das Label geht in den Portnamen auf dem Gerät ein („e02-ap1“), „Verbindung“ setzt es so."}
    </>}
}
pub fn vlan_without_port(interface: Html, vlan: Html) -> Html {
    html!(<>{"Das Netz an "}{interface}{" erreicht niemanden: "}{vlan}{" liegt an keinem Port des Geräts, in keinem WLAN und geht nicht über VXLAN weiter. „VLAN am Port“ legt es an einen Port."}</>)
}
pub fn wlan_without_vlan(wlan: Html, group: Html) -> Html {
    html!(<>{"Das WLAN "}{wlan}{" der Gruppe "}{group}{" hat keine VLAN: Die APs legen seine Clients in keine Bridge und haben kein VXLAN dafür, die Clients verbinden sich und erreichen nichts."}</>)
}
pub fn port_like_name(interface: Html) -> Html {
    html!(<>{interface}{" ist ein physisches Interface, heisst aber nicht wie ein RouterOS-Port: Der Generator nimmt es nicht als Port."}</>)
}
pub fn loopback_without_ospf(device: Html, address: Html) -> Html {
    html!(<>{device}{" hat seine Primary IPv4 "}{address}{" auf lo, aber kein Interface mit ospf: Niemand kündigt sie an, der Provisioner erreicht das Gerät dort nicht."}</>)
}
pub fn radio_without_role(interface: Html) -> Html {
    html!(<>{interface}{" ist ein Ende eines Wireless Links, hat aber keine RF-Rolle (AP oder Station): Der Provisioner richtet die Funkbrücke nicht ein, das Radio behält seine Konfiguration."}</>)
}
pub fn vlan_without_group(interface: Html, vlan: Html) -> Html {
    html!(<>{interface}{" hat ein "}{vlan}{" ohne VLAN-Gruppe: Der Provisioner liest es nicht."}</>)
}
/// the link text of a VLAN known only by its NetBox id
pub fn vlan(id: i64) -> String {
    format!("VLAN (NetBox-ID {id})")
}
pub fn mtu_mismatch(interface: Html, mtu: i64, far_end: Html, far_mtu: i64) -> Html {
    html!(<>{"Ungleiche MTU an einem Kabel: "}{interface}{format!(" {mtu}, ")}{far_end}{format!(" {far_mtu}. OSPF kommt dazwischen nicht hoch.")}</>)
}
pub fn ospf_on_one_end(interface: Html, far_end: Html) -> Html {
    html!(<>{interface}{" hat den Tag „ospf“, das andere Ende "}{far_end}{" nicht."}</>)
}
pub fn no_common_subnet(interface: Html, far_end: Html) -> Html {
    html!(<>{interface}{" und "}{far_end}{" sind verkabelt, haben aber keine Adressen im selben Netz."}</>)
}
pub fn address_without_ospf(interface: Html, far_end: Html) -> Html {
    html!(<>{interface}{" und "}{far_end}{" sind verkabelt und im selben Netz, aber ohne „ospf“. Ist es ein Kunden-Port?"}</>)
}
