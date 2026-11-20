//! Scope: The texts of the residents' view and the admins' page of it; nothing else.

/// the title of a resident's own page
pub const MY_NETWORK: &str = "Mein Netz";
pub const INTRO: &str = "Deine Dosen und Geräte, und welches deiner Netze darauf läuft.";
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
pub const NO_SUBNET: &str = "Keine Adressen in NetBox";
pub const NOBODY: &str = "Du bist angemeldet, aber weder Admin noch Anwohner. Ein Admin trägt dich bei deinem Haushalt ein.";

/// a port of the resident's own device
pub fn own_port(port: &str, device: &str) -> String {
    format!("Port {port} von {device}")
}

pub const CHANGE_WLAN: &str = "WLAN ändern";
pub const SSID: &str = "WLAN-Name";
pub const PSK: &str = "Passwort";
pub const SSID_PLACEHOLDER: &str = "Name des WLANs";
pub const PSK_PLACEHOLDER: &str = "Neues Passwort";
pub const ENABLED: &str = "Eingeschaltet";
pub const PSK_HINT: &str = "12 bis 63 Zeichen, ohne Umlaute";
pub const SHOW_PSK: &str = "Zeigen";
pub const HIDE_PSK: &str = "Verbergen";
pub const RANDOM_PSK: &str = "Zufällig erzeugen";
pub const PREVIEW: &str = "Vorschau";
pub const DISCARD: &str = "Verwerfen";
pub const PLANNING: &str = "Vorschau wird berechnet …";
pub const SAVE: &str = "Speichern";
pub const BACK: &str = "Zurück";
pub const CLOSE: &str = "Schliessen";
pub const SAVING: &str = "Wird gespeichert …";
pub const SAVED: &str = "In NetBox gespeichert";
pub const SAVED_HINT: &str = "Auf die Geräte kommt die Änderung mit dem Ausrollen; wie weit das \
    ist, zeigt diese Seite noch nicht.";
pub const NOT_SAVED: &str = "Nicht gespeichert";
pub const SHOW_DEVICES: &str = "Geräte im Netz";
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

pub const CONNECT_DEVICE: &str = "Gerät anschliessen";
pub const CHOOSE_DEVICE: &str = "Gerät wählen";

/// the button taking one of the resident's devices off its link
pub fn disconnect_device(device: &str) -> String {
    format!("{device} abhängen")
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
pub const UNLINK_HINT: &str = "Das Gerät bleibt deins, ist danach aber nicht mehr angebunden und bekommt keine Änderungen mehr.";

/// devices unreachable until the devices have the change
pub fn cut_off(devices: &[&str]) -> String {
    format!(
        "Bis die Geräte die Änderung haben, nicht erreichbar: {}",
        devices.join(", ")
    )
}

pub const CHANGE_NETWORK: &str = "Netz ändern";
pub const CHOOSE_NETWORK: &str = "Netz wählen";
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

pub fn vid(vid: i32) -> String {
    format!("VLAN {vid}")
}

/// a network by its name, with the SSID of its WLAN behind it where that differs
pub fn network_title(name: &str, ssid: Option<&str>) -> String {
    match ssid {
        Some(ssid) if ssid != name => format!("{name} ({ssid})"),
        _ => name.into(),
    }
}

/// a WLAN and how many access points send it
pub fn wlan(ssid: &str, access_points: i32, enabled: bool) -> String {
    if !enabled {
        return format!("WLAN {ssid}: ausgeschaltet");
    }
    match access_points {
        1 => format!("WLAN {ssid}: 1 Access Point"),
        count => format!("WLAN {ssid}: {count} Access Points"),
    }
}

// the admins' support view

pub const SUPPORT_TITLE: &str = "Anwohner";
pub const SUPPORT_HINT: &str =
    "Support-Ansicht: Du siehst das Netz so, wie es dieser Anwohner sieht.";
pub const PICK_RESIDENT: &str = "Anwohner";
pub const NO_RESIDENTS: &str = "NetBox nennt keinen Owner an einem Gerät, Port oder VLAN.";
