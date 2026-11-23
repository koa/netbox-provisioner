//! Scope: The texts of the residents' view and the admins' page of it; nothing else.

/// the title of a resident's own page
pub const MY_NETWORK: &str = "Mein Netz";
pub const INTRO: &str = "Deine Dosen und Geräte, und welches deiner Netze darauf läuft. Änderungen \
    sammelst du und speicherst sie zusammen.";
pub const TAB_PORTS: &str = "Dosen und Geräte";
pub const TAB_NETWORKS: &str = "Netze";
pub const NO_PORTS: &str = "Dir ist noch keine Dose und kein Gerät zugeordnet.";
pub const NO_NETWORKS: &str = "Dir ist noch kein Netz zugeordnet.";
/// a port without a VLAN of the resident
pub const NO_NETWORK: &str = "Kein Netz";
/// a port with a VLAN of somebody else, the operator's
pub const FOREIGN_NETWORK: &str = "Netz des Betreibers";
pub const CABLE: &str = "Kabel";
pub const YOUR_DEVICE: &str = "dein Gerät";
pub const NO_WLAN: &str = "Kein WLAN";
pub const NOBODY: &str = "Du bist angemeldet, aber weder Admin noch Anwohner. Ein Admin trägt dich bei deinem Haushalt ein.";

/// a port of the resident's own device
pub fn own_port(port: &str, device: &str) -> String {
    format!("Port {port} von {device}")
}

pub const SSID: &str = "WLAN-Name (SSID)";
pub const PSK: &str = "Passwort";
pub const PSK_HINT: &str = "12 bis 63 Zeichen, ohne Umlaute";
pub const SHOW_PSK: &str = "Zeigen";
pub const HIDE_PSK: &str = "Verbergen";
pub const RANDOM_PSK: &str = "Zufällig erzeugen";
pub const PLANNING: &str = "Vorschau wird berechnet …";
pub const SAVE: &str = "Speichern";
pub const CLOSE: &str = "Schliessen";
pub const SAVING: &str = "Wird gespeichert …";
pub const SAVED: &str = "In NetBox gespeichert";
pub const SAVED_HINT: &str = "Auf die Geräte kommt die Änderung mit dem Ausrollen; wie weit das \
    ist, zeigt diese Seite noch nicht.";
pub const NOT_SAVED: &str = "Nicht gespeichert";
pub const SHOW_DEVICES: &str = "Geräte im Netz";
/// the sockets and ports with the network, in its card
pub const CABLED_AT: &str = "Per Kabel an";
pub const NO_CABLED_PORTS: &str = "keiner Dose und keinem Port";
pub const READING_DEVICES: &str = "Geräte werden gelesen …";
pub const READ_AGAIN: &str = "Erneut lesen";
pub const NO_DEVICES: &str = "Keine Geräte gefunden";
pub const NO_HOSTNAME: &str = "Ohne Namen";

/// devices in a network: routers or controllers that didn't answer, so their devices are missing
pub fn devices_unreachable(devices: &[&str]) -> String {
    format!(
        "Nicht erreicht: {}; die Geräte dort fehlen in der Liste",
        devices.join(", ")
    )
}

/// devices in a network: one connected over WLAN, at an access point (empty: not known)
pub fn wlan_connection(access_point: &str, signal: i32) -> String {
    match access_point {
        "" => format!("WLAN, {signal} dBm"),
        ap => format!("WLAN über {ap}, {signal} dBm"),
    }
}

pub const CHOOSE_PORT: &str = "Port wählen";

/// the button taking the port the cable is in as the Anschluss
pub fn take_uplink(port: &str) -> String {
    format!("{port} übernehmen")
}

/// a device moved to another room, in the preview
pub fn room_moved(device: &str, room: &str, before: Option<&str>) -> String {
    match before {
        Some(before) => format!("{device} steht neu in {room} (vorher {before})"),
        None => format!("{device} steht neu in {room}"),
    }
}

/// A port of a device, by name.
pub struct PortAt<'a> {
    pub port: &'a str,
    pub device: &'a str,
}

/// a cable a change adds, between two ports
pub fn cable_added(a: PortAt, b: PortAt) -> String {
    format!(
        "Neues Kabel: {} {} – {} {}",
        a.device, a.port, b.device, b.port
    )
}

