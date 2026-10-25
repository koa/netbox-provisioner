//! A job of the backend (an update, an apply, a setup): its events while it runs, asked for every second, worded
//! by `i18n::job`; with a button to cancel it.
use crate::{
    error::{FrontendError, ServerProblem},
    i18n::{common::CANCEL, job as t},
    graphql::{
        authenticated::{
            CancelJob, JobEvents, cancel_job,
            job_events::{
                self, FallbackLeftReason, JobEventsJob, JobEventsJobEvents,
                JobEventsJobEventsEvent as Event, JobKind, JobState, PowerWait, RebootReason,
            },
        },
        query_authenticated,
    },
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, CodeBlock, CodeBlockCode, Spinner, SpinnerSize,
};
use std::{cell::Cell, rc::Rc, time::Duration};
use yew::{
    Callback, Component, Context, Html, Properties, html, html::Scope, platform::spawn_local,
    platform::time::sleep,
};

/// How often a running job is asked for new events.
pub(crate) const POLL: Duration = Duration::from_secs(1);
/// How long to wait after the 1st, 2nd, … failed asking in a row; then the last one.
pub(crate) const RETRY: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

pub struct JobView {
    job: Option<JobEventsJob>,
    /// the last asking failed (the server is away, restarts); asked again until it answers
    error: Option<FrontendError>,
    /// the server doesn't know the job (anymore): it restarted meanwhile
    lost: bool,
    /// asking stops once the component is gone
    alive: Rc<Cell<bool>>,
    cancelling: bool,
}

#[derive(Properties, Clone, PartialEq)]
pub struct JobViewProps {
    pub id: i64,
    /// the job is over (done, failed or cancelled), `true` when it is done
    #[prop_or_default]
    pub on_finished: Callback<bool>,
}

pub enum JobMessage {
    Loaded(JobEventsJob),
    Error(FrontendError),
    /// the server answered without the job
    Lost,
    Cancel,
}

impl Component for JobView {
    type Message = JobMessage;
    type Properties = JobViewProps;

