//! Every text the UI shows, in German, by the page or component showing it. Components take
//! their texts from here and write none inline; the backend sends data, which is worded here
//! (job events, warnings). A second language replaces these functions, or makes them choose
//! by language, without touching the components. Names RouterOS or NetBox use themselves
//! (`RouterOS`, `Board`, `long-term`) stay as they are.

/// Texts used in several places.
pub mod common {
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
            "CONSOLE_LOGIN" => format!(
                "Anmeldung über mac-telnet an {} abgelehnt oder unbeantwortet",
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
            "REFUSED" => format!("Das Gerät lehnt ab: {}", p("cause")),
            "MISSING_DEPENDENCIES" => format!(
                "Änderungen warten aufeinander oder verweisen auf Fehlendes: {}",
                p("cause")
            ),
            "BUSY" => format!("Auf {} läuft schon ein Auftrag", p("device")),
            "UNKNOWN_JOB" => format!("Auftrag {} gibt es nicht (mehr)", p("id")),
            "CANCELLED" => "Abgebrochen".into(),
            "PING" => format!("Ping fehlgeschlagen: {}", p("cause")),
            "NO_ROUTERBOARD" => "Das Gerät meldet kein RouterBOARD".into(),
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

/// Choosing another address or other credentials (`components::adjust_target`).
pub mod target {
    pub const SHOW: &str = "Ziel ändern";
    pub const HIDE: &str = "Zielauswahl ausblenden";
    pub const ADDRESS: &str = "IP-Adresse";
    pub const ADDRESS_PLACEHOLDER: &str = "Andere IP-Adresse";
    pub const INVALID_ADDRESS: &str = "Keine gültige IP-Adresse";
    pub const CREDENTIALS: &str = "Hinterlegte Zugangsdaten";
    pub const USERNAME: &str = "Benutzername";
    pub const PASSWORD: &str = "Passwort";
}

/// The device page (`pages::devices::show`).
pub mod device {
    pub const NAME: &str = "Name";
    pub const BOARD: &str = "Board";
    pub const ROUTEROS: &str = "RouterOS";
    pub const CHANGES: &str = "Änderungen";
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

    pub const UNREACHABLE: &str = "Gerät nicht erreichbar";
    pub const UNREACHABLE_HINT: &str =
        "Unter „Ziel ändern“ eine andere Adresse oder andere Zugangsdaten wählen.";
    pub const NOT_GENERATED: &str = "Keine Konfiguration erzeugt";
    pub const NOT_GENERATED_HINT: &str =
        "Der Fehler steht oben; ältere RouterOS-Versionen melden Felder, die das Modell nicht kennt.";
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
    }
}

/// A job and its events (`components::job`), as the backend reports them.
pub mod job {
    pub const CANCELLING: &str = "Wird abgebrochen …";
    pub const SCRIPT: &str = "Skript:";
    pub const REMAINING: &str = "Noch offen:";

    pub const UPDATE: &str = "RouterOS-Update";
    pub const APPLY: &str = "Anwenden";
    pub const BOOTSTRAP: &str = "Ersteinrichtung";
    pub const REBOOT: &str = "Neustart";

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
            Other(&'a str),
        }
        pub fn reboot(reason: Reboot, minutes: u64) -> String {
            let reason = match reason {
                Reboot::Packages => "zum Installieren der Pakete",
                Reboot::Firmware => "für die neue Firmware",
                Reboot::Requested => "auf Anfrage",
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
    }
}
