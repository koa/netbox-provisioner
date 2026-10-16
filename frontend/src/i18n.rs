//! Every text the UI shows, in German, by the page or component showing it. Components take
//! their texts from here and write none inline; the backend sends data, which is worded here
//! (job events, warnings). A second language replaces these functions, or makes them choose
//! by language, without touching the components. Names RouterOS or NetBox use themselves
//! (`RouterOS`, `Board`, `long-term`) stay as they are.

/// Texts used in several places.
pub mod common {
    /// Why the provisioner needs a feature of the device mode (`DeviceModeReason`).
    pub enum DeviceModeNeed<'a> {
        ApplyFallback,
        ManagementProxy,
        UpdateFallback,
        Other(&'a str),
    }

    /// a feature of the device mode and why it is needed, e.g. `partitions (für …)`
    pub fn device_mode_feature(feature: &str, need: DeviceModeNeed) -> String {
        let need = match need {
            DeviceModeNeed::ApplyFallback => "für den Fallback beim Anwenden",
            DeviceModeNeed::ManagementProxy => "für den Management-Proxy",
            DeviceModeNeed::UpdateFallback => "für den Fallback bei Updates: zwei Partitionen",
            DeviceModeNeed::Other(other) => other,
        };
        format!("{feature} ({need})")
    }

    pub const CANCEL: &str = "Abbrechen";
    /// a list without entries
    pub const NONE: &str = "keine";

    /// `items` joined, or [`NONE`].
    pub fn list(items: &[String]) -> String {
        if items.is_empty() {
            NONE.into()
        } else {
            items.join(", ")
        }
    }
}

/// `crate::error::FrontendError` as the user reads it.
pub mod error {
    use crate::error::ServerProblem;

    pub const BROWSER: &str = "Fehler im Browser";
    pub const UNREADABLE: &str = "Antwort des Servers nicht lesbar";
    pub const SERVER: &str = "Fehler vom Server";
    pub const UNREACHABLE: &str = "Server nicht erreichbar";
    pub const HEADER: &str = "Ungültiger HTTP-Header";
    pub const MISSING_DATA: &str = "Keine Daten";
    pub const MISSING_DATA_TEXT: &str = "Der Server hat keine Daten geliefert.";

    /// where in the response a GraphQL error is (`path`)
    pub fn at(message: &str, path: &str) -> String {
        format!("{message} bei {path}")
    }

