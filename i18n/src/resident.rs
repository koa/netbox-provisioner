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
pub fn wlan(ssid: &str, access_points: i32) -> String {
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
