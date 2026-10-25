pub const TITLE: &str = "NetBox ergänzen";
pub const LOOPBACK: &str = "Loopback";
pub const CONNECTION: &str = "Verbindung";
pub const CONNECTION_HINT: &str = "Verbindet zwei Router direkt: Kabel, falls es fehlt, ein /31 \
    aus dem Transfer-Pool mit je einer Adresse an beiden Enden, ospf, dieselbe MTU und als \
    Label den Namen der Gegenseite.";
pub const DISCONNECT: &str = "Verbindung abräumen";
pub const DISCONNECT_HINT: &str = "Nimmt eine falsche Verbindung aus NetBox: das Kabel am Port, \
    seine Adressen aus dem Transfer-Pool und die der Gegenseite im selben Netz, und das Prefix \
    dieses Netzes, wenn danach nichts mehr darin ist. Danach lässt sich die richtige mit \
    „Verbindung“ anlegen.";
pub const CABLE_TAKEOVER: &str = "Kabel übernehmen";
pub const CABLE_TAKEOVER_HINT: &str = "Das Gerät sieht am Port einen anderen Router als NetBox \
    dort hat: räumt die Verbindungen ab, die NetBox an diesem Port und am gesehenen Port hat \
    (wie „Verbindung abräumen“), und legt die gesehene an (wie „Verbindung“).";
pub const CHOOSE_DISCONNECT_PORT: &str = "Den Port wählen, dessen Verbindung weg soll.";
pub const CUSTOMER_NET: &str = "Kunden-Netz";
pub const CUSTOMER_NET_HINT: &str = "Legt ein Netz für einen Port oder ein VLAN an: das \
    nächste freie Netz der gewählten Länge im gewählten Prefix der Site, die erste Adresse \
    als Gateway (auf einem VLAN an einem neuen virtuellen Interface mit dem Namen des VLANs) \
    und, falls gewählt, das Tag dhcp. Der DHCP-Pool ist dann der Rest des Netzes. Mit einem \
    IPv6-Prefix dazu das nächste freie /64 mit ::1 als Gateway, per SLAAC angekündigt; ein \
    Kunden-Netz mit IPv4 bekommt so auch später noch sein IPv6.";
pub const NET_TARGET: &str = "Port oder VLAN";
pub const CHOOSE_NET_TARGET: &str = "Port oder VLAN wählen";
pub const NO_NET_TARGETS: &str = "Das Gerät hat keinen freien Port (ohne IPv4 oder ein \
    Kunden-Netz ohne IPv6, ohne Bridge oder VLAN, nicht zu einem Router) und kein VLAN ohne \
    Netz.";
pub const PARENT: &str = "IPv4-Prefix der Site";
pub const CHOOSE_PARENT: &str = "Prefix wählen";
pub const PARENT_V6: &str = "IPv6-Prefix der Site";
pub const NO_PARENT: &str = "keins";
pub const NO_PARENTS_V6: &str = "Die Site hat kein IPv6-Prefix, das Netze aufnehmen kann \
    (Scope Site, ohne Tag loopback/transfer, selbst kein Netz mit Adressen).";
pub const NO_PARENTS: &str = "Die Site hat kein IPv4-Prefix, das Netze aufnehmen kann \
    (Scope Site, ohne Tag loopback/transfer, selbst kein Netz mit Adressen).";
pub const LENGTH: &str = "Länge";
pub const LENGTH_PLACEHOLDER: &str = "24";
pub const DHCP_SERVER: &str = "DHCP-Server (Tag dhcp)";
pub const CHOOSE_NET: &str = "Port oder VLAN und ein IPv4- oder IPv6-Prefix wählen.";
/// the port given by its row
pub const CHOOSE_NET_PREFIX: &str = "Ein IPv4- oder IPv6-Prefix wählen.";
pub fn net_vlan(name: &str, vid: i64, interface: Option<&str>) -> String {
    match interface {
        Some(interface) => format!("VLAN {name} ({vid}), an {interface}"),
        None => format!("VLAN {name} ({vid}), neues Interface"),
    }
}
pub fn net_port(name: &str, label: Option<&str>) -> String {
    match label {
        Some(label) => format!("{name} ({label})"),
        None => name.to_string(),
    }
}
pub const POE: &str = "PoE";
pub const POE_HINT: &str = "Hält fest, wer wen über PoE speist: der speisende Port bekommt \
    PoE-Modus PSE, der gespeiste PD, und das Kabel dazwischen, falls es fehlt. Die Ports \
    von Hand wählen oder vom Gerät ablesen, ob er gerade speist. „Kein PoE“ nimmt einen \
    Eintrag wieder heraus. Ob das Board am Port PoE-out hat, weiss NetBox nicht; die \
    Live-Prüfung der Interfaces vergleicht es mit dem Gerät.";