/// a cable a change takes away (the far end may be unknown)
pub fn cable_removed(end: PortAt, far: Option<PortAt>) -> String {
    match far {
        Some(far) => format!(
            "Kabel weg: {} {} – {} {}",
            end.device, end.port, far.device, far.port
        ),
        None => format!("Kabel weg an {} {}", end.device, end.port),
    }
}

pub const LINK_HINT: &str = "Steck das Kabel um, wie es hier steht. Die Geräte bekommen dafür ein Transfernetz; bis sie die Änderung haben, kann das Gerät kurz weg sein.";
pub const TAKE_HINT: &str =
    "Das Kabel steckt schon so. Bis die Geräte die Änderung haben, kann das Gerät kurz weg sein.";
pub const UNLINK_HINT: &str = "Das Gerät bleibt deins, ist danach aber nicht mehr angebunden und bekommt keine Änderungen mehr.";

/// devices unreachable until the devices have the change
pub fn cut_off(devices: &[&str]) -> String {
    format!(
        "Bis die Geräte die Änderung haben, nicht erreichbar: {}",
        devices.join(", ")
    )
}

pub const PORT_CLIENTS_MOVE: &str =
    "Geräte daran verlieren kurz die Verbindung und bekommen danach eine Adresse im neuen Netz.";
pub const PORT_CLIENTS_CUT: &str = "Geräte daran haben danach kein Netz mehr.";

/// the preview: a port or socket gets another network
pub fn port_network_change(port: &str, before: &str, network: &str) -> String {
    format!("{port}: {network} statt {before}")
}

pub const CLIENTS_DISCONNECT: &str =
    "Verbundene Geräte verlieren das WLAN, bis es wieder eingeschaltet ist.";
pub const CLIENTS_RECONNECT: &str = "Verbundene Geräte verlieren kurz das WLAN und brauchen danach den neuen Namen oder das \
     neue Passwort.";

/// the preview: the WLAN gets another name
pub fn ssid_change(before: &str, ssid: &str) -> String {
    format!("WLAN {before} heisst neu {ssid}")
}

/// the preview: no access point sends the WLAN any more
pub fn wlan_off(ssid: &str) -> String {
    format!("WLAN {ssid} wird ausgeschaltet")
}

/// the preview: the access points send the WLAN again
pub fn wlan_on(ssid: &str) -> String {
    format!("WLAN {ssid} wird eingeschaltet")
}

/// the preview: the WLAN gets another password
pub fn psk_change(ssid: &str) -> String {
    format!("WLAN {ssid} bekommt ein neues Passwort")
}

/// the preview of a change no device's configuration follows (a switch without RouterOS)
pub const NETBOX_ONLY: &str = "Das ändert nur NetBox, an keinem Gerät die Konfiguration";

/// the preview: the devices whose configuration changes
pub fn devices_change(devices: &[&str]) -> String {
    format!("Das ändert die Konfiguration von {}", devices.join(", "))
}

/// a NetBox location with the ones it is in, the outermost first: „2. OG › Wohnung Meier“
pub fn location_path(names: &[String]) -> String {
    names.join(" › ")
}

/// a socket in the flat (a front port in NetBox)
pub const SOCKET: &str = "Dose";

/// a socket by the wall it is in (a NetBox device, named after the room) and its place there
pub fn socket_name(device: &str, name: &str) -> String {
    format!("{device} {name}")
}

/// a socket with a label of its own: which one it is
pub fn socket_at(device: &str, name: &str) -> String {
    format!("Dose {device} {name}")
}

/// a port of another device given to the resident (a switch of the operator)
pub fn port_of(port: &str, device: &str) -> String {
    format!("Port {port} an {device}")
}

/// the device at the far end of the cable and its port
pub fn far_port(port: &str) -> String {
    format!("an {port}")
}

/// a network by its name, with the SSID of its WLAN behind it where that differs
pub fn network_title(name: &str, ssid: Option<&str>) -> String {
    match ssid {
        Some(ssid) if ssid != name => format!("{name} ({ssid})"),
        _ => name.into(),
    }
}

// drafts: changes collected until they are saved together

/// the label of an element with a draft
pub const UNSAVED: &str = "ungespeichert";
pub const RESET: &str = "Zurücksetzen";

