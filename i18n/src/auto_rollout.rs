//! Scope: The texts of a site's automatic rollout card (`components::site::auto_rollout`);
//! nothing else.

pub const TITLE: &str = "Automatisches Rollout";
pub const NOT_SEEN: &str =
    "Noch keine Runde hat die Site gesehen; die erste prüft, ob ihre Geräte wie in NetBox sind.";
pub const READY: &str = "Bereit: Änderungen in NetBox gehen ohne Anwenden von Hand auf die Geräte.";
pub const CLEAR: &str = "Fehler aufheben und ausrollen";
pub const PREVIEW_CLEAR: &str = "Fehler aufheben …";
pub const PREVIEW_TITLE: &str = "Fehler aufheben: was ausrollt";
pub const PREVIEW_HINT: &str = "Wie weit jedes Gerät der Site von NetBox ist, frisch gelesen. Nach \
    dem Aufheben rollt gleich eine Runde die automatischen Geräte aus; die manuellen bleiben, \
    bis ein Admin sie anwendet. Aufheben geht, sobald alle Geräte gelesen sind.";
pub const CANCEL: &str = "Abbrechen";
pub const RELEASE: &str = "Freigeben und mit ausrollen";
pub const RELEASED: &str = "Freigegeben, kommt mit der nächsten Runde";
pub const ACCEPT: &str = "Als Stand übernehmen";
pub const ACCEPTED: &str = "Als Stand übernommen, bleibt auf dem Gerät";

/// Since how long, from seconds.
pub fn ago(seconds: i64) -> String {
    match seconds {
        ..60 => "gerade eben".into(),
        60..3600 => format!("vor {} min", seconds / 60),
        3600..86400 => format!("vor {} h", seconds / 3600),
        _ => format!("vor {} Tagen", seconds / 86400),
    }
}

/// Why the site is in its error state (`SiteError`), the devices it is about, since when.
pub enum SiteError<'a> {
    NotInLine,
    ApplyFailed,
    Other(&'a str),
}

pub fn error(reason: SiteError, devices: &[String], ago: &str) -> String {
    let devices = devices.join(", ");
    match reason {
        SiteError::NotInLine => format!(
            "Fehler ({ago}): beim Einschalten nicht wie in NetBox ({devices}). Aufheben rollt \
             diese Abweichungen aus; was von Hand auf den Geräten ist, geht dabei verloren."
        ),
        SiteError::ApplyFailed => format!(
            "Fehler ({ago}): das Rollout scheiterte an {devices}, alle Geräte luden ihre \
             Konfiguration von vorher. Bis zum Aufheben rollt hier nichts automatisch aus; \
             einzeln anwenden geht weiterhin."
        ),
        SiteError::Other(other) => format!("Fehler ({ago}): {other} ({devices})"),
    }
}

/// What the last round did (`RoundOutcome`).
pub enum Outcome<'a> {
    NothingToDo,
    NotInLine,
    Applied,
    HeldBack,
    NoInternet,
    Busy,
    Failed,
    NetboxUnreadable,
    PreconditionUnmet,
    Other(&'a str),
}

pub fn last_round(outcome: Outcome, ago: &str, applied: &[String]) -> String {
    let applied = applied.join(", ");
    let what = match outcome {
        Outcome::NothingToDo => "nichts zu tun".to_string(),
        Outcome::NotInLine => {
            "nichts angewendet, eine eingeschaltete Site war nicht wie in NetBox".to_string()
        }
        Outcome::Applied => format!("ausgerollt auf {applied}"),
        Outcome::HeldBack => {
            "angehalten, bis ein Admin die Geräte unten freigibt oder von Hand anwendet".to_string()
        }
        Outcome::NoInternet => format!(
            "nichts angewendet, ein Gerät erreichte das Internet nicht über IPv4 und IPv6 \
             ({applied}); die nächste Runde versucht es wieder"
        ),
        Outcome::Busy => format!(
            "nichts angewendet, ein Gerät war mit einem anderen Auftrag beschäftigt ({applied})"
        ),
        Outcome::Failed => format!("gescheitert auf {applied}, alle gingen zurück"),
        Outcome::NetboxUnreadable => "NetBox war nicht lesbar".to_string(),
        Outcome::PreconditionUnmet => {
            "nichts angewendet, die Vorbedingung ist verletzt".to_string()
        }
        Outcome::Other(other) => other.to_string(),
    };
    format!("Letzte Runde {ago}: {what}")
}