pub const CHOOSE_POE: &str = "Richtung und beide Ports wählen.";
pub const CHOOSE_POE_PEER: &str = "Richtung und Port der Gegenseite wählen.";
pub const POE_DIRECTION: &str = "Richtung";
pub const POE_FEEDS: &str = "Dieses Gerät speist die Gegenseite";
pub const POE_FED: &str = "Dieses Gerät wird von der Gegenseite gespeist";
pub const POE_OFF: &str = "Kein PoE an diesem Port";
pub const POE_PEER_LATER: &str = "Die Gegenseite kann offen bleiben: dann wird nur PoE am Port \
    eingeschaltet. Kommt das Gerät dort online, erfasst „Neues Gerät anschliessen“ es als \
    gespeist, oder „PoE vervollständigen“ ergänzt die Gegenseite.";
pub const POE_OFF_HINT: &str = "Nimmt den PoE-Eintrag des Ports aus NetBox, und am anderen Ende \
    des Kabels den passenden (wer von hier gespeist wird oder hierher speist).";
pub const POE_OBSERVE: &str = "Vom Gerät ablesen";
pub const POE_OBSERVED_NONE: &str = "Das Gerät speist an keinem Port etwas.";
pub const POE_TAKE: &str = "Übernehmen";
/// a port the device reports feeding, and what is at its other end; `recorded`: NetBox
/// has it so (cable, PSE, PD)
pub fn poe_observed(
    port: &str,
    far: Option<(&str, &str)>,
    from_neighbor: bool,
    recorded: bool,
) -> String {
    match far {
        Some((device, far)) if recorded => {
            format!("{port} speist {device} {far}, in NetBox so erfasst")
        }
        Some((device, far)) if from_neighbor => {
            format!("{port} speist, dort sieht das Gerät {device} {far} (kein Kabel in NetBox)")
        }
        Some((device, far)) => format!("{port} speist {device} {far} (Kabel in NetBox)"),
        None => format!("{port} speist, die Gegenseite ist unbekannt"),
    }
}
/// a port in the choice of „PoE“: its mode and where its cable goes
pub fn poe_port(
    name: &str,
    label: Option<&str>,
    poe: Option<&str>,
    connected: Option<(&str, &str)>,
    cabled: bool,
) -> String {
    let mut text = match label {
        Some(label) => format!("{name} ({label})"),
        None => name.to_string(),
    };
    if let Some(poe) = poe {
        text.push_str(&format!(", {poe}"));
    }
    match connected {
        Some((device, port)) => text.push_str(&format!(" – Kabel zu {device} {port}")),
        None if cabled => text.push_str(" – Kabel"),
        None => {}
    }
    text
}
pub const RADIO_BRIDGE: &str = "Funkbrücke";
pub const RADIO_BRIDGE_HINT: &str = "Vervollständigt eine Funkbrücke zwischen zwei Routern: \
    Kabel zu den Routern, falls sie fehlen, auf beiden Brücken die Bridge mit Ethernet-Port \
    und Funk-Port (so benannt wie in RouterOS), RF-Rolle AP und Station, lo mit Loopback, den \
    Wireless Link mit SSID und PSK (ein fehlender PSK wird erzeugt), und ein IPv4-Netz für \
    das Segment: das des Router-Ports, sonst ein neues /29 aus dem Transfer-Pool. Router-Ports \
    und Bridges bekommen je eine Adresse darin und ospf; für IPv6 reichen die Link-Local-Adressen. \
    Jedes Kabelende bekommt als Label den Namen des Geräts am anderen Ende. \
    Die Nutzdaten bleiben auf L2.";