/// the bar above the tabs while there are drafts
pub fn unsaved_changes(count: usize) -> String {
    match count {
        1 => "1 ungespeicherte Änderung".into(),
        count => format!("{count} ungespeicherte Änderungen"),
    }
}
pub const UNSAVED_HINT: &str = "Erst nach dem Speichern wird es ausgerollt.";
pub const PREVIEW_AND_SAVE: &str = "Vorschau und speichern";
pub const DISCARD_ALL: &str = "Alle verwerfen";

// a row of „Dosen und Geräte“

pub const NETWORK: &str = "Netz";
pub const OR_CONNECT_DEVICE: &str = "Oder Gerät anschliessen";
pub const CHOOSE_DEVICE: &str = "Gerät wählen";
/// a device a draft links to this port row
pub fn comes_here(device: &str) -> String {
    format!("{device} kommt hierher")
}
pub const REMOVE_DEVICE: &str = "Gerät entfernen";
pub const NOT_CONNECTED: &str = "Nicht angeschlossen";
pub const NOT_CONNECTED_HINT: &str = "Wähle rechts beim Gerät, wo du es anschliesst.";
/// a device standing in another room than the socket its cable comes from
pub fn patched_from(socket: &str) -> String {
    format!("Patchkabel von {socket}")
}
/// a device standing in another room than the port its cable comes from
pub fn cable_from(port: &str) -> String {
    format!("Kabel von {port}")
}
pub const PATCHED_HINT: &str = "In einem anderen Raum; das Kabel führt hierher.";
/// at the socket: its device stands in another room, shown there
pub fn see_room(room: &str) -> String {
    format!("Steht in {room}, siehe dort.")
}
/// a device linked to a port that isn't the resident's (the operator's router)
pub const OPERATOR_PORT: &str = "Port des Betreibers";
pub const CONNECTED_TO: &str = "Angeschlossen an";
pub const UPLINK: &str = "Anschluss";
pub const UPLINK_HINT: &str = "Der Port des Geräts, in dem das Kabel steckt.";
/// the router sees the device's cable in another port than NetBox's Anschluss
pub fn uplink_misplugged(seen: &str, uplink: &str) -> String {
    format!("Das Kabel steckt an {seen}, nicht an {uplink}")
}
pub fn uplink_misplugged_hint(seen: &str, uplink: &str) -> String {
    format!("Übernimm {seen} als Anschluss, oder steck das Kabel an {uplink} um.")
}
/// the room action at a device standing in its socket's room
pub const CHANGE_ROOM: &str = "Raum ändern";
/// the room action at a device standing elsewhere
pub fn in_room(room: &str) -> String {
    format!("in {room}")
}
/// the room dialog
pub fn room_title(device: &str) -> String {
    format!("Wo steht {device}?")
}
pub const ROOM_HINT: &str =
    "Nur nötig, wenn ein Patchkabel von der Dose in einen anderen Raum führt.";
pub const ROOM: &str = "Raum";
pub const APPLY: &str = "Übernehmen";
pub const CANCEL: &str = "Abbrechen";

// the preview of the drafts

pub const PREVIEW_TITLE: &str = "Das ändert sich";
pub const PREVIEW_DESCRIPTION: &str = "Noch nichts gespeichert.";
pub const KEEP_EDITING: &str = "Weiter bearbeiten";

// a network's card

pub fn network_card(title: &str) -> String {
    format!("Netz {title}")
}
/// the WLAN's switch
pub fn wlan_switch(ssid: &str) -> String {
    format!("WLAN „{ssid}“")
}
pub const WLAN_OFF_HINT: &str = "Aus: Das Netz gibt es nur per Kabel.";
pub const CHANGE: &str = "Ändern";
pub const SSID_HINT: &str =
    "1 bis 32 Zeichen (Umlaute zählen doppelt), ohne Leerzeichen am Anfang und Ende";
/// how many access points send the WLAN
pub fn access_points(count: i32) -> String {
    match count {
        1 => "1 Access Point".into(),
        count => format!("{count} Access Points"),
    }
}

// the admins' support view

pub const SUPPORT_TITLE: &str = "Anwohner";
pub const SUPPORT_HINT: &str =
    "Support-Ansicht: Du siehst das Netz so, wie es dieser Anwohner sieht.";
pub const PICK_RESIDENT: &str = "Anwohner";
pub const NO_RESIDENTS: &str = "NetBox nennt keinen Owner an einem Gerät, Port oder VLAN.";
