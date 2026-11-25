//! Scope: The texts of the residents' view and the admins' page of it; nothing else.

/// the title of a resident's own page
pub const MY_NETWORK: &str = "Mein Netz";
pub const INTRO: &str = "Deine Dosen und Geräte, und welches deiner Netze darauf läuft. Änderungen \
    sammelst du und speicherst sie zusammen.";
pub const TAB_PORTS: &str = "Dosen und Geräte";
pub const TAB_NETWORKS: &str = "Netze";
pub const TAB_CHANGES: &str = "Änderungen";
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

/// the table of the devices behind a port, for screen readers
pub fn devices_at(port: &str) -> String {
    format!("Geräte an {port}")
}

/// the tooltip on a device's address: its MAC, and whether the device made it up (a phone's
/// private address, changing per network)
pub fn mac_title(mac: &str, random: bool) -> String {
    match random {
        true => format!("MAC {mac} (zufällige Adresse)"),
        false => format!("MAC {mac}"),
    }
}

/// devices in a network: routers or controllers that didn't answer, so their devices are missing
pub fn devices_unreachable(devices: &[&str]) -> String {
    format!(
        "Nicht erreicht: {}; die Geräte dort fehlen in der Liste",
        devices.join(", ")
    )
}

/// devices in a network: one connected over WLAN, at an access point (empty: not known)
/// `band`: [`wlan_band`]'s, empty where not known
pub fn wlan_connection(access_point: &str, band: &str, signal: i32) -> String {
    let band = match band {
        "" => String::new(),
        band => format!("{band}, "),
    };
    match access_point {
        "" => format!("WLAN, {band}{signal} dBm"),
        ap => format!("WLAN über {ap}, {band}{signal} dBm"),
    }
}
/// a WLAN band: `ghz` like „2,4“, with the IEEE 802.11 standard („ax“) where known
pub fn wlan_band(ghz: &str, standard: Option<&str>) -> String {
    match standard {
        Some(standard) => format!("{ghz} GHz ({standard})"),
        None => format!("{ghz} GHz"),
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
pub const NOT_HERE: &str = "doch nicht";
/// a port without RouterOS at it (of a socket: at the switch behind it) takes no device;
/// `socket`: a socket's
pub fn no_device_here(socket: bool) -> &'static str {
    match socket {
        true => {
            "Hier kann kein Gerät angeschlossen werden: hinter dieser Dose steht kein Switch, \
             den der Provisioner einrichten kann."
        }
        false => {
            "Hier kann kein Gerät angeschlossen werden: dieses Gerät kann der Provisioner nicht \
             einrichten."
        }
    }
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

/// the button opening a WLAN's QR code
pub const QR_BUTTON: &str = "QR-Code zum Verbinden";
pub fn qr_title(ssid: &str) -> String {
    format!("„{ssid}“ verbinden")
}
pub const QR_HINT: &str = "Mit der Kamera des Telefons scannen.";
/// the QR code's alternative text
pub fn qr_label(ssid: &str) -> String {
    format!("QR-Code für das WLAN {ssid}")
}
pub const QR_UNSAVED: &str =
    "Noch nicht gespeichert: Der Code gilt erst, wenn die Änderung auf den Geräten ist.";
pub const WLAN: &str = "WLAN";
pub const DONE: &str = "Fertig";

// the admins' support view

pub const SUPPORT_TITLE: &str = "Anwohner";
pub const SUPPORT_HINT: &str =
    "Support-Ansicht: Du siehst das Netz so, wie es dieser Anwohner sieht.";
pub const PICK_RESIDENT: &str = "Anwohner";
pub const NO_RESIDENTS: &str = "NetBox nennt keinen Owner an einem Gerät, Port oder VLAN.";

pub const NO_CHANGES: &str = "Noch keine gespeicherten Änderungen.";
pub const UNDO_CHANGE: &str = "Zurücknehmen";
pub const UNDO_GONE: &str = "Diese Änderung lässt sich nicht mehr zurücknehmen: Der Provisioner \
    kennt sie nicht mehr.";
pub const UNDO_AS_BEFORE: &str = "Es ist schon alles wie vor dieser Änderung.";
pub const READING_CHANGES: &str = "Lese die Änderungen …";
pub const CHANGES_HINT: &str = "Jede gespeicherte Änderung und wie weit sie auf den Geräten ist. \
    Die Geräte bekommen sie in der Regel innert weniger Minuten.";

/// The head of a saved change: when and by whom.
pub fn change_head(ago: &str, by: &str) -> String {
    format!("Gespeichert {ago} von {by}")
}

/// How many of its devices have a change.
pub fn change_progress(done: usize, of: usize) -> String {
    format!("{done} von {of} Geräten haben sie")
}

/// How far a change has reached a device, for its resident.
pub fn rollout_state(state: crate::auto_rollout::RolloutState) -> &'static str {
    use crate::auto_rollout::RolloutState;
    match state {
        RolloutState::Pending => "wird ausgerollt",
        RolloutState::RolledOut => "ausgerollt",
        RolloutState::Waiting => "wartet",
        RolloutState::WaitingForAdmin => "wartet auf einen Admin",
        RolloutState::Deferred => "wartet, Gerät nicht erreichbar",
        RolloutState::Failed => "nicht ausgerollt, ein Admin schaut es an",
        RolloutState::Manual => "wird von einem Admin angewendet",
    }
}

/// The one label of an element (port, device, network): unsaved, its rollout, else what is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLabel {
    Ok,
    Off,
    Rolling,
    Waiting,
    Failed,
    Down,
    Limited,
}