pub const CHOOSE_RADIO_BRIDGE: &str = "Rolle dieser Brücke und die Gegenseite wählen.";
pub const NOT_A_RADIO_BRIDGE: &str = "Das Gerät hat nicht genau einen Funk-Port: keine \
    Funkbrücke. Den Helper auf der Seite einer Brücke öffnen.";
pub const RADIO_ROLE: &str = "Diese Brücke ist";
pub const RADIO_AP: &str = "AP";
pub const RADIO_STATION: &str = "Station";
pub const RADIO_PEER: &str = "Andere Brücke";
pub const ROUTER_DEVICE: &str = "Router";
pub const ROUTER_PORT: &str = "Router-Port";
pub const CABLED_ROUTER: &str = "wie verkabelt";
pub const BRIDGE_ADDRESS: &str = "IPv4 der Bridge";
pub const BRIDGE_ADDRESS_PLACEHOLDER: &str = "die vorhandene oder die nächste freie";
pub const SSID: &str = "SSID";
pub const SSID_PLACEHOLDER: &str = "die des Links, sonst der Name des APs";
/// a side of the form: this bridge or the other one
pub fn radio_side(device: &str, ap: bool) -> String {
    format!("{device} ({})", if ap { "AP" } else { "Station" })
}
/// a bridge in the choice: its radio and where its link goes
pub fn radio_candidate(device: &str, radio: &str, peer: Option<&str>) -> String {
    match peer {
        Some(peer) => format!("{device} ({radio}, Link zu {peer})"),
        None => format!("{device} ({radio})"),
    }
}
pub fn update_port(
    port: (&str, &str),
    name: Option<&str>,
    role: Option<(&str, Option<&str>)>,
    bridge: Option<&str>,
    leaves_bridge: Option<&str>,
) -> String {
    let mut changes = Vec::new();
    if let Some(name) = name {
        changes.push(format!("heisst {name}"));
    }
    if let Some((role, before)) = role {
        changes.push(match before {
            Some(before) => format!("RF-Rolle {role} (vorher {before})"),
            None => format!("RF-Rolle {role}"),
        });
    }
    if let Some(bridge) = bridge {
        changes.push(format!("Port von {bridge}"));
    }
    if let Some(bridge) = leaves_bridge {
        changes.push(format!("nicht mehr Port von {bridge}"));
    }
    format!("{} {}: {}", port.0, port.1, changes.join(", "))
}
pub fn create_wireless_link(a: (&str, &str), b: (&str, &str), ssid: &str) -> String {
    format!(
        "Wireless Link {} {} – {} {} anlegen, SSID {ssid}, WPA mit neuem PSK",
        a.0, a.1, b.0, b.1
    )
}
pub fn update_wireless_link(
    a: (&str, &str),
    b: (&str, &str),
    ssid: Option<(&str, &str)>,
    psk: bool,
) -> String {
    let mut changes = Vec::new();
    if let Some((ssid, before)) = ssid {
        changes.push(match before {
            "" => format!("SSID {ssid}"),
            before => format!("SSID {ssid} (vorher {before})"),
        });
    }
    if psk {
        changes.push("WPA mit neuem PSK".to_string());
    }
    format!("Wireless Link {} {} – {} {}: {}", a.0, a.1, b.0, b.1, changes.join(", "))
}
pub const FLAGS: &str = "Flags";
pub const FLAGS_HINT: &str = "Die Tags der Interfaces: ospf (OSPF), dhcp (DHCP-Server), \
    dhcp-client (DHCP-Client, falls ohne Adresse), nat (Uplink, ausgehend maskieren). Bei \
    Ports einer Bridge zählen die Tags der Bridge.";