    /// An error the backend reports, by its `code` (`backend::error_info`); a code not worded
    /// here yet shows the backend's English `message`.
    pub fn server(problem: &ServerProblem) -> String {
        let p = |name| problem.param(name);
        match problem.code.as_str() {
            "DEVICE_API" => format!("Zugriff auf das Gerät fehlgeschlagen: {}", p("cause")),
            "MISSING_CREDENTIALS" => "Keine Zugangsdaten für das Gerät".into(),
            "APPLY_NOT_ALLOWED" => "Schreiben auf Geräte ist gesperrt (allow_apply)".into(),
            "UNKNOWN_DEVICE" => format!("Gerät {} gibt es in NetBox nicht", p("id")),
            "TIMEOUT" => format!("Keine Antwort von {}", p("address")),
            "NO_ADDRESS" => format!("{} hat in NetBox keine Adresse", p("device")),
            "PROXY" => format!(
                "Keine Verbindung über den Management-Proxy {}: {}",
                p("address"),
                p("cause")
            ),
            "NO_BOOTSTRAP_PATH" => match p("obstacle") {
                "NO_IPV4_PORT" => format!("{} hat in NetBox keinen Port mit IPv4-Adresse", p("device")),
                _ => "Von keinem Port mit IPv4-Adresse führt ein Kabel zu einem erreichbaren Gerät \
                      im selben Netz"
                    .into(),
            },
            "CONSOLE_LOGIN_REFUSED" => format!(
                "Anmeldung über mac-telnet an {} abgelehnt (Werkseinstellung und hinterlegte \
                 Zugangsdaten)",
                p("macs")
            ),
            "CONSOLE_NO_ANSWER" => format!(
                "Keine Antwort auf die Anmeldung über mac-telnet an {}",
                p("macs")
            ),
            "SETUP_COMMAND" => format!(
                "Ersteinrichtung fehlgeschlagen bei {}: {}",
                p("command"),
                p("cause")
            ),
            "CONSOLE_SESSION_ENDED" => format!(
                "Die mac-telnet-Sitzung endete vor {}",
                p("command")
            ),
            "DOWNLOAD" => format!("{} nicht geladen: {}", p("url"), p("cause")),
            "UNKNOWN_RELEASE" => format!(
                "{} nennt keine RouterOS-Version: {}",
                p("url"),
                p("answer")
            ),
            "MISSING_DEVICE_STATE" => format!("Das Gerät meldet {} nicht", p("field")),
            "UPLOAD_MISSING" => format!("{} fehlt nach dem Hochladen auf dem Gerät", p("file")),
            "UPLOAD_INCOMPLETE" => format!(
                "{} hat auf dem Gerät {} Bytes statt {}",
                p("file"),
                p("size"),
                p("expected")
            ),
            "UNEXPECTED_AFTER_REBOOT" => match p("firmware") {
                "" => format!(
                    "Nach dem Neustart läuft RouterOS {}, nicht was das Update erwartet",
                    p("version")
                ),
                firmware => format!(
                    "Nach dem Neustart läuft RouterOS {} mit Firmware {firmware}, nicht was das \
                     Update erwartet",
                    p("version")
                ),
            },
            "NOT_BACK_AFTER_REBOOT" => "Das Gerät ist nach dem Neustart nicht zurück".into(),
            "PANIC" => format!("Interner Fehler im Provisioner (bitte melden): {}", p("message")),
            "UPDATE_ROLLED_BACK" => format!(
                "Update zurückgenommen: Das Gerät war nach dem Update nicht erreichbar und läuft \
                 wieder mit RouterOS {} von {}",
                p("version"),
                p("partition")
            ),
            "OSPF_NEIGHBORS_MISSING" => format!(
                "Nach dem Update fehlen OSPF-Nachbarn, die vorher Full waren: {}. Zurück geht es \
                 auf der Geräteseite unter Partitionen",
                p("neighbors")
            ),
            "REFUSED" => format!("Das Gerät lehnt ab: {}", p("cause")),
            "CANNOT_REPARTITION" => format!(
                "Der Flash lässt sich nicht aufteilen: {} Partitionen, oder der Device-Mode erlaubt keine",
                p("count")
            ),
            "DEVICE_MODE_NOT_CONFIRMED" => {
                "Die Änderung des Device-Mode wurde am Gerät nicht rechtzeitig bestätigt".into()
            }
            "DEVICE_MODE_UNCHANGED" => format!(
                "Das Gerät läuft weiter ohne {} im Device-Mode",
                p("features")
            ),
            "MISSING_DEPENDENCIES" => format!(
                "Änderungen warten aufeinander oder verweisen auf Fehlendes: {}",
                p("cause")
            ),
            "BUSY" => format!("Auf {} läuft schon ein Auftrag", p("device")),
            "UNKNOWN_JOB" => format!("Auftrag {} gibt es nicht (mehr)", p("id")),
            "CANCELLED" => "Abgebrochen".into(),
            "PING" => format!("Ping fehlgeschlagen: {}", p("cause")),
            "NETBOX_WRITE_NOT_ALLOWED" => "Schreiben in NetBox ist gesperrt (allow_netbox_write, \
                netbox_write_token in der config.yaml)"
                .into(),
            "NETBOX" => format!("NetBox: {}", p("cause")),
            "PLAN_OUTDATED" => "NetBox hat sich seit der Vorschau geändert: bitte den Plan neu \
                ansehen"
                .into(),
            "NETBOX_CHANGED" => format!(
                "NetBox hat sich seit dem Plan geändert ({}): nichts weiter geschrieben",
                p("what")
            ),
            "NO_PREVIEW" | "PREVIEW" => {
                format!("Vorschau nicht möglich: {}", problem.message)
            }
            "NO_FREE_ADDRESS" => format!(
                "Im Pool {} ist keine IPv{}-Adresse mehr frei",
                p("pool"),
                p("version")
            ),
            "NO_ROUTE_CHECK" => format!(
                "Keine Adresse vergeben: ob sie im Netz schon belegt ist, lässt sich nicht \
                prüfen ({})",
                p("reason")
            ),
            "NOTHING_TO_UNDO" => format!("Auftrag {} hat nichts in NetBox geschrieben", p("job")),
            "NO_ROUTERBOARD" => "Das Gerät meldet kein RouterBOARD".into(),
            "NO_PORTS" => format!(
                "Das Board {} ist im Provisioner nicht beschrieben (hw_facts.rs): seine Ports sind unbekannt",
                p("model")
            ),
            "PORT_NOT_FOUND" => format!("Port {} aus NetBox gibt es auf dem Gerät nicht", p("port")),
            "MISSING_PACKAGE" => format!(
                "RouterOS-Paket {} fehlt (ein RouterOS-Update installiert es)",
                p("package")
            ),
            "NO_CONTROLLER" => format!(
                "{} ist AP einer Wireless LAN Group, deren Controller keine Loopback-Adresse hat",
                p("device")
            ),
            "MTU_MISMATCH" => format!(
                "{} hat MTU {}, das andere Ende des Kabels, {}, {}: OSPF braucht auf beiden dieselbe",
                p("port"),
                p("mtu"),
                p("farEnd"),
                p("farMtu")
            ),
            "MTU_NOT_SUPPORTED" => format!(
                "{} hat in NetBox eine MTU, die nur Ethernet-Ports und wlan-Radios bekommen",
                p("port")
            ),
            _ => problem.message.clone(),
        }
    }

