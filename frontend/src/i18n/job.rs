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

    pub fn fallback_armed(seconds: u64, radio_wait: Option<u64>) -> String {
        let text = format!(
            "Backup gespeichert: das Gerät lädt es in {seconds} s, wenn der Provisioner es danach nicht erreicht"
        );
        match radio_wait {
            Some(wait) => format!(
                "{text}; {} min davon, weil sich ein Funk ändert: auf einem DFS-Kanal prüft der AP \
                 bis zu 10 min auf Radar, bevor er sendet",
                wait / 60
            ),
            None => text,
        }
    }
    pub fn fallback_unavailable(device_mode: &str) -> String {
        format!(
            "Ohne Fallback: Der Device-Mode {device_mode} hat den Scheduler aus (Device-Mode \
             anpassen auf der Geräteseite). Schneidet das Anwenden das Gerät ab, holt nichts \
             die alte Konfiguration zurück"
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

    /// the devices applied together
    pub fn batch_apply(devices: &[String]) -> String {
        format!(
            "Gemeinsam anwenden: {}. Die Skripte werden auf allen hinterlegt und dann gleichzeitig \
             gestartet, die Fallbacks werden erst weggenommen, wenn alle wieder erreichbar und \
             angewendet sind",
            devices.join(", ")
        )
    }
    pub fn apply_written(lead: u64) -> String {
        format!("Skript auf dem Gerät hinterlegt; gestartet wartet es {lead} s, bis alle laufen")
    }
    pub fn apply_started(seconds: u64) -> String {
        format!("Skript gestartet, wendet in {seconds} s an")
    }
    pub fn apply_not_started(problem: &str) -> String {
        format!("Skript nicht gestartet: {problem}; das Gerät bleibt, wie es ist, und alle gehen zurück")
    }
    pub fn batch_stopped(device: &str) -> String {
        format!("Gemeinsames Anwenden abgebrochen, bevor etwas angewendet wurde: {device} ging nicht")
    }
    pub fn batch_incomplete(unreached: &[String], not_applied: &[String]) -> String {
        let mut parts = Vec::new();
        if !unreached.is_empty() {
            parts.push(format!("nicht wieder erreicht: {}", unreached.join(", ")));
        }
        if !not_applied.is_empty() {
            parts.push(format!("nicht angewendet: {}", not_applied.join(", ")));
        }
        format!(
            "Nicht alle durch ({}). Kein Fallback wird weggenommen, alle Geräte laden die \
             Konfiguration von vorher",
            parts.join("; ")
        )
    }
    /// Why the fallback stays (`FallbackLeftReason`).
    pub enum FallbackLeft<'a> {
        Cancelled,
        NotReached,
        NotStopped(Option<String>),
        NotApplied,
        BatchIncomplete,
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
            FallbackLeft::NotApplied => {
                "Erreichbar, aber die Änderungen sind nicht angewendet, der Fallback bleibt".into()
            }
            FallbackLeft::BatchIncomplete => {
                "Ein anderes Gerät des gemeinsamen Anwendens ist nicht durch, der Fallback bleibt".into()
            }
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
    pub fn uploading(file: &str, bytes: i64, total: i64) -> String {
        if bytes == 0 {
            return format!("{file}: Gerät nimmt die Datei an, Übertragung beginnt");
        }
        format!(
            "{file}: {:.1} von {:.1} MB auf das Gerät übertragen",
            bytes as f64 / 1_000_000.0,
            total as f64 / 1_000_000.0
        )
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
    /// the fields changed: name, value and the one before
    pub fn netbox_changed<'a>(
        display: &str,
        changes: impl Iterator<Item = (&'a str, Option<&'a str>, Option<&'a str>)>,
    ) -> String {
        let changes: Vec<String> = changes
            .map(|(field, value, previous)| match field {
                "tags" => [
                    value.map(|added| format!("Tag {added} dazu")),
                    previous.map(|removed| format!("Tag {removed} weg")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", "),
                _ => format!(
                    "{field} {} (vorher {})",
                    value.unwrap_or("leer"),
                    previous.unwrap_or("leer")
                ),
            })
            .collect();
        format!("In NetBox geändert: {display}, {}", changes.join(", "))
    }
    pub fn netbox_deleted(display: &str) -> String {
        format!("In NetBox gelöscht: {display}")
    }
    /// created again by an undo, with a new id
    pub fn netbox_recreated(display: &str, path: &str) -> String {
        format!("In NetBox wieder angelegt: {display} ({path})")
    }
    pub fn netbox_restored(display: &str) -> String {
        format!("In NetBox zurückgesetzt: {display}")
    }
    pub fn ospf_neighbors_back(neighbors: &[String]) -> String {
        format!("OSPF-Nachbarn wieder Full: {}", neighbors.join(", "))
    }
    pub fn connecting(device: &str) -> String {
        format!("Verbinde mit {device}")
    }
    pub fn connected(
        device: &str,
        address: &str,
        neighbor: Option<&str>,
        identity: &str,
        same_name: bool,
    ) -> String {
        let way = match neighbor {
            Some(neighbor) => format!("{address}, über {neighbor}"),
            None => address.to_string(),
        };
        if same_name {
            format!("Verbunden mit {device} an {way}")
        } else {
            format!("Verbunden mit {device} an {way}; das Gerät heisst „{identity}“")
        }
    }
    pub fn update_target(version: &str, channel: Option<&str>) -> String {
        match channel {
            Some(channel) => format!("Ziel: RouterOS {version} ({channel})"),
            None => format!("Ziel: die laufende Version {version} (nur Pakete und Firmware)"),
        }
    }
    pub fn watchdog_not_set(device: &str) -> String {
        format!("{device} hat keinen Watchdog, der auslösen könnte: nichts auszusetzen")
    }
    /// what a device's watchdogs watch: its watch address and the ports power cycling
    /// the updating device (port, address)
    pub fn watching(address: Option<&str>, ports: &[(&str, &str)]) -> String {
        address
            .map(|address| format!("watch-address {address}"))
            .into_iter()
            .chain(ports.iter().map(|(port, address)| format!("PoE-Ping an {port} auf {address}")))
            .collect::<Vec<_>>()
            .join(", ")
    }
    pub fn watchdog_suspended(device: &str, watching: &str, safety_net_minutes: u64) -> String {
        format!(
            "Watchdog von {device} ausgesetzt ({watching}); stellt sich nach \
            {safety_net_minutes} min selbst wieder her, falls der Provisioner es nicht tut"
        )
    }
    pub fn watchdog_restored(device: &str, watching: &str) -> String {
        format!("Watchdog von {device} wieder aktiv ({watching})")
    }
    pub fn watchdog_not_restored(device: &str, watching: &str, error: &str) -> String {
        format!(
            "Watchdog von {device} nicht wiederhergestellt ({error}): der Scheduler auf \
            {device} stellt {watching} nach Ablauf seiner Frist selbst wieder her"
        )
    }
    pub fn watchdog_left(device: &str, error: &str) -> String {
        format!(
            "Watchdog von {device} nicht ausgesetzt ({error}): {device} kann in der Zeit neu \
            starten"
        )
    }
    pub fn watchdog_kept(device: &str, watching: &str, safety_net_minutes: u64) -> String {
        format!(
            "Watchdog von {device} bleibt ausgesetzt, solange der Fallback noch auslösen kann: \
            der Scheduler auf {device} stellt {watching} nach {safety_net_minutes} min wieder her"
        )
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