pub const CHOOSE_FLAGS: &str = "Schalter umstellen, dann zeigt der Plan die Änderung.";
pub const INTERFACE: &str = "Interface";
/// the device's findings, `after` the switches or as they are
pub fn flag_findings(after: bool) -> &'static str {
    if after { "Befunde des Geräts danach:" } else { "Befunde des Geräts:" }
}
pub fn no_flag_findings(after: bool) -> &'static str {
    if after { "Danach keine Befunde zum Gerät." } else { "Keine Befunde zum Gerät." }
}
pub fn bridge_port(bridge: &str) -> String {
    format!("Port von {bridge}: dessen Tags zählen")
}
pub fn cabled_to(device: &str, port: &str) -> String {
    format!("Kabel zu {device} {port}")
}
pub const ADDRESSES_GO: &str = "Beim nächsten Anwenden verschwinden diese Adressen von den \
    Geräten, die Verbindung ist bis zur neuen ohne Adressen.";
pub const CUT_OFF: &str = "Über diese Verbindung allein erreicht: nach dem Anwenden des einen \
    Endes weg, bis die neue Verbindung an beiden Enden angewendet ist";
pub const APPLY_TOGETHER: &str = "Die Funkeinstellungen ändern sich: die beiden Seiten finden \
    sich erst wieder, wenn beide sie haben. Nur gemeinsam anwenden („Gemeinsam mit anderen \
    Geräten anwenden …“ auf der Geräteseite); nacheinander lädt der Fallback der zuerst \
    angewendeten Seite die alten Einstellungen zurück";
/// the button in „Verbindung“ for a chosen port whose link is in the way
pub fn disconnect_port(device: &str, port: &str) -> String {
    format!("Verbindung an {device} {port} abräumen")
}
pub fn delete_cable(port: (&str, &str), far: Option<(&str, &str)>) -> String {
    match far {
        Some(far) => format!("Kabel {} {} – {} {} löschen", port.0, port.1, far.0, far.1),
        None => format!("Kabel an {} {} löschen", port.0, port.1),
    }
}
pub fn delete_address(address: &str, device: &str, interface: &str) -> String {
    format!("Adresse {address} auf {device} {interface} löschen")
}
pub fn delete_wireless_link(a: (&str, &str), b: (&str, &str)) -> String {
    format!("Wireless Link {} {} – {} {} löschen", a.0, a.1, b.0, b.1)
}
pub fn update_port_vlan(
    port: (&str, &str),
    vlan: Option<(i64, &str)>,
    before: Option<(i64, &str)>,
    tagged_before: &[(i64, &str)],
) -> String {
    let name = |(vid, name): (i64, &str)| format!("VLAN {vid} ({name})");
    let mut text = match (vlan, before) {
        (Some(vlan), Some(before)) => {
            format!("{} {}: untagged {} statt {}", port.0, port.1, name(vlan), name(before))
        }
        (Some(vlan), None) => format!("{} {}: untagged {}", port.0, port.1, name(vlan)),
        (None, Some(before)) => format!("{} {}: {} weg", port.0, port.1, name(before)),
        (None, None) => format!("{} {}:", port.0, port.1),
    };
    if !tagged_before.is_empty() {
        let tagged = tagged_before.iter().map(|v| name(*v)).collect::<Vec<_>>().join(", ");
        text.push_str(&format!(", tagged {tagged} weg"));
    }
    text
}
pub fn delete_interface(interface: (&str, &str)) -> String {
    format!("Interface {} {} löschen", interface.0, interface.1)
}
pub const BRIDGE_LEAVE: &str = "Aus Bridge lösen";
pub const BRIDGE_LEAVE_HINT: &str = "Nimmt den Port aus seiner Bridge, mit den VLANs des Ports \
    selbst: danach ist er frei. Sein Kabel bleibt, die Bridge mit ihrem Netz auch.";
pub const CHOOSE_BRIDGE_LEAVE: &str = "Über den Port der Bridge in den Interfaces starten.";
pub const LEASE_TAKEOVER: &str = "Lease übernehmen";
pub const LEASE_TAKEOVER_HINT: &str = "Trägt den Client eines statischen DHCP-Leases, den nur der \
    Router hat, in NetBox ein: ein Gerät mit Interface eth0, seiner MAC und der Adresse, damit das \
    nächste Anwenden den Lease schreibt statt entfernt.";
