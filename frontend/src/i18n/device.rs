pub const BOARD: &str = "Board";
pub const CONNECTED: &str = "Verbunden mit";
/// the device the connection reached (`access.connected`)
pub fn connected(address: &str, neighbor: Option<&str>, identity: &str) -> String {
    match neighbor {
        Some(neighbor) => format!("„{identity}“ an {address}, über {neighbor}"),
        None => format!("„{identity}“ an {address}"),
    }
}
pub fn other_identity(identity: &str, name: &str) -> String {
    format!(
        "Das Gerät heisst „{identity}“, nicht „{name}“: vor dem Anwenden prüfen, ob es das \
         richtige ist (nach der Ersteinrichtung heisst es noch wie ab Werk)"
    )
}
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
pub const NO_WAY_BACK_TITLE: &str = "Kein Weg zurück nach einem Update";
pub const NO_WAY_BACK: &str = "Das Gerät hat keine zweite Partition mit dem laufenden RouterOS, \
    und kein Nachbar erreicht es, wenn es nach dem Update nicht ins Netz zurückfindet. Geht dabei \
    etwas schief, muss jemand vor Ort hin.";
pub const UPDATE_WITHOUT_WAY_BACK: &str = "Trotzdem ohne Weg zurück aktualisieren";
pub const WATCHDOG_TITLE: &str = "Watchdogs, die während des Updates auslösen können";
/// a device watching an address the update may cut off; `supplier`: it powers this one
pub fn watchdog_watching(device: &str, supplier: bool, address: &str) -> String {
    if supplier {
        format!(
            "{device} speist dieses Gerät über PoE und überwacht {address}. Ist die Adresse \
            nur über dieses Gerät erreichbar, startet {device} während des Updates neu und \
            schaltet ihm mitten in der Installation den Strom ab."
        )
    } else {
        format!(
            "{device} überwacht {address} auf dem Weg über dieses Gerät und startet neu, wenn \
            das Update länger dauert als sein ping-timeout."
        )
    }
}
/// a port powering this device, power cycling it when it doesn't answer
pub fn watchdog_poe_ping(device: &str, port: &str, address: &str) -> String {
    format!(
        "{device} schaltet {port} aus und wieder ein, wenn {address} nicht antwortet: dauert \
        die Installation zu lange, verliert dieses Gerät mittendrin den Strom."
    )
}
pub fn watchdog_unread(device: &str, problem: &str) -> String {
    format!("Watchdog von {device} nicht lesbar: {problem}")
}
pub const SUSPEND_WATCHDOGS: &str = "Watchdogs während des Updates aussetzen (danach wieder \
    hergestellt; ein Scheduler auf jedem Gerät tut es nach 2 h selbst, falls der Provisioner \
    nicht dazu kommt)";
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

    /// the board's radios run with `package`
    pub fn cap_off_outside_wlan_group(package: &str) -> String {
        let menu = if package == "wireless" { "/interface wireless" } else { "/interface wifi" };
        format!(
            "Das Gerät ist in NetBox in keiner WLAN-Gruppe und nutzt keinen seiner Funk-Ports: \
             sein CAP ({menu} cap) ist danach aus, es sendet kein WLAN. Für einen Access Point \
             die WLAN-Gruppe in NetBox ergänzen."
        )
    }

    /// a watchdog's address (`port`'s power cycling, else the ping watchdog) that doesn't
    /// answer from the device
    pub fn watch_target_silent(address: &str, port: Option<&str>, device: &str) -> String {
        let watchdog = match port {
            Some(port) => format!("Der PoE-Watchdog an {port}"),
            None => "Der Ping-Watchdog".to_string(),
        };
        let device = if device.is_empty() { String::new() } else { format!(" ({device})") };
        format!(
            "{watchdog} soll {address}{device} überwachen, aber die Adresse antwortet vom Gerät \
             aus nicht: er bleibt, wie er ist, bis sie antwortet (ein Gerät ohne erstes \
             Anwenden hat noch keine Loopback)."
        )
    }
}