pub fn deferred(devices: &[String]) -> String {
    format!(
        "Wartet (nicht erreichbar, oder das andere Ende einer Funkstrecke nicht): {}",
        devices.join(", ")
    )
}

pub const HELD_BY: &str = "Diese manuellen Geräte haben eine neue Abweichung von NetBox; die \
    Automatik wartet, bis sie von Hand angewendet, freigegeben (ihre Abweichung geht mit hinaus) \
    oder als Stand übernommen sind (sie bleibt auf dem Gerät, erst eine weitere Änderung hält \
    wieder an):";

pub const UNMET: &str = "Ausgesetzt: Die Vorbedingung ist verletzt, an keiner automatischen \
    Site rollt etwas aus. Sobald sie wieder erfüllt ist, läuft die Automatik von selbst weiter. \
    Es fehlt:";

/// What an automatic device lacks for the precondition (`Lack`).
pub enum Lack<'a> {
    LoopbackIpv4,
    LoopbackIpv6,
    Connection,
    Scheduler,
    Other(&'a str),
}

pub fn lack(lack: Lack) -> String {
    match lack {
        Lack::LoopbackIpv4 => "Loopback IPv4 (/32 auf lo)".into(),
        Lack::LoopbackIpv6 => "Loopback IPv6 (/128 auf lo)".into(),
        Lack::Connection => {
            "OSPF-Verbindung zu den übrigen automatischen Geräten (ein ospf-Interface im selben \
             Netz wie ein Nachbar)"
                .into()
        }
        Lack::Scheduler => "Scheduler im Device-Mode (für den Fallback des Anwendens; \
            einschalten mit „Device-Mode“ auf der Geräteseite und Bestätigung am Gerät)"
            .into(),
        Lack::Other(other) => other.into(),
    }
}

pub const HELD: &str = "Diese automatischen Geräte haben Änderungen, die ein Admin freigeben \
    muss (oder von Hand anwendet); bis dahin rollt nichts aus:";

/// Why an automatic device's changes wait for an admin (`HoldReason`).
pub enum HoldReason<'a> {
    PoeCut,
    CapOff,
    ApiClientsOutside,
    RadioLink,
    Rewire,
    Outward,
    Other(&'a str),
}

pub fn hold_reason(reason: HoldReason) -> String {
    match reason {
        HoldReason::PoeCut => "schaltet PoE an einem Port ab, der etwas speist".into(),
        HoldReason::CapOff => "seine Radios senden danach kein WLAN".into(),
        HoldReason::ApiClientsOutside => {
            "sperrt die API für einen Client, der jetzt verbunden ist".into()
        }
        HoldReason::RadioLink => {
            "ändert ein Radio an einer Funkstrecke: sie und alles dahinter kann bis zu 24 min \
             weg sein"
                .into()
        }
        HoldReason::Rewire => {
            "ein Kabel ist in NetBox an einem anderen Port, als das Gerät es sieht: geht von \
             selbst, sobald es umgesteckt ist"
                .into()
        }
        HoldReason::Outward => {
            "ändert, was OSPF nach aussen ankündigt (an einem Uplink, die Default-Route oder \
             eine Adresse ausserhalb der Aggregates von NetBox)"
                .into()
        }
        HoldReason::Other(other) => other.into(),
    }
}

pub const OPEN_CHANGES: &str = "Änderungen, die noch nicht überall sind:";

/// How far a change has reached a device.
pub enum RolloutState {
    Pending,
    RolledOut,
    Waiting,
    WaitingForAdmin,
    Deferred,
    Failed,
    Manual,
}

/// How far a change has reached a device, for an admin.
pub fn rollout_state(state: RolloutState) -> &'static str {
    match state {
        RolloutState::Pending => "kommt mit der nächsten Runde",
        RolloutState::RolledOut => "ausgerollt",
        RolloutState::Waiting => "wartet, die Runde hält an",
        RolloutState::WaitingForAdmin => "wartet auf Freigabe",
        RolloutState::Deferred => "zurückgestellt, nicht erreicht",
        RolloutState::Failed => "gescheitert, Fehlerzustand der Site",
        RolloutState::Manual => "manuelles Gerät, von Hand anwenden",
    }
}

/// A change and who saved it, for an admin.
pub fn change_head(ago: &str, by: &str) -> String {
    format!("{ago} von {by}")
}