pub const CHOOSE_LEASE_TAKEOVER: &str = "Über den Lease in der Live-Prüfung der Interfaces starten.";
pub const LEASE_NAME: &str = "Name des Clients";
pub const LEASE_NAME_PLACEHOLDER: &str = "drucker-buero";
pub const LEASE_ROLE_PLACEHOLDER: &str = "Rolle wählen";
pub fn lease_seen(address: &str, mac: &str) -> String {
    format!("Lease {address} für {mac}")
}
pub const ADDRESS_TAKEOVER: &str = "Adresse übernehmen";
pub const ADDRESS_TAKEOVER_HINT: &str = "Trägt eine Adresse, die nur das Gerät hat, in NetBox an \
    ihrem Interface ein, damit das nächste Anwenden sie nicht entfernt. Ihr Netz wird ein Prefix, \
    wenn NetBox davon noch nichts hat.";
pub const CHOOSE_ADDRESS_TAKEOVER: &str = "Über die Adresse in der Live-Prüfung der Interfaces starten.";
pub const NEW_DEVICE: &str = "Neues Gerät";
pub const NEW_DEVICE_HINT: &str = "Legt in NetBox ein Gerät an, das an diesem Port hängt: an der \
    Site und mit dem Tenant dieses Geräts (dessen Zugangsdaten die Ersteinrichtung braucht), \
    Plattform RouterOS, dazu seine Loopback und, ausser für ein Brückengerät, die Verbindung \
    hierher. Weitere Ports legt NetBox mit dem Device Type an.";
pub const CHOOSE_NEW_DEVICE_NAME: &str = "Name, Device Type und Rolle wählen.";
pub const CHOOSE_NEW_DEVICE: &str = "Über einen freien Port in den Interfaces starten.";
pub const NEW_DEVICE_NAME: &str = "Name";
pub const NEW_DEVICE_TYPE: &str = "Device Type";
pub const NEW_DEVICE_NAME_PLACEHOLDER: &str = "z.B. ap-buero";
pub const NEW_DEVICE_TYPE_PLACEHOLDER: &str = "Device Type wählen";
pub const NEW_DEVICE_ROLE: &str = "Rolle";
pub const NEW_DEVICE_PORT: &str = "Port am neuen Gerät";
pub const NEW_DEVICE_WLAN_GROUP: &str = "WLAN-Gruppe (Access Point)";
pub const NEW_DEVICE_NO_WLAN_GROUP: &str = "keine";
pub const NEW_DEVICE_NO_TYPES: &str = "NetBox hat keinen Device Type von MikroTik: zuerst dort anlegen.";
pub const NEW_DEVICE_OPEN: &str = "Zum neuen Gerät";
/// the board RouterOS reports at the port, whether a device type is it
pub fn new_device_seen(identity: &str, board: &str, port: &str, matched: bool) -> String {
    if matched {
        format!("Hier meldet sich {identity} ({board}) mit {port}.")
    } else {
        format!(
            "Hier meldet sich {identity} ({board}) mit {port}; kein Device Type hat dieses Board \
            als Modell oder Part Number, bitte wählen."
        )
    }
}
/// a WLAN group with its controller
pub fn wlan_group(name: &str, controller: &str) -> String {
    format!("{name} (Controller {controller})")
}
/// the plan takes a bridge's last port
pub fn emptied_bridge(bridge: &str) -> String {
    format!("{bridge} hat danach keinen Port mehr")
}
pub const EMPTIED_BRIDGE_TEXT: &str = "Ihr Netz erreicht dann niemanden. „Verbindung entfernen“ \
    auf ihrer Zeile nimmt es aus NetBox.";
pub const CUSTOMER_DISCONNECT: &str = "Kunden-Anschluss abräumen";
pub const CUSTOMER_DISCONNECT_HINT: &str = "Nimmt das Netz eines Kunden-Anschlusses aus NetBox: \
    die Adressen am Interface, ihr Prefix, wenn danach nichts mehr darin ist, dhcp und die VLANs \
    am Port (bei einer Bridge auch an ihren Ports, bei einem VLAN-Interface an den Kunden-Ports \
    mit seinem VLAN). Ein VLAN-Interface, das sonst nichts hat, wird gelöscht. Das VLAN selbst \
    bleibt.";