    /// What a status the server answers instead of a result means: `401` and `403` come from
    /// the token check (`binary`: the token, or the user not in `auth_users`; the server log
    /// has the reason).
    pub fn refused(status: u16) -> String {
        match status {
            401 => "Nicht angemeldet: bitte neu anmelden".into(),
            403 => "Kein Zugriff: Anmeldung abgelehnt oder Benutzer nicht freigegeben".into(),
            status => format!("Server antwortet {status}"),
        }
    }
}

pub mod login {
    pub const AGAIN: &str = "Erneut anmelden";

    pub fn failed(failure: &str) -> String {
        format!("Anmeldung fehlgeschlagen: {failure}")
    }
}

/// The breadcrumb of dropdowns (`components::menu`).
pub mod menu {
    pub const NOT_LOADED: &str = "Nicht geladen";
    pub const OVERVIEW: &str = "Übersicht";
    pub const ALL_DEVICES: &str = "Alle Geräte";

    /// `name`: the site shown, `None` with `known` an id NetBox doesn't have, none without
    pub fn site(name: Option<&str>, known: bool) -> String {
        format!("Site: {}", name.unwrap_or(if known { "unbekannt" } else { "-" }))
    }

    pub fn device(name: Option<&str>, known: bool) -> String {
        format!("Gerät: {}", name.unwrap_or(if known { "unbekannt" } else { "-" }))
    }
}

pub mod pages {
    pub const NOT_FOUND: &str = "Seite nicht gefunden";

    pub fn device_count(count: usize) -> String {
        match count {
            1 => "1 Gerät".into(),
            n => format!("{n} Geräte"),
        }
    }

    /// a ping's round trip
    pub fn ping(nanos: u64) -> String {
        format!("{:.2} ms", nanos as f32 / 1_000_000.0)
    }
}

/// What NetBox lacks or what doesn't fit together (`components::findings`). The objects come in
/// as links to NetBox.
pub mod findings {
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
    pub fn port_like_name(interface: Html) -> Html {
        html!(<>{interface}{" ist ein physisches Interface, heisst aber nicht wie ein RouterOS-Port: Der Generator nimmt es nicht als Port."}</>)
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
}

/// What the devices see at their ports against NetBox's cables (`components::neighbor_check`).
pub mod neighbors {
    use yew::{Html, html};

    pub const TITLE: &str = "Vergleich mit den Geräten";
    pub const INTRO: &str = "Liest auf jedem Gerät, wen es an seinen Ports sieht (/ip/neighbor), und vergleicht das mit den Kabeln in NetBox. Auf den Geräten ändert sich nichts.";
    pub const START: &str = "Nachbarn lesen";
    pub const AGAIN: &str = "Nochmals lesen";
    pub const IN_LINE: &str = "passt zu NetBox";
    pub const UNREACHABLE: &str = "nicht erreichbar";

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
}

/// Choosing another address or other credentials (`components::adjust_target`).
pub mod target {
    use crate::components::adjust_target::{SelectedCredentials, SelectedTarget};