    fn create(ctx: &Context<Self>) -> Self {
        let alive = Rc::new(Cell::new(true));
        poll(ctx.link().clone(), ctx.props().id, alive.clone());
        JobView {
            job: None,
            error: None,
            lost: false,
            alive,
            cancelling: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            JobMessage::Loaded(mut loaded) => {
                if let Some(job) = self.job.take() {
                    let mut events = job.events;
                    events.append(&mut loaded.events);
                    loaded.events = events;
                }
                if loaded.state != JobState::RUNNING {
                    ctx.props().on_finished.emit(loaded.state == JobState::DONE);
                }
                self.job = Some(loaded);
                self.error = None;
            }
            JobMessage::Error(error) => self.error = Some(error),
            JobMessage::Lost => {
                self.lost = true;
                self.error = None;
                ctx.props().on_finished.emit(false);
            }
            JobMessage::Cancel => {
                self.cancelling = true;
                let scope = ctx.link().clone();
                let id = ctx.props().id;
                spawn_local(async move {
                    if let Err(error) =
                        query_authenticated::<CancelJob, _>(scope.clone(), cancel_job::Variables { id })
                            .await
                    {
                        scope.send_message(JobMessage::Error(error));
                    }
                });
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        // one line while the server is away, it is asked again
        let error = self
            .error
            .as_ref()
            .map(|e| html!(<p class="job-unreachable">{t::server_away(&e.message())}</p>));
        let Some(job) = &self.job else {
            if self.lost {
                return html!(<Alert inline=true title={t::LOST} r#type={AlertType::Warning}><p>{t::LOST_TEXT}{t::LOST_FALLBACK}</p></Alert>);
            }
            return html!(<>{error}<Spinner size={SpinnerSize::Md}/></>);
        };
        let fallback = matches!(job.kind, JobKind::APPLY | JobKind::UPDATE);
        let lost = self.lost.then(|| {
            html!(<p>{t::LOST_TEXT}{if fallback { t::LOST_FALLBACK } else { "" }}</p>)
        });
        let running = job.state == JobState::RUNNING && !self.lost;
        let (title, kind) = if self.lost {
            (t::LOST.to_string(), AlertType::Warning)
        } else {
            title(job)
        };
        html! {
            <Alert inline=true title={title} r#type={kind}>
                {error}
                {lost}
                if let Some(error) = &job.error { <p>{ problem(error) }</p> }
                <ul class="job-events">
                    { for items(&job.events) }
                </ul>
                if running {
                    <p>
                        <Spinner size={SpinnerSize::Md}/>{" "}
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={if self.cancelling { t::CANCELLING } else { CANCEL }}
                            disabled={self.cancelling}
                            onclick={ctx.link().callback(|_| JobMessage::Cancel)}
                        />
                    </p>
                }
            </Alert>
        }
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        self.alive.set(false);
    }
}

/// Asks for the job's new events until it is over, or the server doesn't know it anymore. While
/// asking fails it asks again, less often.
fn poll(scope: Scope<JobView>, id: i64, alive: Rc<Cell<bool>>) {
    spawn_local(async move {
        let mut after = 0;
        let mut failed = 0;
        while alive.get() {
            match query_authenticated::<JobEvents, _>(
                scope.clone(),
                job_events::Variables {
                    id,
                    after: Some(after),
                },
            )
            .await
            {
                Ok(job_events::ResponseData { job: Some(job) }) => {
                    failed = 0;
                    if let Some(last) = job.events.last() {
                        after = last.index + 1;
                    }
                    let over = job.state != JobState::RUNNING;
                    scope.send_message(JobMessage::Loaded(job));
                    if over {
                        return;
                    }
                }
                // the backend restarted and forgot it
                Ok(job_events::ResponseData { job: None }) => {
                    scope.send_message(JobMessage::Lost);
                    return;
                }
                Err(error) => {
                    scope.send_message(JobMessage::Error(error));
                    sleep(RETRY[failed.min(RETRY.len() - 1)]).await;
                    failed += 1;
                    continue;
                }
            }
            sleep(POLL).await;
        }
    });
}

fn title(job: &JobEventsJob) -> (String, AlertType) {
    let what = match &job.kind {
        JobKind::UPDATE => t::UPDATE,
        JobKind::DOWNGRADE => t::DOWNGRADE,
        JobKind::APPLY => t::APPLY,
        JobKind::BOOTSTRAP => t::BOOTSTRAP,
        JobKind::REBOOT => t::REBOOT,
        JobKind::POWER_CYCLE => t::POWER_CYCLE,
        JobKind::DEVICE_MODE => t::DEVICE_MODE,
        JobKind::REPARTITION => t::REPARTITION,
        JobKind::BOOT_PARTITION => t::BOOT_PARTITION,
        JobKind::NETBOX_WRITE => t::NETBOX_WRITE,
        JobKind::NETBOX_UNDO => t::NETBOX_UNDO,
        JobKind::SITE_DIFF => t::SITE_DIFF,
        JobKind::Other(other) => other,
    };
    let events = || job.events.iter().map(|event| &event.event);
    if job.state == JobState::DONE {
        if events().any(|e| matches!(e, Event::ApplyPlanned(planned) if planned.script.trim().is_empty())) {
            return (t::NOTHING_TO_APPLY.into(), AlertType::Success);
        }
        let incomplete = events().any(|e| match e {
            Event::ApplyRunning(running) => running.error.is_some(),
            Event::ApplyStarted(started) => started.error.is_some(),
            Event::ApplyChecked(checked) => checked.error.is_some() || !checked.remaining.trim().is_empty(),
            Event::FallbackLeft(_) | Event::FallbackFired(_) => true,
            Event::ScriptProgress(progress) => progress.error.is_some() || !progress.done,
            _ => false,
        });
        if incomplete {
            return (t::NOT_ALL_APPLIED.into(), AlertType::Warning);
        }
        if events().any(|e| matches!(e, Event::ApplyChecked(_))) {
            return (t::APPLIED.into(), AlertType::Success);
        }
        if let Some(address) = events().find_map(|e| match e {
            Event::SetupDone(done) => Some(&done.address),
            _ => None,
        }) {
            return (t::set_up(address), AlertType::Success);
        }
    }
    match &job.state {
        JobState::RUNNING => (t::running(what), AlertType::Info),
        JobState::DONE => {
            let update = events().find_map(|event| match event {
                Event::UpdateDone(done) => Some((done.from.as_str(), done.to.as_str())),
                Event::Downgraded(done) => Some((done.from.as_str(), done.to.as_str())),
                _ => None,
            });
            (t::done(what, update), AlertType::Success)
        }
        JobState::FAILED => (t::failed(what), AlertType::Danger),
        JobState::CANCELLED => (t::cancelled(what), AlertType::Warning),
        JobState::Other(other) => (t::other_state(what, other), AlertType::Warning),
    }
}

/// `m:ss` since the start of the job.
fn elapsed(nanos: u64) -> String {
    let seconds = nanos / 1_000_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// An error the job or one of its events reports.
fn problem(problem: &job_events::Problem) -> String {
    ServerProblem::from(problem).text()
}

/// whole seconds
fn seconds(nanos: u64) -> u64 {
    nanos / 1_000_000_000
}

fn script(title: &str, script: &str) -> Html {
    html! {<>
        { title.to_string() }
        <CodeBlock><CodeBlockCode>{ script.to_string() }</CodeBlockCode></CodeBlock>
    </>}
}

/// One entry per event, consecutive console output in one.
fn items(events: &[JobEventsJobEvents]) -> Vec<Html> {
    // the lines the marks of the script count
    let planned = events.iter().rev().find_map(|event| match &event.event {
        Event::ApplyPlanned(planned) => Some(planned.script.as_str()),
        _ => None,
    });
    let mut items = Vec::new();
    let mut at = 0;
    while at < events.len() {
        let event = &events[at];
        let content = if let Event::ConsoleOutput(_) = event.event {
            let mut text = String::new();
            while let Some(Event::ConsoleOutput(output)) = events.get(at).map(|e| &e.event) {
                text.push_str(&output.text);
                at += 1;
            }
            console(&text)
        } else {
            at += 1;
            match &event.event {
                Event::ScriptProgress(progress) => script_progress(progress, planned),
                _ => describe(event),
            }
        };
        items.push(html!(<li>
            <span class="job-time">{ elapsed(event.at) }</span>
            <div>{ content }</div>
        </li>));
    }
    items
}

/// How far the script got: what it says, the lines it stopped at or lost the provisioner
/// after (with the two before, from `planned`), and the marks as the device has them.
fn script_progress(progress: &job_events::JobEventsJobEventsEventOnScriptProgress, planned: Option<&str>) -> Html {
    let lines = t::event::script_progress(t::event::Progress {
        lines: progress.lines as u32,
        section: progress.section.map(|s| s as u32),
        done: progress.done,
        error: progress
            .error_line
            .zip(progress.error.as_deref())
            .map(|(line, error)| (line as u32, error)),
        probe: progress.probe.as_deref().map(|probe| (probe, progress.probe_answered)),
        lost: &progress.lost,
        back: &progress.back,
        rearmed: progress.rearmed.map(seconds),
        after_fallback: progress.after_fallback,
    });
    let at: Vec<i64> = progress.error_line.iter().chain(&progress.lost).copied().collect();
    let shown = planned.map(|planned| {
        let script: Vec<&str> = planned.lines().collect();
        at.iter()
            .map(|&line| {
                let from = (line - 2).max(1);
                (from..=line)
                    .filter_map(|n| script.get(n as usize - 1).map(|text| format!("{n:>4}  {text}")))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n…\n")
    });
    html! {<>
        { for lines.into_iter().map(|line| html!(<p>{ line }</p>)) }
        if let Some(shown) = shown.filter(|shown| !shown.is_empty()) {
            <CodeBlock><CodeBlockCode>{ shown }</CodeBlockCode></CodeBlock>
        }
        <details>
            <summary>{ t::event::MARKS }</summary>
            <pre>{ progress.marks.clone() }</pre>
        </details>
    </>}
}

/// Scrolled to its end, where new output comes (`column-reverse` in `local.css`).
fn console(text: &str) -> Html {
    html!(<div class="job-console"><pre>{ text.to_string() }</pre></div>)
}

/// The words for what a watchdog watches; each event has its own type for it.
macro_rules! watching {
    ($watching:expr) => {{
        let ports = $watching
            .poe_pings
            .iter()
            .map(|ping| (ping.port.as_str(), ping.address.as_str()))
            .collect::<Vec<_>>();
        e::watching($watching.watch_address.as_deref(), &ports)
    }};
}

fn describe(event: &JobEventsJobEvents) -> Html {
    use t::event as e;
    let text = match &event.event {
        // a site's job, shown by `components::site_diff`, never on a device's page
        Event::SiteDiffStarted | Event::DeviceDiffed => return Html::default(),
        Event::ApplyPlanned(planned) if planned.script.trim().is_empty() => e::NOTHING_TO_DO.into(),
        Event::ApplyPlanned(planned) => return script(t::SCRIPT, &planned.script),
        Event::FallbackArmed(armed) => e::fallback_armed(
            seconds(armed.delay),
            armed.window.map(seconds),
            armed.radio_wait.map(seconds),
        ),
        Event::FallbackUnavailable(unavailable) => e::fallback_unavailable(&unavailable.device_mode),
        Event::ApplyRunning(running) => match &running.error {
            Some(error) => e::script_not_started(&problem(error)),
            None => e::script_running(seconds(running.timeout)),
        },
        Event::FallbackFired(fired) => e::fallback_fired(seconds(fired.after)),
        // with the script's lines, in `items`
        Event::ScriptProgress(_) => String::new(),
        Event::ApplyChecked(checked) => match &checked.error {
            Some(error) => e::not_checked(&problem(error)),
            None if checked.remaining.trim().is_empty() => e::IN_LINE.into(),
            None => return script(t::REMAINING, &checked.remaining),
        },
        Event::FallbackStopped(_) => e::FALLBACK_STOPPED.into(),
        Event::FallbackLeft(left) => e::fallback_left(
            match &left.reason {
                FallbackLeftReason::CANCELLED => e::FallbackLeft::Cancelled,
                FallbackLeftReason::NOT_REACHED => e::FallbackLeft::NotReached,
                FallbackLeftReason::NOT_STOPPED => {
                    e::FallbackLeft::NotStopped(left.error.as_ref().map(problem))
                }
                FallbackLeftReason::NOT_APPLIED => e::FallbackLeft::NotApplied,
                FallbackLeftReason::DUE => e::FallbackLeft::Due,
                FallbackLeftReason::BATCH_INCOMPLETE => e::FallbackLeft::BatchIncomplete,
                FallbackLeftReason::Other(other) => e::FallbackLeft::Other(other),
            },
            seconds(left.fires_in),
        ),
        Event::SetupStarted(setup) => e::setup_started(&setup.neighbor, &setup.neighbor_port, &setup.macs),
        Event::ConsoleOutput(output) => return console(&output.text),
        Event::WaitingForApi(wait) => e::waiting_for_api(seconds(wait.timeout), &wait.address),
        Event::SetupDone(done) => e::setup_done(&done.address),
        Event::BatchApply(batch) => e::batch_apply(&batch.devices),
        Event::BatchStopped(stopped) => e::batch_stopped(&stopped.device),
        Event::BatchIncomplete(incomplete) => {
            e::batch_incomplete(&incomplete.devices, &incomplete.not_applied)
        }
        Event::ApplyWritten(written) => e::apply_written(seconds(written.lead)),
        Event::ApplyStarted(started) => match &started.error {
            Some(error) => e::apply_not_started(&problem(error)),
            None => e::apply_started(seconds(started.starts_in)),
        },
        Event::RadiosReprovisioned(radios) => match &radios.error {
            None => e::radios_reprovisioned(radios.count, &radios.menu),
            Some(error) => e::radios_not_reprovisioned(&radios.menu, &problem(error)),
        },
        Event::Connecting(connecting) => e::connecting(&connecting.device),
        Event::Connected(connected) => e::connected(
            &connected.device,
            &connected.address,
            connected.neighbor.as_deref(),
            &connected.identity,
            connected.same_name,
        ),
        Event::UpdateTarget(target) => e::update_target(&target.version, target.channel.as_deref()),
        Event::WatchdogNotSet(not_set) => e::watchdog_not_set(&not_set.device),
        Event::PoeChain(chain) => e::poe_chain(&chain.suppliers, &chain.consumers),
        Event::WaitingForPower(wait) => match &wait.reason {
            PowerWait::SUPPLIER_REBOOTING => e::waiting_for_supplier(&wait.device),
            PowerWait::CONSUMER_UPDATING => e::waiting_for_consumer(&wait.device),
            PowerWait::Other(other) => e::waiting_for(&wait.device, other),
        },
        Event::UpdateStart(start) => e::update_start(&start.version, &start.architecture, &start.packages),
        Event::UpdateStepPlanned(step) => e::update_step(&step.version, &step.files, &step.uninstall),
        Event::SshPasswordLogin(ssh) if ssh.restored => e::ssh_restored(&ssh.setting, &ssh.value),
        Event::SshPasswordLogin(ssh) => e::ssh_for_upload(&ssh.setting, &ssh.value),
        Event::PackageDownloaded(package) => e::downloaded(&package.file, package.bytes),
        Event::PackageUploading(package) => e::uploading(&package.file, package.bytes, package.total),
        Event::PackageUploaded(package) => e::uploaded(&package.file),
        Event::Reboot(reboot) => e::reboot(
            match &reboot.reason {
                RebootReason::PACKAGES => e::Reboot::Packages,
                RebootReason::FIRMWARE => e::Reboot::Firmware,
                RebootReason::REQUESTED => e::Reboot::Requested,
                RebootReason::DEVICE_MODE => e::Reboot::DeviceMode,
                RebootReason::REPARTITION => e::Reboot::Repartition,
                RebootReason::PARTITION => e::Reboot::Partition,
                RebootReason::DOWNGRADE => e::Reboot::Downgrade,
                RebootReason::Other(other) => e::Reboot::Other(other),
            },
            reboot.timeout / 60_000_000_000,
        ),
        Event::RouterOsRunning(running) => e::routeros_running(&running.version),
        Event::FirmwareUpgrade(firmware) if firmware.running => e::firmware_running(&firmware.upgrade),
        Event::FirmwareUpgrade(firmware) => e::firmware_upgrade(&firmware.current, &firmware.upgrade),
        Event::UpdateDone(done) => e::update_done(&done.from, &done.to),
        Event::Downgraded(done) => e::downgraded(&done.from, &done.to),
        Event::DeviceModeConfirm(confirm) => e::device_mode_confirm(
            &features(confirm.changes.iter().map(|c| (c.feature.as_str(), &c.reason))),
            confirm.timeout / 60_000_000_000,
        ),
        Event::PowerCycled(cycled) => {
            e::power_cycled(&cycled.supplier, &cycled.port, &cycled.device, cycled.seconds)
        }
        Event::PartitionRunning(running) => e::partition_running(&running.name, &running.version),
        Event::UpdateFallbackArmed(armed) => {
            e::update_fallback_armed(&armed.partition, &armed.version, seconds(armed.delay) / 60)
        }
        Event::UpdateFallbackDisarmed(disarmed) => e::update_fallback_disarmed(&disarmed.partition),
        Event::UpdateWithoutFallback(without) => e::update_without_fallback(without.partitions, &without.neighbors),
        Event::OspfNeighborsBack(back) => e::ospf_neighbors_back(&back.neighbors),
        Event::WatchdogSuspended(suspended) => e::watchdog_suspended(
            &suspended.device,
            &watching!(suspended.watching),
            seconds(suspended.safety_net) / 60,
        ),
        Event::WatchdogRestored(restored) => {
            e::watchdog_restored(&restored.device, &watching!(restored.watching))
        }
        Event::WatchdogNotRestored(failed) => e::watchdog_not_restored(
            &failed.device,
            &watching!(failed.watching),
            &problem(&failed.error),
        ),
        Event::WatchdogLeft(left) => e::watchdog_left(&left.device, &problem(&left.error)),
        Event::WatchdogKept(kept) => e::watchdog_kept(
            &kept.device,
            &watching!(kept.watching),
            seconds(kept.safety_net) / 60,
        ),
        Event::NetboxWritten(written) if written.deleted => e::netbox_deleted(&written.display),
        Event::NetboxWritten(written) if written.changes.is_empty() => {
            e::netbox_created(&written.display, &written.path)
        }
        Event::NetboxWritten(written) => e::netbox_changed(
            &written.display,
            written
                .changes
                .iter()
                .map(|change| (change.field.as_str(), change.value.as_deref(), change.previous.as_deref())),
        ),
        Event::NetboxUndone(undone) if undone.deleted => e::netbox_deleted(&undone.display),
        Event::NetboxUndone(undone) if undone.recreated => {
            e::netbox_recreated(&undone.display, &undone.path)
        }
        Event::NetboxUndone(undone) => e::netbox_restored(&undone.display),
        Event::RepartitionStarted(started) => e::repartition_started(started.count, started.files),
        Event::Repartitioned(done) => e::repartitioned(
            &done
                .partitions
                .iter()
                .map(|p| crate::i18n::device::partition(&p.name, &p.version, p.size_mib, false, p.running))
                .collect::<Vec<_>>(),
            &done.missing_files,
        ),
        Event::DeviceModeChanged(changed) => e::device_mode_changed(&features(
            changed.changes.iter().map(|c| (c.feature.as_str(), &c.reason)),
        )),
    };
    html!({ text })
}

/// The features of a device-mode change, each with why it is needed.
fn features<'a>(changes: impl Iterator<Item = (&'a str, &'a job_events::DeviceModeReason)>) -> Vec<String> {
    use crate::i18n::common::{DeviceModeNeed, device_mode_feature};
    changes
        .map(|(feature, reason)| {
            device_mode_feature(
                feature,
                match reason {
                    job_events::DeviceModeReason::APPLY_FALLBACK => DeviceModeNeed::ApplyFallback,
                    job_events::DeviceModeReason::MANAGEMENT_PROXY => DeviceModeNeed::ManagementProxy,
                    job_events::DeviceModeReason::UPDATE_FALLBACK => DeviceModeNeed::UpdateFallback,
                    job_events::DeviceModeReason::Other(other) => DeviceModeNeed::Other(other),
                },
            )
        })
        .collect()
}