pub const CHOOSE_CUSTOMER_DISCONNECT: &str = "Über die Kunden-Zeile der Interfaces starten.";
pub const PORT_VLAN: &str = "VLAN am Port";
pub const PORT_VLAN_HINT: &str = "Legt ein VLAN untagged an einen Port (Mode access), für einen \
    Kunden: das Netz eines VLAN-Interfaces an einen Port, oder ein VLAN, dessen Netz auf einem \
    anderen Gerät liegt und über VXLAN kommt. Tagged VLANs am Port fallen weg.";
pub const CHOOSE_PORT_VLAN: &str = "Port und VLAN wählen.";
pub const CHOOSE_PORT_VLAN_GROUP_VLAN: &str = "VLAN-Gruppe und VLAN wählen.";
pub const PORT_VLAN_PORT: &str = "Port";
pub const PORT_VLAN_GROUP: &str = "VLAN-Gruppe";
pub const PORT_VLAN_VLAN: &str = "VLAN";
pub const CHOOSE_PORT_VLAN_PORT: &str = "Port wählen";
pub const CHOOSE_PORT_VLAN_GROUP: &str = "Gruppe wählen";
pub const CHOOSE_PORT_VLAN_VLAN: &str = "VLAN wählen";
pub fn port_vlan_group(name: &str, vxlan: bool) -> String {
    if vxlan {
        format!("{name} (über VXLAN)")
    } else {
        format!("{name} (lokal)")
    }
}
pub const NO_PORT_VLAN_PORTS: &str = "Kein Port frei: nur Ethernet-Ports ohne Bridge, Adresse, \
    Uplink und Kabel zu einem Router.";
pub const NO_PORT_VLAN_VLANS: &str = "Kein VLAN, das ein Port erreicht: eines des Geräts oder \
    eines aus einer VLAN-Gruppe mit vxlan_vni. Weggelassen sind VLANs mit einer VID, die auf dem \
    Gerät schon ein anderes hat, und VLANs einer Gruppe, deren VXLAN auf dem Gerät an einer \
    anderen Bridge hängt.";
pub fn port_vlan_port(name: &str, label: Option<&str>, vlan: Option<(i64, &str)>) -> String {
    let mut text = match label {
        Some(label) => format!("{name} ({label})"),
        None => name.to_string(),
    };
    if let Some((vid, vlan)) = vlan {
        text.push_str(&format!(", jetzt VLAN {vid} ({vlan})"));
    }
    text
}
pub fn port_vlan_vlan(vid: i64, name: &str) -> String {
    format!("VLAN {vid} ({name})")
}
pub const RADIO_DISCONNECT: &str = "Richtfunk abräumen";
pub const RADIO_DISCONNECT_HINT: &str = "Nimmt das Netz einer Richtfunk-Strecke aus NetBox: \
    die IPv4-Adressen des Segments an den beiden Router-Ports und den beiden Bridges, sein Prefix, \
    wenn danach nichts mehr darin ist, ospf und an beiden Kabeln die Labels, die das andere Ende \
    nennen. Kabel, Brückengeräte und Funk bleiben; den Wireless Link nur auf Wunsch. \
    Nach dem Anwenden sind die Brückengeräte nicht mehr erreichbar.";
pub const CHOOSE_RADIO_DISCONNECT: &str = "Über die Richtfunk-Zeile der Interfaces starten.";
pub const RADIO_DISCONNECT_LINK: &str = "Wireless Link auch löschen";
pub fn delete_prefix(prefix: &str) -> String {
    format!("Prefix {prefix} löschen")
}
pub const CHOOSE_PORTS: &str = "Beide Ports wählen.";
pub const CHOOSE_PEER: &str = "Gegenseite und ihren Port wählen.";
pub const COMPLETE: &str = "NetBox hat schon alles dafür.";
pub const PORT: &str = "Port";
pub const PEER_DEVICE: &str = "Gegenseite";
pub const PEER_PORT: &str = "Port der Gegenseite";
pub const MTU: &str = "MTU";
pub const CHOOSE_PORT: &str = "Port wählen";
pub const CHOOSE_DEVICE: &str = "Gerät wählen";
pub const CHOOSE_PEER_PORT: &str = "Port der Gegenseite wählen";
pub const MTU_PLACEHOLDER: &str = "wie an den Ports (die grössere)";
pub const LOOPBACK_HINT: &str = "Legt lo mit je einer Adresse aus den Loopback-Pools an und \
    macht sie zur Primary IP (Router-ID und Verwaltungsadresse).";
