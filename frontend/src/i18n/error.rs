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
/// `PORT_IN_BRIDGES`: a port (the VXLAN of a VLAN group) whose VLANs NetBox puts in
/// different bridges, each VLAN with its bridge (`vlans`, `vlanNames`, `bridges`: a line each).
fn port_in_bridges(problem: &ServerProblem) -> String {
    let lines = |name| problem.param(name).split('\n');
    let vlans = lines("vlans")
        .zip(lines("vlanNames"))
        .zip(lines("bridges"))
        .map(|((vlan, name), bridge)| {
            let vlan = match (vlan, name) {
                ("", _) => "ohne VLAN".to_string(),
                (vlan, "") => format!("VLAN {vlan}"),
                (vlan, name) => format!("VLAN {vlan} ({name})"),
            };
            match bridge {
                "" => format!("{vlan} ohne Bridge"),
                bridge => format!("{vlan} in {bridge}"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} müsste Port mehrerer Bridges sein, seine VLANs liegen in NetBox in verschiedenen: \
         {vlans}. Ein Port kann nur in einer Bridge sein: leg diese VLANs in dieselbe Bridge",
        problem.param("port")
    )
}

pub fn server(problem: &ServerProblem) -> String {
    let p = |name| problem.param(name);
    match problem.code.as_str() {
        "DEVICE_API" => format!("Zugriff auf das Gerät fehlgeschlagen: {}", p("cause")),
        "MISSING_CREDENTIALS" => "Keine Zugangsdaten für das Gerät".into(),
        "APPLY_NOT_ALLOWED" => "Schreiben auf Geräte ist gesperrt (allow_apply)".into(),
        "UNKNOWN_DEVICE" => format!("Gerät {} gibt es in NetBox nicht", p("id")),
        "UNKNOWN_DEVICE_NAME" => format!("Gerät {} gibt es in NetBox nicht (mehr)", p("device")),
        "UNKNOWN_SITE" => format!("Site {} gibt es in NetBox nicht", p("id")),
        "UNKNOWN_INTERFACE" => format!("Interface {} gibt es in NetBox nicht", p("id")),
        "NO_TOPOLOGY" => "NetBox liess sich nicht lesen".into(),
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
        "NEIGHBOR_NO_ANSWER" => format!(
            "{} hat die Verbindung zu {} angenommen, aber das Gerät hat in drei Versuchen \
             nicht auf den Login geantwortet: kommt der Nachbar bis zum API-Dienst (Firewall \
             des Geräts, Route auf {})?",
            p("neighbor"),
            p("address"),
            p("neighbor")
        ),
        "NEIGHBOR_NO_FORWARDING" => format!(
            "{} leitet keine Verbindungen zum Gerät weiter (/ip ssh forwarding-enabled={}); \
             es braucht local, wie es jede generierte Konfiguration hat: {} zuerst anwenden",
            p("neighbor"),
            p("forwarding"),
            p("neighbor")
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
        "REBOOT_NOT_SENT" => "Der Provisioner wurde neu gestartet, bevor er das Gerät neu startete: \
                              Die hochgeladenen Pakete und der Fallback des Updates liegen noch \
                              auf dem Gerät (siehe Geräteseite)"
            .into(),
        "UPDATE_FAILED_BEFORE" => format!("Das Update schlug vor dem Neustart des Provisioners fehl: {}", p("message")),
        "PANIC" => format!("Interner Fehler im Provisioner (bitte melden): {}", p("message")),
        "UPDATE_ROLLED_BACK" => format!(
            "Update zurückgenommen: Das Gerät war nach dem Update nicht erreichbar und läuft \
             wieder mit RouterOS {} von {}",
            p("version"),
            p("partition")
        ),
        "NO_POWER_SUPPLY" => format!("Kein Gerät speist {} über PoE (NetBox)", p("device")),
        "POE_PORT_NOT_FOUND" => format!("{} hat keinen Port {}, an dem es PoE schalten kann", p("device"), p("port")),
        "LEFTOVER_GONE" => format!("Auf {} liegt das nicht mehr, die Seite ist neu geladen", p("device")),
        "LEFTOVER_HELD" => format!(
            "Die Watchdogs von {} hält der laufende Auftrag #{} ausgesetzt; er setzt sie an seinem Ende wieder",
            p("device"),
            p("job")
        ),
        "NO_DOWNGRADE" => format!(
            "Kein Downgrade von RouterOS {} auf {}: nur auf eine ältere Version von RouterOS 7",
            p("running"),
            p("target")
        ),
        "NO_VERSION" => format!("„{}“ ist keine RouterOS-Version", p("version")),
        "NO_UPDATE_FALLBACK" => format!(
            "{} hätte nach einem Update keinen Weg zurück (keine zweite Partition, kein Nachbar, \
             über den es erreichbar bleibt). Nur mit Bestätigung auf der Geräteseite",
            p("device")
        ),
        "OSPF_NEIGHBORS_MISSING" => format!(
            "Nach dem Update fehlen OSPF-Nachbarn, mit denen das Gerät vorher benachbart war: {}. \
             Zurück geht es auf der Geräteseite unter Partitionen",
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
        "BATCH_STOPPED" => format!("Gemeinsames Anwenden abgebrochen: {} ging nicht", p("device")),
        "BATCH_INCOMPLETE" => format!(
            "Nicht durch: {}; alle Geräte laden die Konfiguration von vorher",
            p("devices")
        ),
        "SCRIPT_NOT_STARTED" => format!("Skript läuft nicht innerhalb von {} s nach dem Start", p("seconds")),
        "UNKNOWN_JOB" => format!("Auftrag {} gibt es nicht (mehr)", p("id")),
        "NO_SAVED_JOB" => format!("Für Gerät {} wartet kein gespeicherter Auftrag (mehr)", p("device")),
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
        "SAME_DEVICE" => format!("Beide Enden sind auf {}", p("device")),
        "NO_ROUTEROS" => format!("Auf {} läuft laut NetBox kein RouterOS", p("device")),
        "PORT_CABLED" => match p("far") {
            "" => format!("{} hat schon ein Kabel", p("port")),
            far => format!("{} hat schon ein Kabel zu {far}", p("port")),
        },
        "PORT_PATCHED" => format!(
            "{} ist auf ein Patch-Panel verkabelt: das Kabel in NetBox von Hand korrigieren, ein \
             direktes Kabel würde die Panels ersetzen",
            p("port")
        ),
        "PORT_IN_BRIDGE" => format!(
            "{} ist Port der Bridge {}: eine Verbindung braucht den Port für sich",
            p("port"),
            p("bridge")
        ),
        "PORT_ADDRESSED" => format!(
            "{} hat schon die Adresse {}",
            p("port"),
            p("address")
        ),
        "TAG_MISSING" => format!("Den Tag {} gibt es in NetBox nicht", p("tag")),
        "PRIMARY_ADDRESS" => format!(
            "{} ist die Primary IP von {}: zuerst eine andere Primary IP setzen (Loopback)",
            p("address"),
            p("device")
        ),
        "NOTHING_TO_DISCONNECT" => format!(
            "{} hat in NetBox kein Kabel und keine Adresse aus dem Transfer-Pool mehr",
            p("port")
        ),
        "NO_BRIDGE" => format!("{} ist in keiner Bridge", p("port")),
        "ADDRESS_IN_NETBOX" => format!("NetBox hat {} schon", p("address")),
        "INVALID_ADDRESS" => format!("{} ist keine Adresse mit Länge", p("address")),
        "INVALID_MAC" => format!("{} ist keine MAC-Adresse", p("mac")),
        "NO_DHCP_NETWORK" => format!("{} liegt in keinem DHCP-Netz des Routers", p("address")),
        "NO_CUSTOMER_NET" => format!(
            "{} hat in NetBox kein Kunden-Netz mehr: keine Adresse, kein dhcp und kein VLAN",
            p("interface")
        ),
        "VLAN_ID_TAKEN" => format!(
            "VID {} hat auf dem Gerät schon VLAN {} der Gruppe {}: RouterOS kennt nur die Nummer",
            p("vid"),
            p("vlan"),
            p("group")
        ),
        "VXLAN_IN_BRIDGE" => {
            let bridge = match p("bridge") {
                "" => "der Standard-Bridge".to_string(),
                bridge => format!("Bridge {bridge}"),
            };
            format!(
                "Das VXLAN der Gruppe {} hängt auf dem Gerät an {bridge}, der Port käme in eine andere: ein VXLAN kann nur in einer Bridge sein",
                p("group")
            )
        }
        "PORT_UPLINK" => format!("{} ist ein Uplink (dhcp-client oder nat)", p("port")),
        "PORT_HAS_VLAN" => format!("{} hat VLAN {} schon, untagged und ohne weitere", p("port"), p("vlan")),
        "NO_SITE" => format!("{} hat in NetBox keine Site", p("device")),
        "NO_DEVICE_NAME" => "Das Gerät braucht einen Namen".to_string(),
        "DEVICE_NAME_TAKEN" => format!("NetBox hat schon ein Gerät {}", p("name")),
        "UNKNOWN_WLAN_GROUP" => {
            format!("WLAN-Gruppe {} gibt es nicht oder sie hat keinen Controller", p("id"))
        }
        "NO_PLATFORM" => format!("NetBox hat keine Plattform „{}“", p("name")),
        "UNKNOWN_PREFIX" => format!("Prefix {} ist keines der Site des Geräts", p("id")),
        "UNKNOWN_VLAN" => format!("VLAN {} hat das Gerät nicht", p("id")),
        "NETWORK_LENGTH" => format!(
            "Ein /{} passt nicht in {} (länger als das Prefix, höchstens /30)",
            p("length"),
            p("parent")
        ),
        "NOT_ETHERNET" => format!("{} ist laut NetBox kein Ethernet-Port", p("port")),
        "NO_SINGLE_RADIO" => format!(
            "{} hat in NetBox keinen oder mehrere Funk-Ports: eine Funkbrücke hat genau einen",
            p("device")
        ),
        "NO_ROUTER_PORT" => format!(
            "{} ist mit keinem Router verkabelt: den Router-Port wählen",
            p("device")
        ),
        "WIRELESS_LINK_ELSEWHERE" => match p("far") {
            "" => format!("{} hat schon einen Wireless Link", p("port")),
            far => format!("{} hat schon einen Wireless Link zu {far}", p("port")),
        },
        "PSK_UNREADABLE" => format!(
            "Der Wireless Link an {} ist verschlüsselt, NetBox gibt seinen PSK aber nicht \
            heraus (Rechte des API-Tokens)",
            p("port")
        ),
        "NETWORK_TOO_SMALL" => format!(
            "{} hat keinen Platz für {} Adressen (zwei Router, zwei Brücken)",
            p("net"),
            p("ends")
        ),
        "NAME_TAKEN" => format!("{} hat schon ein Interface {}", p("device"), p("name")),
        "WRONG_DEVICE" => format!(
            "An {} antwortet {}, ein anderes Gerät in NetBox, nicht {}: Adresse doppelt \
             vergeben oder ein DHCP-Client noch aktiv?",
            p("address"),
            p("other"),
            p("device")
        ),
        "WATCHDOG_NOT_SUSPENDED" => format!(
            "Der Watchdog von {} liess sich nicht aussetzen ({}): kein Update gestartet",
            p("device"),
            p("cause")
        ),
        "NO_PARENT" => "Ein IPv4- oder IPv6-Prefix wählen, aus dem das Netz kommt".into(),
        "NO_FREE_NETWORK" => format!("In {} ist kein /{} mehr frei", p("parent"), p("length")),
        "NO_ROUTERBOARD" => "Das Gerät meldet kein RouterBOARD".into(),
        "NO_PREFIX" => format!(
            "DHCP-Server auf {} (Tag dhcp): NetBox hat kein Prefix {}, aus dem der Pool käme",
            p("interface"),
            p("net")
        ),
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
        "PORT_IN_BRIDGES" => port_in_bridges(problem),
        "MTU_MISMATCH" => format!(
            "{} hat MTU {}, das andere Ende des Kabels, {}, {}: OSPF braucht auf beiden dieselbe",
            p("port"),
            p("mtu"),
            p("farEnd"),
            p("farMtu")
        ),
        "SPEED_NOT_SUPPORTED" => format!(
            "{} hat in NetBox eine Geschwindigkeit, die nur Ethernet-Ports fest bekommen",
            p("port")
        ),
        "SPEED_NEEDS_CABLE_TYPE" => format!(
            "{} läuft in NetBox fest mit {}: sein Kabel braucht einen Typ (DAC, Glasfaser oder \
             Kupfer), RouterOS nennt die Geschwindigkeit mit dem Medium",
            p("port"),
            p("speed")
        ),
        "UNKNOWN_SPEED" => format!(
            "{} läuft in NetBox fest mit {}: dafür hat RouterOS an diesem Port mit diesem \
             Kabeltyp keine Geschwindigkeit",
            p("port"),
            p("speed")
        ),
        "SPEED_NOT_ON_PORT" => format!(
            "{} läuft in NetBox fest mit {}, das bietet das Board an diesem Port nicht",
            p("port"),
            p("speed")
        ),
        "SPEED_MISMATCH" => {
            let far = match p("farSpeed") {
                "" => "handelt die Geschwindigkeit aus".to_string(),
                speed => format!("läuft mit {speed}"),
            };
            format!(
                "{} läuft in NetBox fest mit {}, das andere Ende des Kabels, {}, {far}: ohne \
                 dieselbe feste Geschwindigkeit an beiden Enden kommt kein Link",
                p("port"),
                p("speed"),
                p("farEnd")
            )
        }
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
