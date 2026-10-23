use crate::components::interfaces::rows::Task;

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
pub fn tags(ospf: bool, dhcp: bool, dhcp_client: bool, nat: bool) -> String {
    [
        (ospf, "ospf"),
        (dhcp, "dhcp"),
        (dhcp_client, "dhcp-client"),
        (nat, "nat"),
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