pub const PREVIEW: &str = "Vorschau";
pub const PREVIEW_TITLE: &str = "Was danach Anwenden auf dem Gerät tun würde:";
pub const PREVIEW_NOTHING: &str = "Anwenden hätte danach nichts zu tun.";
pub const PREVIEW_UNREACHABLE: &str = "Das Gerät ist nicht erreichbar: keine Vorschau des \
    Skripts.";
pub const FINDINGS_AFTER: &str = "Befunde danach:";
pub const NO_FINDINGS_AFTER: &str = "Danach keine Befunde mehr zum Gerät.";
pub const WRITE: &str = "In NetBox schreiben";
pub const UNDO: &str = "Rückgängig";
pub const LOCKED: &str = "Schreiben in NetBox gibt erst allow_netbox_write: true mit \
    netbox_write_token in der config.yaml frei.";
pub const WRITTEN: &str = "Geschrieben. Das Gerät bekommt es erst mit Anwenden.";

/// a finding by its kind (`__typename`), short
pub fn finding_kind(kind: String) -> String {
    match kind.as_str() {
        "NoLoopback" => "kein lo".into(),
        "NoPrimaryIpv4" => "keine Primary IPv4".into(),
        "PrimaryIpv4NotOnLoopback" => "Primary IPv4 nicht auf lo".into(),
        "AddressWithoutPrefix" => "Adresse ohne Prefix".into(),
        "DhcpPoolEmpty" => "DHCP ohne freie Adresse".into(),
        "PortLikeName" => "Name wie ein Port".into(),
        "RadioWithoutRole" => "Funk ohne RF-Rolle".into(),
        "LoopbackWithoutOspf" => "Loopback ohne OSPF".into(),
        "VlanWithoutGroup" => "VLAN ohne Gruppe".into(),
        "MtuMismatch" => "MTU verschieden".into(),
        "OspfOnOneEnd" => "OSPF nur an einem Ende".into(),
        "NoCommonSubnet" => "kein gemeinsames Netz".into(),
        "AddressWithoutOspf" => "Adresse ohne OSPF".into(),
        "WlanWithoutVlan" => "WLAN ohne VLAN".into(),
        _ => kind,
    }
}
pub fn findings_gone(gone: &[&String]) -> String {
    let gone: Vec<&str> = gone.iter().map(|kind| kind.as_str()).collect();
    format!("Erledigt: {}", gone.join(", "))
}
pub const UNDOCUMENTED: &str = "Belegt, aber nicht in NetBox: übersprungen, bitte dokumentieren";
pub fn checked_with(router: &str) -> String {
    format!("Die neuen Adressen sind gegen die Routen von {router} geprüft: dort nicht belegt.")
}
pub fn undocumented(address: &str, route: &str, gateway: Option<&str>, router: &str) -> String {
    match gateway {
        Some(gateway) => format!("{address}: {router} hat die Route {route} über {gateway}"),
        None => format!("{address}: {router} hat die Route {route}"),
    }
}
pub fn create_device(name: &str, device_type: &str, role: &str, site: &str, wlan_group: Option<&str>) -> String {
    match wlan_group {
        Some(group) => format!(
            "Gerät {name} anlegen ({device_type}, Rolle {role}, Site {site}, WLAN-Gruppe {group})"
        ),
        None => format!("Gerät {name} anlegen ({device_type}, Rolle {role}, Site {site})"),
    }
}
pub fn create_interface(device: &str, name: &str, kind: &str, tags: &[&str], mac: Option<&str>) -> String {
    let mut what = vec![kind.to_string()];
    if !tags.is_empty() {
        what.push(format!("{} {}", tags_word(tags), tags.join(", ")));
    }
    if let Some(mac) = mac {
        what.push(format!("MAC {mac}"));
    }
    format!("Interface {device} {name} ({}) anlegen", what.join(", "))
}
pub fn create_address(address: &str, device: &str, interface: &str, prefix: &str) -> String {
    format!("Adresse {address} auf {device} {interface} anlegen (aus {prefix})")
}
pub fn create_cable(a: (&str, &str), b: (&str, &str)) -> String {
    format!("Kabel {} {} – {} {} anlegen", a.0, a.1, b.0, b.1)
}
pub fn create_prefix(prefix: &str, pool: &str, description: &str) -> String {
    if pool.is_empty() {
        // a network NetBox has no prefix around (`helper::address_takeover`)
        format!("Prefix {prefix} „{description}“ anlegen")
    } else {
        format!("Prefix {prefix} „{description}“ anlegen (aus {pool})")
    }
}
/// the new label and MTU, each with the one before, and whether `ospf` is added
pub fn update_interface(
    port: (&str, &str),
    label: Option<(&str, Option<&str>)>,
    mtu: Option<(i64, Option<i64>)>,
    tags_added: &[&str],
    tags_removed: &[&str],
    poe: Option<(Option<&str>, Option<&str>)>,
) -> String {
    let mut changes = Vec::new();
    if let Some((poe, before)) = poe {
        changes.push(match (poe, before) {
            (Some(poe), Some(before)) => format!("PoE {poe} (vorher {before})"),
            (Some(poe), None) => format!("PoE {poe}"),
            (None, Some(before)) => format!("kein PoE mehr (vorher {before})"),
            (None, None) => "kein PoE".to_string(),
        });
    }
    if let Some((label, before)) = label {
        changes.push(match before {
            Some(before) if label.is_empty() => format!("Label {before} weg"),
            Some(before) => format!("Label {label} (vorher {before})"),
            None => format!("Label {label}"),
        });
    }
    if let Some((mtu, before)) = mtu {
        changes.push(match before {
            Some(before) => format!("MTU {mtu} (vorher {before})"),
            None => format!("MTU {mtu} (vorher 1500)"),
        });
    }
    if !tags_added.is_empty() {
        changes.push(format!("{} {}", tags_word(tags_added), tags_added.join(", ")));
    }
    if !tags_removed.is_empty() {
        changes.push(format!("{} {} weg", tags_word(tags_removed), tags_removed.join(", ")));
    }
    format!("{} {}: {}", port.0, port.1, changes.join(", "))
}
fn tags_word(tags: &[&str]) -> &'static str {
    if tags.len() == 1 { "Tag" } else { "Tags" }
}
/// a port in the choice of a link's ends: where its cable goes, what it has
pub fn link_port(
    name: &str,
    label: Option<&str>,
    connected: Option<(&str, &str)>,
    cabled: bool,
    ipv4: Option<&str>,
    bridge: Option<&str>,
) -> String {
    let mut text = match label {
        Some(label) => format!("{name} ({label})"),
        None => name.to_string(),
    };
    match connected {
        Some((device, port)) => text.push_str(&format!(" – Kabel zu {device} {port}")),
        None if cabled => text.push_str(" – Kabel"),
        None => {}
    }
    if let Some(ipv4) = ipv4 {
        text.push_str(&format!(", {ipv4}"));
    }
    if let Some(bridge) = bridge {
        text.push_str(&format!(", in {bridge}"));
    }
    text
}
pub fn link_device(name: &str, site: Option<&str>) -> String {
    match site {
        Some(site) => format!("{name} ({site})"),
        None => name.to_string(),
    }
}
pub fn set_primary(version: &str, device: &str, address: &str, before: Option<&str>) -> String {
    match before {
        Some(before) => {
            format!("Primary {version} von {device} auf {address} setzen (vorher {before})")
        }
        None => format!("Primary {version} von {device} auf {address} setzen"),
    }
}
