//! A job's entry in its device's journal in NetBox (`docs/protocol.md`): created when the job
//! starts, written again at each state it enters and at its end, so NetBox tells how far a job
//! came after the provisioner forgot it. Written whenever `netbox_write_token` is set; a write
//! that fails never stops the job, it emits [`JournalNotWritten`] and the next state tries
//! again. The text is German from the crate `i18n`; what the provisioner reads back is in the
//! custom field `provisioner_job`.
use crate::{
    Error,
    config::CONFIG,
    device_state::JobPhase,
    error_info::ErrorInfo,
    job::{Event, Job, JobKind, JobState},
    netbox::{NetboxError, rest::NetboxRest},
    topology::DeviceId,
};
use async_graphql::SimpleObject;
use i18n::journal::{Left, Outcome};
use log::warn;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

/// The tag of the provisioner's entries.
pub const TAG: &str = "netbox-provisioner";
/// How many of its entries the provisioner keeps per device; older ones go after a job's end.
pub const KEEP: usize = 20;

/// Who started a job, for its entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Started {
    /// the name or email of the login
    By(Box<str>),
    /// went on after a restart of the provisioner
    Resumed,
}

/// The job's entry in NetBox' journal couldn't be written; the next state tries again.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct JournalNotWritten {
    pub error: ErrorInfo,
}

/// Whether entries are written at all; never in the unit tests, which have no `config.yaml`.
pub fn enabled() -> bool {
    !cfg!(test) && CONFIG.netbox_write_token.is_some()
}

/// Writes the entry of `job` on `device` each time `changes` ticks, until the job has ended;
/// then drops the device's oldest entries beyond [`KEEP`].
pub async fn follow(job: Arc<Job>, device: DeviceId, started: Started, mut changes: watch::Receiver<()>) {
    let rest = match NetboxRest::journal(format!("netbox-provisioner job {}", job.job_id())) {
        Ok(rest) => rest,
        Err(error) => return not_written(&job, error),
    };
    let mut id = None;
    let mut failing = false;
    loop {
        changes.borrow_and_update();
        let entry = Entry::of(&job, &started);
        match save(&rest, device, &mut id, &entry).await {
            Ok(()) => failing = false,
            Err(error) => {
                if !failing {
                    not_written(&job, error);
                }
                failing = true;
            }
        }
        if entry.outcome != Outcome::Running || changes.changed().await.is_err() {
            break;
        }
    }
    if id.is_some()
        && let Err(error) = prune(&rest, device).await
    {
        warn!("{}: old journal entries not removed: {error}", job.name());
    }
}

fn not_written(job: &Job, error: NetboxError) {
    warn!("{}: journal entry not written: {error}", job.name());
    job.log().emit(JournalNotWritten {
        error: ErrorInfo::from(&Error::Netbox(error)),
    });
}

async fn save(rest: &NetboxRest, device: DeviceId, id: &mut Option<u64>, entry: &Entry) -> Result<(), NetboxError> {
    match id {
        Some(id) => rest.update(&format!("extras/journal-entries/{id}/"), entry.fields()).await.map(|_| ()),
        None => {
            let mut fields = entry.fields();
            fields["assigned_object_type"] = json!("dcim.device");
            fields["assigned_object_id"] = json!(device.0);
            fields["tags"] = json!([{ "slug": TAG }]);
            let created = rest.create("extras/journal-entries/", fields).await?;
            *id = created["id"].as_u64();
            Ok(())
        }
    }
}

/// Deletes the provisioner's entries on `device` beyond the newest [`KEEP`].
async fn prune(rest: &NetboxRest, device: DeviceId) -> Result<(), NetboxError> {
    let entries = rest
        .get(&format!(
            "extras/journal-entries/?assigned_object_type=dcim.device&assigned_object_id={}&tag={TAG}\
             &ordering=-created&limit=1000",
            device.0
        ))
        .await?;
    let old = entries["results"].as_array().into_iter().flatten().skip(KEEP);
    for id in old.filter_map(|entry| entry["id"].as_u64()) {
        rest.delete(&format!("extras/journal-entries/{id}/")).await?;
    }
    Ok(())
}

