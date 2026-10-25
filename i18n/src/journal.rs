//! A job's entry in the device's journal in NetBox (`backend::journal`), in Markdown: what
//! people read in NetBox about what the provisioner did there.

/// How the job stands or ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Done,
    /// done, but not everything is applied (a fallback fired or stays, something left in the
    /// diff)
    Incomplete,
    Failed,
    Cancelled,
    /// the provisioner restarted before the job ended
    Interrupted,
}

/// The first line: what and how it stands, e.g. „**Anwenden** abgeschlossen“.
pub fn heading(what: &str, outcome: Outcome) -> String {
    format!("**{what}** {}", how(outcome))
}

/// How the job stands or ended, after its kind.
pub fn how(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Running => "läuft",
        Outcome::Done => "abgeschlossen",
        Outcome::Incomplete => "abgeschlossen, aber nicht alles angewendet",
        Outcome::Failed => "fehlgeschlagen",
        Outcome::Cancelled => "abgebrochen",
        Outcome::Interrupted => "unterbrochen: der Provisioner wurde neu gestartet",
    }
}

pub fn started_by(user: &str) -> String {
    format!("Gestartet von {user}")
}
pub const RESUMED: &str = "Nach einem Neustart des Provisioners weitergeführt";

pub fn took(seconds: u64) -> String {
    match seconds {
        0..60 => format!("Dauer: {seconds} s"),
        _ => format!("Dauer: {} min {} s", seconds / 60, seconds % 60),
    }
}

/// `state` worded by `job::phase`
pub fn state(state: &str, running: bool) -> String {
    match running {
        true => crate::job::phase::running(state),
        false => crate::job::phase::ended(state),
    }
}

pub fn error(text: &str) -> String {
    format!("Fehler: {text}")
}

/// the other devices of a batch
pub fn together(devices: &[String]) -> String {
    format!("Gemeinsam angewendet mit {}", crate::common::list(devices))
}

pub fn update(from: &str, to: &str) -> String {
    format!("RouterOS {from} → {to}")
}

pub fn power_cycled(device: &str, port: &str) -> String {
    format!("Strom an {port} für {device} aus und ein")
}

pub fn netbox_written(objects: usize) -> String {
    format!("{objects} Objekte in NetBox geschrieben")
}

/// What the job leaves on the device, which acts by itself unless someone takes it away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    ApplyFallback,
    UpdateFallback,
    WatchdogSafetyNet,
}

fn what(left: Left) -> &'static str {
    match left {
        Left::ApplyFallback => "der Fallback des Anwendens (lädt die Konfiguration von vorher)",
        Left::UpdateFallback => "der Fallback des Updates (bootet die alte Partition)",
        Left::WatchdogSafetyNet => "das Sicherheitsnetz der Watchdogs (stellt sie nach 2 h selbst wieder her)",
    }
}

pub fn left(left: &[Left]) -> String {
    format!("Bleibt auf dem Gerät: {}", items(left))
}

/// The heading of an entry the device page wrote without a job (`backend::journal::observe`).
pub const OBSERVED: &str = "**Beobachtung** auf dem Gerät, ohne Job";

fn items(left: &[Left]) -> String {
    let items: Vec<String> = left.iter().map(|left| what(*left).to_string()).collect();
    crate::common::list(&items)
}

/// a fallback a job left is gone and the device booted since
pub fn fired(left: &[Left]) -> String {
    format!("Ausgelöst, ohne dass ein Job es sah (das Gerät ist seither neu gestartet): {}", items(left))
}
/// on the device, but no job left it
pub fn found(left: &[Left]) -> String {
    format!(
        "Auf dem Gerät, ohne dass ein Eintrag es nennt (eine andere Instanz, ein älterer Stand oder \
         von Hand): {}",
        items(left)
    )
}
/// a job left it, it is gone without a reboot since
pub fn removed(left: &[Left]) -> String {
    format!("Nicht mehr da, ohne Neustart seither (von Hand entfernt oder abgelaufen): {}", items(left))
}

/// The device page's „Verlauf“ (`components::history`): the entries from the journal.
pub mod history {
    pub const TITLE: &str = "Verlauf";
    pub const NONE: &str = "Noch keine Einträge des Provisioners im Journal des Geräts in NetBox.";
    pub const IN_NETBOX: &str = "im Journal in NetBox";
    pub const OBSERVED: &str = "Beobachtung auf dem Gerät, ohne Job";
    pub fn by(user: &str) -> String {
        format!("von {user}")
    }
}