    pub const SHOW: &str = "Ändern";
    pub const HIDE: &str = "Ausblenden";
    pub const ACCESS: &str = "Zugang";

    /// How the device is reached: as NetBox and the config say, or what was chosen by hand.
    pub fn describe(target: &SelectedTarget) -> String {
        let address = match target.address {
            Some(address) => format!("Adresse von Hand: {address}"),
            None => "Adresse aus NetBox".into(),
        };
        let credentials = match &target.credentials {
            SelectedCredentials::Default => "Zugangsdaten des Tenants".into(),
            SelectedCredentials::Named(name) => format!("Zugangsdaten von Hand: „{name}“"),
            SelectedCredentials::Adhoc { username, .. } if username.is_empty() => {
                "Passwort von Hand".into()
            }
            SelectedCredentials::Adhoc { username, .. } => format!("Benutzer von Hand: {username}"),
        };
        format!("{address}, {credentials}")
    }
    pub const ADDRESS: &str = "IP-Adresse";
    pub const ADDRESS_PLACEHOLDER: &str = "Andere IP-Adresse";
    pub const INVALID_ADDRESS: &str = "Keine gültige IP-Adresse";
    pub const CREDENTIALS: &str = "Hinterlegte Zugangsdaten";
    pub const USERNAME: &str = "Benutzername";
    pub const PASSWORD: &str = "Passwort";
}

/// The device page (`pages::devices::show`).
pub mod device {
    pub const BOARD: &str = "Board";
    pub const ROUTEROS: &str = "RouterOS";
    pub const CHANGES: &str = "Änderungen";
    /// the card with RouterOS, device mode and partitions
    pub const DEVICE: &str = "Gerät";
    pub const APPLY: &str = "Anwenden";

    pub const LOCKED: &str = "Schreiben auf Geräte ist gesperrt";
    pub const LOCKED_BEFORE: &str = "Anwenden, Ersteinrichtung, Update und Neustart gibt erst ";
    pub const LOCKED_AFTER: &str = " in der config.yaml des Provisioners frei.";

    pub const ADJUST_PACKAGES: &str = "Pakete anpassen";
    pub const UP_TO_DATE: &str = " aktuell";
    pub const REBOOT: &str = "Neu starten";
    pub const REBOOT_NOW: &str = "Jetzt neu starten";
    pub const REBOOT_CONFIRM: &str =
        "Das Gerät und was es über PoE versorgt, ist während des Neustarts weg. ";

    pub const DEVICE_MODE: &str = "Device-Mode";
    pub const PARTITIONS: &str = "Partitionen";
    pub const PARTITIONS_SPLIT: &str = "In zwei Partitionen aufteilen";
    pub const PARTITIONS_NOW: &str = "Jetzt aufteilen";
    pub const PARTITIONS_CONFIRM: &str = "Das Gerät formatiert den Flash neu und startet neu; laut \
        MikroTik bleibt nur die laufende Partition erhalten. Das Gerät und was es über PoE versorgt, \
        ist währenddessen weg. ";

    /// one partition, e.g. `part0: RouterOS v7.23.8 …, 128 MiB, läuft`
    pub fn partition(name: &str, version: &str, size_mib: i64, active: bool, running: bool) -> String {
        let mut text = format!(
            "{name}: {}, {size_mib} MiB",
            if version.is_empty() { "leer" } else { version }
        );
        if running {
            text.push_str(", läuft");
        } else if active {
            text.push_str(", startet beim nächsten Neustart");
        }
        text
    }
    pub fn partition_boot(name: &str) -> String {
        format!("Von {name} starten")
    }
    pub const PARTITION_BOOT_NOW: &str = "Jetzt starten";
    pub const PARTITION_BOOT_CONFIRM: &str = "Das Gerät startet neu mit dem RouterOS und der \
        Konfiguration auf dieser Partition. Das Gerät und was es über PoE versorgt, ist \
        währenddessen weg. ";
    pub const DEVICE_MODE_ADJUST: &str = "Device-Mode anpassen";
    pub const DEVICE_MODE_NOW: &str = "Jetzt anpassen";
    pub const DEVICE_MODE_CONFIRM: &str = "Danach verlangt das Gerät eine Bestätigung vor Ort: den Knopf \
        am Gerät drücken oder den Strom kurz unterbrechen. Das Gerät und was es über PoE versorgt, \
        startet dabei neu. ";

