use crate::components::{
    interfaces::rows::{Kind, Task},
    netbox_helper::HelperKind,
};

pub const TITLE: &str = "Interfaces";
pub const ALL: &str = "Alle";
pub const ONLY_FINDINGS: &str = "nur mit Befund";
pub const NONE_WITH_FINDINGS: &str = "Kein Interface mit Befund.";
pub const NO_FINDINGS: &str = "Keine Befunde.";
pub const OTHER_FINDINGS: &str = "Weitere Befunde zum Gerät";
pub const NO_LOOPBACK: &str = "Keine Loopback";
pub const NO_LOOPBACK_TEXT: &str = "Das Gerät hat in NetBox kein Interface „lo“.";
pub const OPEN_IN_NETBOX: &str = "In NetBox öffnen";
pub const NO_CABLE: &str = "kein Kabel";
pub const NO_GATEWAY: &str = "kein L3-Übergang";
pub const PORTS: &str = "Ports";
pub const ADDRESSES: &str = "Adressen";
pub const TAGGED: &str = "tagged";
pub const NONE: &str = "–";

// columns
pub const PORT: &str = "Port";
pub const ADDRESS: &str = "Adresse";
pub const NAT: &str = "NAT";
pub const FAR_END: &str = "Gegenstelle";
pub const NETWORK: &str = "Netz";
pub const MTU: &str = "MTU";
pub const PATH: &str = "Strecke";
pub const VLAN: &str = "VLAN";
pub const IPV4: &str = "IPv4";
pub const IPV6: &str = "IPv6";
pub const DHCP: &str = "DHCP";
pub const NETBOX_HAS: &str = "NetBox hat";
pub const STATE: &str = "Zustand";
pub const NETBOX: &str = "NetBox";

/// a filter button
pub fn task(task: Task) -> &'static str {
    match task {
        Task::Loopback => "Loopback",
        Task::Uplink => "Uplink",
        Task::Direct => "Direkt",
        Task::RadioLink => "Richtfunk",
        Task::Customer => "Kunden",
        Task::Unknown => "Unbekannt",
    }
}

/// the title of a task's section
pub fn section(task: Task) -> &'static str {
    match task {
        Task::Loopback => "Loopback",
        Task::Uplink => "Uplinks",
        Task::Direct => "Direktverbindungen",
        Task::RadioLink => "Richtfunk",
        Task::Customer => "Kunden-Anschlüsse",
        Task::Unknown => "Unbekannt",
    }
}

pub const IN_ORDER: &str = "in Ordnung";
pub fn errors(count: usize) -> String {
    format!("{count} Fehler")
}
pub fn warnings(count: usize) -> String {
    match count {
        1 => "1 Warnung".to_string(),
        n => format!("{n} Warnungen"),
    }
}
pub fn hints(count: usize) -> String {
    match count {
        1 => "1 Hinweis".to_string(),
        n => format!("{n} Hinweise"),
    }
}
pub fn free_ports(count: usize) -> String {
    match count {
        1 => "1 freier Port".to_string(),
        n => format!("{n} freie Ports"),
    }
}
/// a customer port without an address here: where its VLAN has one
pub const L3_ON: &str = "L3 auf ";
/// a tag of an interface
pub fn tags(ospf: bool, dhcp: bool, dhcp_client: bool, nat: bool, internet: bool) -> String {
    [
        (ospf, "ospf"),
        (dhcp, "dhcp"),
        (dhcp_client, "dhcp-client"),
        (nat, "nat"),
        (internet, "internet"),
    ]
    .iter()
    .filter(|(set, _)| *set)
    .map(|(_, tag)| *tag)
    .collect::<Vec<_>>()
    .join(", ")
}
pub const TAGS: &str = "Tags";
pub const LABEL: &str = "Label";
pub const DHCP_CLIENT: &str = "DHCP-Client";
pub const ACTIONS: &str = "Aktionen";
/// the bridge a port is in
pub const BRIDGE: &str = "Bridge";
pub const CHANGED: &str = "in NetBox geändert, noch nicht angewendet";
pub const CHANGED_TITLE: &str = "In NetBox geändert";
pub const CHANGED_TEXT: &str = "Das Gerät hat die Änderung noch nicht.";
pub const UNDO: &str = "Rückgängig";
pub const APPLY: &str = "Anwenden";

/// the button of a static lease only the device has
pub fn lease_takeover(address: &str) -> String {
    format!("Lease {address} übernehmen")
}
/// the button of an address only the device has
pub fn address_takeover(address: &str) -> String {
    format!("{address} übernehmen")
}
/// a button or menu entry of a row
pub fn action(kind: Kind, helper: HelperKind) -> String {
    match kind {
        Kind::Suggestion if helper == HelperKind::CableTakeover => helper.to_string(),
        Kind::Remove if helper == HelperKind::BridgeLeave => helper.to_string(),
        // a device to cable here: the same from what the router sees and from a free port
        Kind::Suggestion | Kind::AssignRole if helper == HelperKind::NewDevice => {
            "Neues Gerät anschliessen".to_string()
        }
        Kind::Suggestion => "Lösungsvorschlag".to_string(),
        Kind::Remove => "Verbindung entfernen".to_string(),
        Kind::Edit => helper.to_string(),
        Kind::AssignRole => format!(
            "Als {}",
            match helper {
                HelperKind::CustomerNet => "Kunden-Anschluss",
                HelperKind::Connection => "Direktverbindung",
                HelperKind::RadioBridge => "Richtfunk",
                other => return format!("Als {other}"),
            }
        ),
    }
}