/// What an entry says, taken from the job as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: JobKind,
    pub started: Started,
    pub outcome: Outcome,
    /// the state it is in or ended in
    pub phase: Option<JobPhase>,
    pub error: Option<ErrorInfo>,
    pub took: Option<u64>,
    pub events: Vec<Event>,
    /// the job's device, left out of the batch it was applied with
    pub device: Box<str>,
}

impl Entry {
    fn of(job: &Job, started: &Started) -> Entry {
        let (state, error) = job.outcome();
        let events = job.log().events();
        Entry {
            kind: job.job_kind(),
            started: started.clone(),
            outcome: outcome(state, &events),
            phase: job.log().phase(),
            error,
            took: job.ran_for().map(|took| took.as_secs()),
            events,
            device: job.name().into(),
        }
    }

    /// NetBox' `kind` of the entry.
    fn netbox_kind(&self) -> &'static str {
        match self.outcome {
            Outcome::Running => "info",
            Outcome::Done => "success",
            Outcome::Incomplete | Outcome::Cancelled | Outcome::Interrupted => "warning",
            Outcome::Failed => "danger",
        }
    }

    /// What it leaves on the device, by its events.
    pub fn left(&self) -> Vec<Left> {
        left(&self.events)
    }

    /// The fields to create or update the entry with.
    fn fields(&self) -> Value {
        json!({
            "kind": self.netbox_kind(),
            "comments": self.comments(),
            "custom_fields": { "provisioner_job": self.record() },
        })
    }

    /// What the provisioner reads back (`provisioner_job`).
    pub fn record(&self) -> Value {
        json!({
            "job": format!("{:?}", self.kind),
            "outcome": format!("{:?}", self.outcome),
            "state": self.phase.map(phase_name),
            "left": self.left().iter().map(|left| format!("{left:?}")).collect::<Vec<_>>(),
            "at": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        })
    }

    /// The text people read, in Markdown: a paragraph per line.
    pub fn comments(&self) -> String {
        use i18n::journal as t;
        let mut lines = vec![t::heading(kind_name(self.kind), self.outcome)];
        lines.push(match &self.started {
            Started::By(user) => t::started_by(user),
            Started::Resumed => t::RESUMED.into(),
        });
        if self.outcome != Outcome::Done
            && let Some(phase) = self.phase
        {
            let name = phase_name(phase);
            let state = i18n::job::phase::of(&name).map_or(name.clone(), str::to_string);
            lines.push(t::state(&state, self.outcome == Outcome::Running));
        }
        if let Some(error) = &self.error {
            lines.push(t::error(&i18n::error::server(error)));
        }
        lines.extend(self.results());
        let left = self.left();
        if !left.is_empty() {
            lines.push(t::left(&left));
        }
        if let Some(took) = self.took {
            lines.push(t::took(took));
        }
        lines.join("\n\n")
    }

    /// The results worth keeping, by the events: not each step, the job view has those.
    fn results(&self) -> Vec<String> {
        use i18n::journal as t;
        let mut results = Vec::new();
        let mut written = 0;
        for event in &self.events {
            match event {
                Event::BatchApply(batch) => {
                    let others: Vec<String> = batch
                        .devices
                        .iter()
                        .filter(|device| **device != self.device)
                        .map(|device| device.to_string())
                        .collect();
                    results.push(t::together(&others));
                }
                Event::ApplyPlanned(planned) if planned.script.trim().is_empty() => {
                    results.push(i18n::job::NOTHING_TO_APPLY.into())
                }
                Event::UpdateDone(done) => results.push(t::update(&done.from, &done.to)),
                Event::Downgraded(done) => results.push(t::update(&done.from, &done.to)),
                Event::PowerCycled(cycled) => results.push(t::power_cycled(&cycled.device, &cycled.port)),
                Event::ApplyChecked(checked) if checked.error.is_none() => results.push(
                    match checked.remaining.is_empty() {
                        true => i18n::job::APPLIED,
                        false => i18n::job::NOT_ALL_APPLIED,
                    }
                    .into(),
                ),
                Event::FallbackFired(fired) => {
                    results.push(i18n::job::event::fallback_fired(Duration::from(fired.after).as_secs()))
                }
                Event::NetboxWritten(_) => written += 1,
                _ => {}
            }
        }
        if written > 0 {
            results.push(t::netbox_written(written));
        }
        results
    }
}