    /// the features the provisioner needs that are off
    pub fn device_mode_missing(features: &[String]) -> String {
        format!("; fehlt: {}", features.join(", "))
    }

    pub const UNREACHABLE: &str = "Gerät nicht erreichbar";
    pub const UNREACHABLE_HINT: &str = "Mit einer anderen Adresse oder anderen Zugangsdaten versuchen:";
    pub const NOT_GENERATED: &str = "Keine Konfiguration erzeugt";
    pub const NOT_GENERATED_HINT: &str =
        "Der Fehler steht oben.";
    pub const IN_LINE: &str = "Gerät entspricht NetBox";

    pub fn update_to(version: &str) -> String {
        format!("Auf {version} aktualisieren")
    }

    pub fn packages(installed: &[String], wanted: &[String]) -> String {
        format!(" (Pakete: {}; nach dem Update: {})", installed.join(", "), wanted.join(", "))
    }

    /// what the channel select shows, e.g. `stable: 7.20.8`
    pub fn channel(name: &str, newest: &str) -> String {
        format!("{name}: {newest}")
    }

    /// The initial setup over MAC-Telnet from a neighbor (`bootstrapPlan`).
    pub mod setup {
        pub const IDENTITY: &str = "Identity";
        pub const BOARD: &str = "Board";
        pub const ROUTEROS: &str = "RouterOS";
        pub const MAC: &str = "MAC";
        pub const PORT: &str = "Port";
        pub const MAC_PLACEHOLDER: &str = "MAC-Adresse";
        pub const WITH_MAC: &str = "Mit dieser MAC einrichten";

        pub fn plan(neighbor: &str, neighbor_port: &str, port: &str, address: &str) -> String {
            format!(
                "Ersteinrichtung: {neighbor} öffnet an {neighbor_port} MAC-Telnet zum gewählten Gerät, meldet sich an, setzt das Passwort und gibt dessen {port} die Adresse {address}."
            )
        }

        pub fn searching(neighbor: &str, port: &str) -> String {
            format!(" {neighbor} sucht Geräte an {port} …")
        }

        pub fn search_failed(neighbor: &str, error: &str) -> String {
            format!("{neighbor} kann nicht nach Geräten suchen: {error}")
        }

        pub fn nothing_found(neighbor: &str, port: &str) -> String {
            format!("{neighbor} sieht an {port} kein Gerät.")
        }

        pub fn set_up_as(name: &str) -> String {
            format!("Als {name} einrichten")
        }

        pub fn impossible(problem: &str) -> String {
            format!("Keine Ersteinrichtung möglich: {problem}")
        }
    }

    /// The warnings of `generateCfg` (`ApplyWarning`), before applying.
    pub mod warning {
        /// `port` (with `default_name` where it is named otherwise) loses its PoE out
        pub fn poe_cut(port: &str, default_name: &str) -> String {
            let port = if port == default_name {
                port.to_string()
            } else {
                format!("{port} ({default_name})")
            };
            format!("Soll der Strom an {port} wirklich abgeschaltet werden?")
        }

        pub fn radio_bound(radio: &str, configuration: Option<&str>, expected: &str) -> String {
            format!(
                "{radio} ist an CAPsMAN gebunden und lässt sich nicht ändern: es verwendet die Konfiguration {} statt {expected}",
                configuration.unwrap_or("-")
            )
        }

        pub fn legacy_radio_managed(radio: &str, expected: &str) -> String {
            if radio == expected {
                format!("{radio} wird von CAPsMAN verwaltet und lässt sich nicht ändern: es behält seine Einstellungen")
            } else {
                format!("{radio} wird von CAPsMAN verwaltet und lässt sich nicht ändern: es behält seinen Namen (NetBox: {expected}) und seine Einstellungen")
            }
        }
    }
}

/// A job and its events (`components::job`), as the backend reports them.
pub mod job {
    pub const CANCELLING: &str = "Wird abgebrochen …";
    pub const LOST: &str = "Auftrag unterbrochen";
    pub const LOST_TEXT: &str = "Der Provisioner wurde neu gestartet und kennt den Auftrag nicht \
        mehr. Was der Auftrag auf dem Gerät begonnen hat, läuft dort weiter.";
    /// after LOST_TEXT for an apply or an update, whose fallback is still armed on the device
    pub const LOST_FALLBACK: &str = " Ein Fallback, den er scharf gemacht hat, setzt das Gerät nach \
        Ablauf seiner Frist zurück und startet es neu.";