/// the title of a helper's modal: the action's label, at its row's interface (`None`: the
/// loopback's, without row)
pub fn flow_title(what: String, interface: Option<&str>) -> String {
    match interface {
        Some(interface) => format!("{what}: {interface}"),
        None => what,
    }
}
pub const CHECK_LIVE: &str = "Live prüfen";
pub const LIVE_PENDING: &str = "Live-Prüfung läuft …";
pub const LIVE_UNCHECKED: &str = "Live nicht geprüft: Gerät nicht erreichbar.";
pub const LIVE_UNCHECKED_TITLE: &str = "Live-Prüfungen nicht möglich, nur NetBox geprüft";
pub const WITHOUT_DEVICE: &str = "ohne Gerät";
pub const SEES: &str = "Gerät sieht";
pub const SEES_NOBODY: &str = "niemanden";
pub const OSPF: &str = "OSPF";
pub const OSPF_MISSING: &str = "kein OSPF-Nachbar:";
pub const BRIDGES: &str = "Brückengeräte";
pub const DEVICE_ONLY: &str = "Nur auf dem Gerät";
/// an address only the device has: the next apply removes it
pub fn device_only(address: &str, interface: &str, disabled: bool) -> String {
    let disabled = if disabled { ", deaktiviert" } else { "" };
    format!("{address} an {interface}{disabled}: das nächste Anwenden entfernt sie")
}
pub const DEVICE_ONLY_ELSEWHERE: &str = "– NetBox hat sie an";
pub const DEVICE_ONLY_UNASSIGNED: &str = "– NetBox hat sie an keinem Interface";
pub const LEASES_DEVICE_ONLY: &str = "Leases nur auf dem Gerät";
pub const POE: &str = "PoE";
pub const POE_REMOVE: &str = "PoE-Eintrag entfernen";
pub const POE_COMPLETE: &str = "PoE vervollständigen";

/// Where the device contradicts NetBox's PoE mode of a port (`PoeProblem`).
pub enum PoeMismatch {
    NoPoeOut,
    NoLoad,
    PdMissing,
    FeedsInstead,
    Other,
}
/// `status`: `poe-out-status` as the device reports it
pub fn poe_mismatch(port: &str, problem: PoeMismatch, status: Option<&str>) -> String {
    let status = status.map(|s| format!(" ({s})")).unwrap_or_default();
    match problem {
        PoeMismatch::NoPoeOut => {
            format!("{port}: in NetBox speisend, das Gerät hat an diesem Port kein PoE-Out")
        }
        PoeMismatch::NoLoad => format!(
            "{port}: in NetBox speisend, aber am anderen Ende bezieht nichts Strom{status}: das \
             Gerät dort kann kein PoE, hat ein eigenes Netzteil oder ist aus"
        ),
        PoeMismatch::PdMissing => format!(
            "{port}: speist{status}, in NetBox ist die Gegenseite aber nicht als gespeist erfasst"
        ),
        PoeMismatch::FeedsInstead => {
            format!("{port}: in NetBox gespeist, das Gerät speist an diesem Port aber selbst{status}")
        }
        PoeMismatch::Other => format!("{port}: PoE stimmt nicht mit NetBox überein{status}"),
    }
}
/// a static lease only the device has: the next apply removes it
pub fn device_only_lease(address: &str, client: &str, name: Option<&str>, disabled: bool) -> String {
    let name = name.map(|name| format!(" „{name}“")).unwrap_or_default();
    let disabled = if disabled { ", deaktiviert" } else { "" };
    format!("{address} für {client}{name}{disabled}: das nächste Anwenden entfernt ihn")
}
/// what NetBox can't hold of a lease
pub fn lease_settings_lost(settings: &[String]) -> String {
    format!("– übernommen gehen verloren: {}", settings.join(", "))
}
pub const LEASE_BY_CLIENT_ID: &str = "– per Client-ID, NetBox kann ihn nicht halten";
pub const OTHER_LEASES: &str = "Leases nur auf dem Gerät, von Servern ohne Zeile";
pub const OTHER_ADDRESSES: &str = "Adressen nur auf dem Gerät, an Interfaces ohne Zeile";
pub const BRIDGE_NO_ADDRESS: &str = "ohne Primary IPv4, nicht gefragt";
/// an OSPF neighbor's state in its instance (`default-v2`)
pub fn ospf_state(instance: &str, state: &str) -> String {
    format!("{state} ({instance})")
}
/// whether a bridge device answered a ping at `address`
pub fn bridge_answer(address: &str, answers: bool) -> String {
    if answers {
        format!("antwortet an {address}")
    } else {
        format!("antwortet nicht an {address}")
    }
}