/// How the job stands: done but not all of it applied (a part left in the diff, a fallback
/// that fired or stays) is `Incomplete`.
fn outcome(state: JobState, events: &[Event]) -> Outcome {
    match state {
        JobState::Running => Outcome::Running,
        JobState::Failed => Outcome::Failed,
        JobState::Cancelled => Outcome::Cancelled,
        JobState::Done => {
            let incomplete = events.iter().any(|event| match event {
                Event::ApplyChecked(checked) => !checked.remaining.is_empty() || checked.error.is_some(),
                Event::FallbackFired(_) | Event::BatchIncomplete(_) => true,
                _ => false,
            });
            match incomplete || !left(events).is_empty() {
                true => Outcome::Incomplete,
                false => Outcome::Done,
            }
        }
    }
}

/// What `events` leave on the device: a fallback armed and neither stopped nor fired, the
/// watchdogs' safety net where they stay suspended.
fn left(events: &[Event]) -> Vec<Left> {
    let mut apply = false;
    let mut update = false;
    let mut safety_net = false;
    for event in events {
        match event {
            Event::FallbackArmed(_) => apply = true,
            Event::FallbackStopped(_) | Event::FallbackFired(_) => apply = false,
            Event::UpdateFallbackArmed(_) => update = true,
            Event::UpdateFallbackDisarmed(_) => update = false,
            Event::WatchdogKept(_) | Event::WatchdogNotRestored(_) => safety_net = true,
            _ => {}
        }
    }
    [(apply, Left::ApplyFallback), (update, Left::UpdateFallback), (safety_net, Left::WatchdogSafetyNet)]
        .into_iter()
        .filter_map(|(is, left)| is.then_some(left))
        .collect()
}

fn kind_name(kind: JobKind) -> &'static str {
    use i18n::job as t;
    match kind {
        JobKind::Update => t::UPDATE,
        JobKind::Downgrade => t::DOWNGRADE,
        JobKind::Apply => t::APPLY,
        JobKind::Cleanup => t::CLEANUP,
        JobKind::Bootstrap => t::BOOTSTRAP,
        JobKind::Reboot => t::REBOOT,
        JobKind::PowerCycle => t::POWER_CYCLE,
        JobKind::DeviceMode => t::DEVICE_MODE,
        JobKind::Repartition => t::REPARTITION,
        JobKind::BootPartition => t::BOOT_PARTITION,
        JobKind::NetboxWrite => t::NETBOX_WRITE,
        JobKind::NetboxUndo => t::NETBOX_UNDO,
        JobKind::SiteDiff => t::SITE_DIFF,
    }
}

/// The state's GraphQL name (`SCRIPT_RUNNING`), as `i18n::job::phase::of` takes it.
fn phase_name(phase: JobPhase) -> String {
    let debug = format!("{phase:?}");
    let state = debug
        .split_once('(')
        .map_or(debug.as_str(), |(_, state)| state.trim_end_matches(')'));
    let mut name = String::new();
    for (index, char) in state.char_indices() {
        if char.is_uppercase() && index > 0 {
            name.push('_');
        }
        name.push(char.to_ascii_uppercase());
    }
    name
}

#[cfg(test)]
mod test;