    /// asking for the job failed, it is asked again
    pub fn server_away(cause: &str) -> String {
        format!("Keine Antwort vom Provisioner, es wird weiter gefragt … ({cause})")
    }
    pub const SCRIPT: &str = "Skript:";
    pub const REMAINING: &str = "Noch offen:";

    pub const UPDATE: &str = "RouterOS-Update";
    pub const APPLY: &str = "Anwenden";
    pub const BOOTSTRAP: &str = "Ersteinrichtung";
    pub const REBOOT: &str = "Neustart";
    pub const DEVICE_MODE: &str = "Device-Mode-Änderung";
    pub const REPARTITION: &str = "Aufteilen des Flash";
    pub const BOOT_PARTITION: &str = "Start von einer anderen Partition";
    pub const NETBOX_WRITE: &str = "Schreiben in NetBox";
    pub const NETBOX_UNDO: &str = "Rückgängig in NetBox";

    pub const NOTHING_TO_APPLY: &str = "Nichts anzuwenden, das Gerät entspricht NetBox";
    pub const NOT_ALL_APPLIED: &str = "Nicht alles wurde angewendet";
    pub const APPLIED: &str = "Angewendet, das Gerät entspricht jetzt NetBox";

    pub fn set_up(address: &str) -> String {
        format!("Ersteinrichtung abgeschlossen, das Gerät antwortet an {address}")
    }
    pub fn running(what: &str) -> String {
        format!("{what} läuft")
    }
    /// `update`: from and to version
    pub fn done(what: &str, update: Option<(&str, &str)>) -> String {
        match update {
            Some((from, to)) => format!("{what} abgeschlossen: RouterOS {from} → {to}"),
            None => format!("{what} abgeschlossen"),
        }
    }
    pub fn failed(what: &str) -> String {
        format!("{what} fehlgeschlagen")
    }
    pub fn cancelled(what: &str) -> String {
        format!("{what} abgebrochen")
    }
    /// a state this frontend doesn't know yet
    pub fn other_state(what: &str, state: &str) -> String {
        format!("{what}: {state}")
    }

    /// The events, one sentence each.
    pub mod event {
        use crate::i18n::common::list;

        pub const NOTHING_TO_DO: &str = "Nichts zu tun: das Gerät entspricht NetBox";
        pub const IN_LINE: &str = "Das Gerät entspricht NetBox";
        pub const FALLBACK_STOPPED: &str =
            "Fallback gestoppt: das Gerät ist mit der neuen Konfiguration erreichbar";

        pub fn fallback_armed(seconds: u64) -> String {
            format!(
                "Backup gespeichert: das Gerät lädt es in {seconds} s, wenn der Provisioner es danach nicht erreicht"
            )
        }
        pub fn script_stopped(error: &str, seconds: u64) -> String {
            format!(
                "Das Skript hat angehalten ({error}), die Zeilen davor sind angewendet; wartet bis zu {seconds} s auf das Gerät"
            )
        }
        pub fn script_running(seconds: u64) -> String {
            format!("Das Skript läuft auf dem Gerät; wartet bis zu {seconds} s, bis es NetBox entspricht")
        }
        pub fn not_checked(error: &str) -> String {
            format!("Das Gerät lässt sich nicht prüfen: {error}")
        }

        /// Why the fallback stays (`FallbackLeftReason`).
        pub enum FallbackLeft<'a> {
            Cancelled,
            NotReached,
            NotStopped(Option<String>),
            Other(&'a str),
        }
        pub fn fallback_left(reason: FallbackLeft, seconds: u64) -> String {
            let reason = match reason {
                FallbackLeft::Cancelled => "Abgebrochen, der Fallback bleibt".into(),
                FallbackLeft::NotReached => "Gerät nicht erreicht, der Fallback bleibt".into(),
                FallbackLeft::NotStopped(Some(error)) => {
                    format!("Fallback lässt sich nicht stoppen ({error})")
                }
                FallbackLeft::NotStopped(None) => "Fallback lässt sich nicht stoppen".into(),
                FallbackLeft::Other(other) => format!("Fallback bleibt ({other})"),
            };
            format!(
                "{reason}: das Gerät lädt in etwa {seconds} s das Backup von vor dem Anwenden und startet neu"
            )
        }