pub fn status_label(label: StatusLabel) -> &'static str {
    match label {
        StatusLabel::Ok => "Ok",
        StatusLabel::Off => "aus",
        StatusLabel::Rolling => "wird ausgerollt",
        StatusLabel::Waiting => "wartet",
        StatusLabel::Failed => "nicht ausgerollt",
        StatusLabel::Down => "gestört",
        StatusLabel::Limited => "Problem",
    }
}

/// an element whose change some of its devices have
pub fn rolling_out(done: usize, of: usize) -> String {
    format!("Wird ausgerollt: {done} von {of} Geräten.")
}

/// an element whose change failed on `devices`
pub fn not_rolled_out(devices: &str) -> String {
    format!(
        "Auf {devices} nicht ausgerollt; dort gilt noch das Alte. Bleibt als Abweichung, bis es \
         klappt."
    )
}

/// an element whose change waits for `devices`, not reached
pub fn waiting_for(devices: &str) -> String {
    format!("Wartet auf {devices} (nicht erreichbar); wird nachgeholt, sobald es wieder da ist.")
}

/// an element whose change waits for an admin, at `devices`, with why
pub fn waiting_for_admin(devices: &str, reasons: &str) -> String {
    match reasons.is_empty() {
        true => format!("Wartet bei {devices} auf einen Admin."),
        false => format!("Wartet bei {devices} auf einen Admin: {reasons}."),
    }
}

/// why a change waits at a device that changes a radio of a radio link (`RadioLink`): a
/// resident's changes are not meant to touch one, so the resident learns it needs support
pub const RADIO_LINK_HOLD: &str = "eine komplexe Anpassung an einer Richtfunk-Verbindung, die \
                                   nicht automatisch ausgerollt werden kann; der Support \
                                   übernimmt sie";

/// an element whose change waits for the rollout's next round (another device holds it back,
/// or the rollout pauses)
pub const WAITING_FOR_ROUND: &str = "Wartet auf die nächste Runde des Ausrollens.";

/// a device getting a change now
pub const DEVICE_ROLLING: &str = "Bekommt gerade eine gespeicherte Änderung.";
/// a device on which a change failed
pub const DEVICE_FAILED: &str = "Hat noch den alten Stand; ein Admin schaut es an.";
/// a device not reached (or its radio partner not), its change waiting
pub const DEVICE_WAITING: &str = "Nicht erreicht; bekommt die Änderung danach.";

/// a device the routers don't reach
pub const UNREACHABLE: &str = "Nicht erreichbar. Abgeschaltet, ohne Strom oder Kabel gezogen?";

/// a device the routers don't reach behind another one they don't reach either
pub fn unreachable_behind(other: &str) -> String {
    format!("Nicht erreichbar, weil {other} nicht erreichbar ist.")
}

/// a port whose switch the routers don't reach
pub fn switch_unreachable(switch: &str) -> String {
    format!("Nicht erreichbar: {switch} antwortet nicht.")
}

/// a port whose own device the routers don't reach; `link`: the switch port has one
pub fn device_silent(device: &str, link: bool) -> String {
    match link {
        true => format!("Das Kabel hat Link, aber {device} antwortet nicht."),
        false => format!("{device} antwortet nicht."),
    }
}

/// a port with a cable in NetBox whose switch port has no link; `socket`: a socket's
pub fn no_link(socket: bool) -> &'static str {
    match socket {
        true => "Kein Link. Steckt das Kabel an der Dose und am Gerät?",
        false => "Kein Link. Steckt das Kabel an beiden Enden?",
    }
}

/// a device whose port has no link
pub fn no_link_at(port: &str) -> String {
    format!("Nicht erreichbar: {port} hat keinen Link.")
}

/// a WLAN on some of its access points only
pub fn wlan_limited(on: usize, of: usize, unreached: &str) -> String {
    format!("WLAN nur auf {on} von {of} Access Points; {unreached} nicht erreichbar.")
}

/// a WLAN none of whose access points is reached
pub const WLAN_DOWN: &str = "Kein Access Point des WLANs ist erreichbar.";

/// the summary on top: nothing wrong
pub const ALL_OK: &str = "Alles ok";
/// the summary on top: the state is read again every 15 s
pub const ALL_OK_HINT: &str = "Wird alle 15 Sekunden neu geprüft.";
/// the summary on top: a change not through yet, and how it stands
pub fn summary_change(head: &str, stands: &str) -> String {
    format!("{head}: {stands}")
}
/// the summary on top: a device the routers don't reach
pub fn summary_unreachable(device: &str) -> String {
    format!("{device} ist nicht erreichbar.")
}
/// the summary on top: a port with a cable in NetBox and no link
pub fn summary_no_link(port: &str) -> String {
    format!("{port} hat keinen Link: steckt das Kabel?")
}
/// the summary on top: a WLAN on some access points only
pub fn summary_wlan(ssid: &str, on: usize, of: usize) -> String {
    match on {
        0 => format!("WLAN „{ssid}“ auf keinem Access Point."),
        _ => format!("WLAN „{ssid}“ nur auf {on} von {of} Access Points."),
    }
}
/// the summary on top: its title where something isn't ok
pub const NOT_ALL_OK: &str = "Nicht alles ok";
