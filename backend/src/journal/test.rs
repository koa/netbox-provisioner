use super::{Entry, Started, interrupted, outcome, phase_name};
use crate::{
    device::{batch::BatchApply, ros::graphql as apply, update::UpdateDone},
    device_state::{ApplyState, UpdateState},
    error_info::{ErrorInfo, ErrorParam},
    job::{Event, JobKind, JobState},
};
use i18n::journal::{Left, Outcome};
use std::time::Duration;

fn armed() -> Event {
    Event::FallbackArmed(apply::FallbackArmed {
        delay: Duration::from_secs(210).into(),
        window: None,
        radio_wait: None,
    })
}
fn stopped() -> Event {
    Event::FallbackStopped(apply::FallbackStopped { after: Duration::from_secs(40).into() })
}
fn checked(remaining: &str) -> Event {
    Event::ApplyChecked(apply::ApplyChecked { remaining: remaining.into(), error: None })
}

fn entry(kind: JobKind, outcome: Outcome, events: Vec<Event>) -> Entry {
    Entry {
        kind,
        started: Started::By("koa".into()),
        outcome,
        phase: None,
        error: None,
        took: None,
        events,
        device: "lab-core".into(),
    }
}

#[test]
fn phase_names_are_graphqls() {
    assert_eq!(phase_name(ApplyState::ScriptRunning.into()), "SCRIPT_RUNNING");
    assert_eq!(phase_name(UpdateState::WaitingToRebootFirmware.into()), "WAITING_TO_REBOOT_FIRMWARE");
    assert_eq!(phase_name(ApplyState::Planning.into()), "PLANNING");
}

#[test]
fn a_fallback_stopped_leaves_nothing() {
    let events = vec![armed(), stopped(), checked("")];
    assert!(super::left(&events).is_empty());
    assert_eq!(outcome(JobState::Done, &events), Outcome::Done);
}

#[test]
fn a_fallback_not_stopped_is_left() {
    let events = vec![armed(), checked("")];
    assert_eq!(super::left(&events), vec![Left::ApplyFallback]);
    assert_eq!(outcome(JobState::Done, &events), Outcome::Incomplete);
    assert_eq!(outcome(JobState::Cancelled, &events), Outcome::Cancelled);
}

#[test]
fn a_part_left_in_the_diff_is_incomplete() {
    let events = vec![armed(), stopped(), checked("/ip address add …")];
    assert_eq!(outcome(JobState::Done, &events), Outcome::Incomplete);
}

#[test]
fn a_running_entry_says_its_state() {
    let mut running = entry(JobKind::Apply, Outcome::Running, vec![armed()]);
    running.phase = Some(ApplyState::ScriptRunning.into());
    assert_eq!(
        running.comments(),
        "**Anwenden** läuft\n\n\
         Gestartet von koa\n\n\
         Stand: Skript läuft\n\n\
         Bleibt auf dem Gerät: der Fallback des Anwendens (lädt die Konfiguration von vorher)"
    );
    let record = running.record();
    assert_eq!(record["job"], "Apply");
    assert_eq!(record["outcome"], "Running");
    assert_eq!(record["state"], "SCRIPT_RUNNING");
    assert_eq!(record["left"], serde_json::json!(["ApplyFallback"]));
}

#[test]
fn a_finished_entry_has_its_results() {
    let mut done = entry(
        JobKind::Apply,
        Outcome::Done,
        vec![
            Event::BatchApply(BatchApply { devices: vec!["lab-core".into(), "lab-np1".into()] }),
            armed(),
            stopped(),
            checked(""),
        ],
    );
    done.started = Started::Resumed { entry: Some(7) };
    done.phase = Some(ApplyState::Finishing.into());
    done.took = Some(75);
    assert_eq!(
        done.comments(),
        "**Anwenden** abgeschlossen\n\n\
         Nach einem Neustart des Provisioners weitergeführt\n\n\
         Gemeinsam angewendet mit lab-np1\n\n\
         Angewendet, das Gerät entspricht jetzt NetBox\n\n\
         Dauer: 1 min 15 s"
    );
}

#[test]
fn a_failed_entry_words_its_error() {
    let mut failed = entry(
        JobKind::Update,
        Outcome::Failed,
        vec![Event::UpdateDone(UpdateDone { from: "7.23.8".into(), to: "7.24.5".into() })],
    );
    failed.phase = Some(UpdateState::WaitingForNeighbors.into());
    failed.error = Some(ErrorInfo {
        code: "TIMEOUT",
        params: vec![ErrorParam { name: "address", value: "192.168.100.3".into() }],
        message: "timeout".into(),
    });
    assert_eq!(
        failed.comments(),
        "**RouterOS-Update** fehlgeschlagen\n\n\
         Gestartet von koa\n\n\
         Endete in: Wartet auf die OSPF-Nachbarn\n\n\
         Fehler: Keine Antwort von 192.168.100.3\n\n\
         RouterOS 7.23.8 → 7.24.5"
    );
}

#[test]
fn an_interrupted_entry_keeps_its_state() {
    let mut running = entry(JobKind::Apply, Outcome::Running, vec![armed()]);
    running.phase = Some(ApplyState::ScriptRunning.into());
    let record = running.record().as_object().unwrap().clone();
    let fields = interrupted(&running.comments(), record);
    assert_eq!(fields["kind"], "warning");
    assert_eq!(
        fields["comments"],
        "**Anwenden** unterbrochen: der Provisioner wurde neu gestartet\n\n\
         Gestartet von koa\n\n\
         Stand: Skript läuft\n\n\
         Bleibt auf dem Gerät: der Fallback des Anwendens (lädt die Konfiguration von vorher)"
    );
    let record = &fields["custom_fields"]["provisioner_job"];
    assert_eq!(record["outcome"], "Interrupted");
    assert_eq!(record["state"], "SCRIPT_RUNNING");
    assert_eq!(record["left"], serde_json::json!(["ApplyFallback"]));
}