        pub fn setup_started(neighbor: &str, port: &str, macs: &[String]) -> String {
            format!("Konsole auf {neighbor}, MAC-Telnet an {port} zu {}", macs.join(", "))
        }
        pub fn waiting_for_api(seconds: u64, address: &str) -> String {
            format!("Wartet bis zu {seconds} s, bis das Gerät an {address} die API öffnet")
        }
        pub fn setup_done(address: &str) -> String {
            format!("Das Gerät antwortet an {address}")
        }
        pub fn radios_reprovisioned(count: i64, menu: &str) -> String {
            format!("{count} Radios neu provisioniert ({menu})")
        }
        pub fn radios_not_reprovisioned(menu: &str, error: &str) -> String {
            format!("{menu} nicht neu provisioniert: {error}")
        }

        pub fn poe_chain(suppliers: &[String], consumers: &[String]) -> String {
            let mut parts = Vec::new();
            if !suppliers.is_empty() {
                parts.push(format!("über PoE versorgt von {}", suppliers.join(", ")));
            }
            if !consumers.is_empty() {
                parts.push(format!("versorgt über PoE {}", consumers.join(", ")));
            }
            format!("Das Gerät wird {}", parts.join(" und "))
        }
        pub fn waiting_for_supplier(device: &str) -> String {
            format!("Wartet, bis {device} (versorgt das Gerät über PoE) nach seinem Neustart zurück ist")
        }
        pub fn waiting_for_consumer(device: &str) -> String {
            format!("Wartet mit dem Neustart, bis {device} (über PoE versorgt) sein Update abgeschlossen hat")
        }
        pub fn waiting_for(device: &str, reason: &str) -> String {
            format!("Wartet auf {device} ({reason})")
        }

        pub fn update_start(version: &str, architecture: &str, packages: &[String]) -> String {
            format!("RouterOS {version} ({architecture}), Pakete: {}", list(packages))
        }
        pub fn update_step(version: &str, files: &[String], uninstall: &[String]) -> String {
            let mut text = format!("Update auf {version}: {}", list(files));
            if !uninstall.is_empty() {
                text.push_str(&format!("; entfernt {}", uninstall.join(", ")));
            }
            text
        }
        pub fn ssh_restored(setting: &str, value: &str) -> String {
            format!("SSH: {setting}={value} wieder gesetzt")
        }
        pub fn ssh_for_upload(setting: &str, value: &str) -> String {
            format!("SSH: {setting}={value} für den Upload (der Benutzer hat SSH-Schlüssel)")
        }
        pub fn downloaded(file: &str, bytes: i64) -> String {
            format!("{file}: {:.1} MB heruntergeladen", bytes as f64 / 1_000_000.0)
        }
        pub fn uploaded(file: &str) -> String {
            format!("{file}: auf das Gerät kopiert")
        }

        /// Why a reboot happens (`RebootReason`).
        pub enum Reboot<'a> {
            Packages,
            Firmware,
            Requested,
            DeviceMode,
            Repartition,
            Partition,
            Other(&'a str),
        }
        pub fn reboot(reason: Reboot, minutes: u64) -> String {
            let reason = match reason {
                Reboot::Packages => "zum Installieren der Pakete",
                Reboot::Firmware => "für die neue Firmware",
                Reboot::Requested => "auf Anfrage",
                Reboot::DeviceMode => "nach der Bestätigung",
                Reboot::Repartition => "zum Aufteilen des Flash",
                Reboot::Partition => "von der anderen Partition",
                Reboot::Other(other) => other,
            };
            format!("Neustart {reason}, wartet bis zu {minutes} min, bis das Gerät zurück ist")
        }
        pub fn routeros_running(version: &str) -> String {
            format!("RouterOS {version} läuft")
        }
        pub fn firmware_running(version: &str) -> String {
            format!("Firmware {version} läuft")
        }
        pub fn firmware_upgrade(current: &str, upgrade: &str) -> String {
            format!("Firmware {current} → {upgrade}")
        }
        pub fn update_done(from: &str, to: &str) -> String {
            format!("Fertig: RouterOS {from} → {to}")
        }

        pub fn device_mode_confirm(features: &[String], minutes: u64) -> String {
            format!(
                "Jetzt am Gerät bestätigen: den Knopf (Reset oder Mode) drücken oder den Strom kurz unterbrechen, innerhalb von {minutes} min. Schaltet ein: {}",
                features.join(", ")
            )
        }
        pub fn partition_running(name: &str, version: &str) -> String {
            format!("Läuft von {name}: {version}")
        }
        pub fn update_fallback_armed(partition: &str, version: &str, minutes: u64) -> String {
            format!(
                "{version} samt Konfiguration auf {partition} kopiert: Erreicht der Provisioner das \
                 Gerät nicht innert {minutes} min nach dem Start, startet es wieder von {partition}"
            )
        }
        pub fn update_fallback_disarmed(partition: &str) -> String {
            format!("Gerät erreicht, es schaltet nicht mehr selbst zurück; {partition} bleibt als Weg zurück")
        }
        pub fn update_without_fallback(partitions: i64) -> String {
            match partitions {
                0 => "Ohne Weg zurück: Das Gerät hat keine Partitionen".into(),
                n => format!(
                    "Ohne Weg zurück: {n} Partition(en), es braucht zwei (Partitionen auf der Geräteseite)"
                ),
            }
        }
        pub fn netbox_created(display: &str, path: &str) -> String {
            format!("In NetBox angelegt: {display} ({path})")
        }
        pub fn netbox_changed(
            display: &str,
            field: &str,
            value: Option<&str>,
            previous: Option<&str>,
        ) -> String {
            format!(
                "In NetBox geändert: {display}, {field} {} (vorher {})",
                value.unwrap_or("leer"),
                previous.unwrap_or("leer")
            )
        }
        pub fn netbox_deleted(display: &str) -> String {
            format!("In NetBox gelöscht: {display}")
        }
        pub fn netbox_restored(display: &str) -> String {
            format!("In NetBox zurückgesetzt: {display}")
        }
        pub fn ospf_neighbors_back(neighbors: &[String]) -> String {
            format!("OSPF-Nachbarn wieder Full: {}", neighbors.join(", "))
        }
        pub fn repartition_started(count: i64, files: i64) -> String {
            format!("Teilt den Flash in {count} Partitionen auf ({files} Dateien vorher)")
        }
        pub fn repartitioned(partitions: &[String], missing_files: &[String]) -> String {
            let mut text = format!("Aufgeteilt: {}", partitions.join("; "));
            if !missing_files.is_empty() {
                text.push_str(&format!(". Nicht mehr da: {}", missing_files.join(", ")));
            }
            text
        }
        pub fn device_mode_changed(features: &[String]) -> String {
            if features.is_empty() {
                "Der Device-Mode hat schon alles, was der Provisioner braucht".into()
            } else {
                format!("Device-Mode geändert, eingeschaltet: {}", features.join(", "))
            }
        }
    }
}

/// The helpers that write to NetBox (`components::netbox_helper`).
pub mod helper {
    pub const TITLE: &str = "NetBox ergänzen";
    pub const LOOPBACK: &str = "Loopback";
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
            "PortLikeName" => "Name wie ein Port".into(),
            "VlanWithoutGroup" => "VLAN ohne Gruppe".into(),
            "MtuMismatch" => "MTU verschieden".into(),
            "OspfOnOneEnd" => "OSPF nur an einem Ende".into(),
            "NoCommonSubnet" => "kein gemeinsames Netz".into(),
            "AddressWithoutOspf" => "Adresse ohne OSPF".into(),
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
    pub fn create_interface(name: &str, kind: &str) -> String {
        format!("Interface {name} ({kind}) anlegen")
    }
    pub fn create_address(address: &str, interface: &str, prefix: &str) -> String {
        format!("Adresse {address} auf {interface} anlegen (aus {prefix})")
    }
    pub fn set_primary(version: &str, address: &str, before: Option<&str>) -> String {
        match before {
            Some(before) => format!("Primary {version} auf {address} setzen (vorher {before})"),
            None => format!("Primary {version} auf {address} setzen"),
        }
    }
}
