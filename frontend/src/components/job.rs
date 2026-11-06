//! A job of the backend (an update, an apply, a setup): its events while it runs, subscribed to
//! (each answer brings the new ones), worded by `i18n::job`; with a button to cancel it.
use crate::{
    error::{FrontendError, ServerProblem},
    graphql::{
        authenticated::{
            job::{
                CancelJob, CancelJobVariables, Event, FallbackLeftReason, Job, JobEvents,
                JobEventsVariables, Logged, PowerWait, RebootReason, ScriptProgress,
            },
            types::{
                DeviceModeReason, ErrorInfo, JobKind, JobPhase as Phase, JobState, LeftoverAction,
            },
        },
        live::{Subscription, watch_data},
        request,
    },
    i18n::{common::CANCEL, device, job as t, login},
    pages::start_login,
};
use cynic::{MutationBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, CodeBlock, CodeBlockCode, Spinner, SpinnerSize,
};
use std::time::Duration;
use yew::{Callback, Component, Context, Html, Properties, html, platform::spawn_local};

/// How often a running job is asked for new events where it isn't subscribed to.
pub(crate) const POLL: Duration = Duration::from_secs(1);
/// How long to wait after the 1st, 2nd, … failed asking in a row; then the last one.
pub(crate) const RETRY: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

pub struct JobView {
    job: Option<Job>,
    /// the last asking failed (the server is away, restarts); asked again until it answers
    error: Option<FrontendError>,
    /// the server doesn't know the job (anymore): it restarted meanwhile
    lost: bool,
    /// its events as they come; ends with the job, or once the component is gone
    subscription: Option<Subscription>,
    cancelling: bool,
    /// starting the login (after the server refused the token) failed
    login_error: Option<String>,
}

#[derive(Properties, Clone, PartialEq)]
pub struct JobViewProps {
    pub id: i32,
    /// the job is over (done, failed or cancelled), `true` when it is done
    #[prop_or_default]
    pub on_finished: Callback<bool>,
}

pub enum JobMessage {
    Loaded(Job),
    Error(FrontendError),
    /// the server answered without the job
    Lost,
    Cancel,
    /// log in again: the server refused the token
    Login,
}

impl Component for JobView {
    type Message = JobMessage;
    type Properties = JobViewProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let variables = JobEventsVariables {
            id: ctx.props().id,
            after: Some(0),
        };
        let subscription = watch_data(ctx.link(), JobEvents::build(variables), move |answer| {
            scope.send_message(match answer {
                Ok(JobEvents { job: Some(job) }) => JobMessage::Loaded(job),
                // the backend restarted and forgot it
                Ok(JobEvents { job: None }) => JobMessage::Lost,
                Err(error) => JobMessage::Error(error),
            })
        });
        JobView {
            job: None,
            error: None,
            lost: false,
            subscription: Some(subscription),
            cancelling: false,
            login_error: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            JobMessage::Loaded(mut loaded) => {
                if let Some(job) = self.job.take() {
                    // after the connection came back the subscription starts over: only the new
                    let next = job.events.last().map_or(0, |last| last.index + 1);
                    let mut events = job.events;
                    events.extend(
                        loaded
                            .events
                            .into_iter()
                            .filter(|event| event.index >= next),
                    );
                    loaded.events = events;
                }
                if loaded.state != JobState::Running {
                    self.subscription = None;
                    ctx.props().on_finished.emit(loaded.state == JobState::Done);
                }
                self.job = Some(loaded);
                self.error = None;
            }
            JobMessage::Error(error) => self.error = Some(error),
            JobMessage::Lost => {
                self.subscription = None;
                self.lost = true;
                self.error = None;
                ctx.props().on_finished.emit(false);
            }
            JobMessage::Login => self.login_error = start_login(ctx.link()),
            JobMessage::Cancel => {
                self.cancelling = true;
                let scope = ctx.link().clone();
                let id = ctx.props().id;
                spawn_local(async move {
                    let cancel = CancelJob::build(CancelJobVariables { id });
                    if let Err(error) = request(scope.clone(), cancel).await {
                        scope.send_message(JobMessage::Error(error));
                    }
                });
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        // one line while the server is away or refuses the token, it is asked again
        let error = self.error.as_ref().map(|e| match e {
            FrontendError::Server {
                status: 401 | 403, ..
            } => html! {
                <p class="job-unreachable">
                    {t::login_refused(&e.message())}{" "}
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={login::AGAIN}
                        onclick={ctx.link().callback(|_| JobMessage::Login)}
                    />
                    if let Some(failure) = &self.login_error { <br/>{login::failed(failure)} }
                </p>
            },
            e => html!(<p class="job-unreachable">{t::server_away(&e.message())}</p>),
        });
        let Some(job) = &self.job else {
            if self.lost {
                return html! {
                    <Alert inline=true title={t::LOST} r#type={AlertType::Warning}>
                        <p>{t::LOST_TEXT}{t::LOST_FALLBACK}</p>
                    </Alert>
                };
            }
            return html!(<>{error}<Spinner size={SpinnerSize::Md}/></>);
        };
        let fallback = matches!(job.kind, JobKind::Apply | JobKind::Update);
        let lost = self
            .lost
            .then(|| html!(<p>{t::LOST_TEXT}{if fallback { t::LOST_FALLBACK } else { "" }}</p>));
        let running = job.state == JobState::Running && !self.lost;
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
                if let Some(phase) = phase_line(job, running) { <p class="job-phase">{phase}</p> }
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
}

fn title(job: &Job) -> (String, AlertType) {
    let what = match &job.kind {
        JobKind::Update => t::UPDATE,
        JobKind::Downgrade => t::DOWNGRADE,
        JobKind::Cleanup => t::CLEANUP,
        JobKind::Apply => t::APPLY,
        JobKind::Bootstrap => t::BOOTSTRAP,
        JobKind::Reboot => t::REBOOT,
        JobKind::PowerCycle => t::POWER_CYCLE,
        JobKind::DeviceMode => t::DEVICE_MODE,
        JobKind::Repartition => t::REPARTITION,
        JobKind::BootPartition => t::BOOT_PARTITION,
        JobKind::NetboxWrite => t::NETBOX_WRITE,
        JobKind::NetboxUndo => t::NETBOX_UNDO,
        JobKind::SiteDiff => t::SITE_DIFF,
        JobKind::Other(other) => other,
    };
    let events = || job.events.iter().map(|event| &event.event);
    if job.state == JobState::Done {
        let nothing = |e: &Event| matches!(e, Event::ApplyPlanned(p) if p.script.trim().is_empty());
        if events().any(nothing) {
            return (t::NOTHING_TO_APPLY.into(), AlertType::Success);
        }
        let incomplete = events().any(|e| match e {
            Event::ApplyRunning(running) => running.error.is_some(),
            Event::ApplyStarted(started) => started.error.is_some(),
            Event::ApplyChecked(checked) => {
                checked.error.is_some() || !checked.remaining.trim().is_empty()
            }
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
        JobState::Running => (t::running(what), AlertType::Info),
        JobState::Done => {
            let update = events().find_map(|event| match event {
                Event::UpdateDone(done) => Some((done.from.as_str(), done.to.as_str())),
                Event::Downgraded(done) => Some((done.from.as_str(), done.to.as_str())),
                _ => None,
            });
            (t::done(what, update), AlertType::Success)
        }
        JobState::Failed => (t::failed(what), AlertType::Danger),
        JobState::Cancelled => (t::cancelled(what), AlertType::Warning),
        JobState::Other(other) => (t::other_state(what, other), AlertType::Warning),
    }
}

/// The state the job is in while it runs, or ended in when it didn't get through; nothing
/// once done (its last state says nothing then).
fn phase_line(job: &Job, running: bool) -> Option<String> {
    use crate::graphql::authenticated::types::{
        ApplyState as A, BootstrapState as B, CleanupState as C, DeviceModeState as D,
        NetboxState as N, PowerCycleState as P, RebootState as R, UpdateState as U,
    };
    use t::phase as p;
    if job.state == JobState::Done {
        return None;
    }
    let other = |other: &str| other.to_string();
    let state = match job.phase.as_ref()? {
        Phase::ApplyPhase(phase) => match &phase.apply {
            A::Planning => p::PLANNING.into(),
            A::SuspendingWatchdogs => p::SUSPENDING_WATCHDOGS.into(),
            A::ArmingFallback => p::ARMING_FALLBACK.into(),
            A::WritingScript => p::WRITING_SCRIPT.into(),
            A::StartingScript => p::STARTING_SCRIPT.into(),
            A::ScriptRunning => p::SCRIPT_RUNNING.into(),
            A::TakingBack => p::TAKING_BACK.into(),
            A::StoppingFallback => p::STOPPING_FALLBACK.into(),
            A::NotReached => p::NOT_REACHED.into(),
            A::Checking => p::CHECKING.into(),
            A::Finishing => p::FINISHING.into(),
            A::Other(o) => other(o),
        },
        Phase::UpdatePhase(phase) => match &phase.update {
            U::Reading => p::READING.into(),
            U::SuspendingWatchdogs => p::SUSPENDING_WATCHDOGS.into(),
            U::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            U::RememberingNeighbors => p::REMEMBERING_NEIGHBORS.into(),
            U::ArmingFallback => p::ARMING_FALLBACK.into(),
            U::Downloading => p::DOWNLOADING.into(),
            U::Uploading => p::UPLOADING.into(),
            U::WaitingToReboot => p::WAITING_TO_REBOOT.into(),
            U::Rebooting => p::REBOOTING.into(),
            U::NotUpdated => p::NOT_UPDATED.into(),
            U::StoppingFallback => p::STOPPING_FALLBACK.into(),
            U::WaitingForNeighbors => p::WAITING_FOR_NEIGHBORS.into(),
            U::Firmware => p::FIRMWARE.into(),
            U::WaitingToRebootFirmware => p::WAITING_TO_REBOOT_FIRMWARE.into(),
            U::RebootingFirmware => p::REBOOTING_FIRMWARE.into(),
            U::RestoringWatchdogs => p::RESTORING_WATCHDOGS.into(),
            U::Other(o) => other(o),
        },
        Phase::RebootPhase(phase) => match &phase.reboot {
            R::Reading => p::READING.into(),
            R::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            R::Rebooting => p::REBOOTING.into(),
            R::Other(o) => other(o),
        },
        Phase::PowerCyclePhase(phase) => match &phase.power_cycle {
            P::Reading => p::READING.into(),
            P::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            P::Cutting => p::CUTTING.into(),
            P::WaitingForDevice => p::WAITING_FOR_DEVICE.into(),
            P::Other(o) => other(o),
        },
        Phase::DeviceModePhase(phase) => match &phase.device_mode {
            D::Reading => p::READING.into(),
            D::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            D::Requesting => p::REQUESTING.into(),
            D::WaitingForConfirmation => p::WAITING_FOR_CONFIRMATION.into(),
            D::Rebooting => p::REBOOTING.into(),
            D::Other(o) => other(o),
        },
        Phase::BootstrapPhase(phase) => match &phase.bootstrap {
            B::SuspendingWatchdogs => p::SUSPENDING_WATCHDOGS.into(),
            B::CheckingNeighbor => p::CHECKING_NEIGHBOR.into(),
            B::Console => p::CONSOLE.into(),
            B::WaitingForApi => p::WAITING_FOR_API.into(),
            B::RestoringWatchdogs => p::RESTORING_WATCHDOGS.into(),
            B::Other(o) => other(o),
        },
        Phase::CleanupPhase(phase) => match &phase.cleanup {
            C::Clearing => p::CLEARING.into(),
            C::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            C::Rebooting => p::REBOOTING.into(),
            C::Other(o) => other(o),
        },
        Phase::NetboxPhase(phase) => match &phase.netbox {
            N::Planning => p::PLANNING.into(),
            N::Writing => p::WRITING.into(),
            N::Undoing => p::UNDOING.into(),
            N::Refetching => p::REFETCHING.into(),
            N::Other(o) => other(o),
        },
        Phase::Unknown => return None,
    };
    Some(if running {
        p::running(&state)
    } else {
        p::ended(&state)
    })
}

/// `m:ss` since the start of the job.
fn elapsed(nanos: u64) -> String {
    let seconds = nanos / 1_000_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// An error the job or one of its events reports.
fn problem(problem: &ErrorInfo) -> String {
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
fn items(events: &[Logged]) -> Vec<Html> {
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
fn script_progress(progress: &ScriptProgress, planned: Option<&str>) -> Html {
    let line_numbers = |lines: &[i32]| lines.iter().map(|&line| line as u32).collect::<Vec<_>>();
    let (lost, back) = (line_numbers(&progress.lost), line_numbers(&progress.back));
    let lines = t::event::script_progress(t::event::Progress {
        lines: progress.lines as u32,
        section: progress.section.map(|s| s as u32),
        done: progress.done,
        error: progress
            .error_line
            .zip(progress.error.as_deref())
            .map(|(line, error)| (line as u32, error)),
        probe: progress
            .probe
            .as_deref()
            .map(|probe| (probe, progress.probe_answered)),
        lost: &lost,
        back: &back,
        rearmed: progress.rearmed.map(seconds),
        after_fallback: progress.after_fallback,
    });
    let at: Vec<i32> = progress
        .error_line
        .iter()
        .chain(&progress.lost)
        .copied()
        .collect();
    let shown = planned.map(|planned| {
        let script: Vec<&str> = planned.lines().collect();
        at.iter()
            .map(|&line| {
                let from = (line - 2).max(1);
                (from..=line)
                    .filter_map(|n| {
                        script
                            .get(n as usize - 1)
                            .map(|text| format!("{n:>4}  {text}"))
                    })
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

fn describe(event: &Logged) -> Html {
    use t::event as e;
    let text = match &event.event {
        // one this page doesn't know yet; a site's job (`components::site_diff`) is never on a
        // device's page
        Event::Unknown => e::UNKNOWN.into(),
        Event::ApplyPlanned(planned) if planned.script.trim().is_empty() => e::NOTHING_TO_DO.into(),
        Event::ApplyPlanned(planned) => return script(t::SCRIPT, &planned.script),
        Event::FallbackArmed(armed) => e::fallback_armed(
            seconds(armed.delay),
            armed.window.map(seconds),
            armed.radio_wait.map(seconds),
        ),
        Event::FallbackUnavailable(unavailable) => {
            e::fallback_unavailable(&unavailable.device_mode)
        }
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
                FallbackLeftReason::Cancelled => e::FallbackLeft::Cancelled,
                FallbackLeftReason::NotReached => e::FallbackLeft::NotReached,
                FallbackLeftReason::NotStopped => {
                    e::FallbackLeft::NotStopped(left.error.as_ref().map(problem))
                }
                FallbackLeftReason::NotApplied => e::FallbackLeft::NotApplied,
                FallbackLeftReason::Due => e::FallbackLeft::Due,
                FallbackLeftReason::BatchIncomplete => e::FallbackLeft::BatchIncomplete,
                FallbackLeftReason::Other(other) => e::FallbackLeft::Other(other),
            },
            seconds(left.fires_in),
        ),
        Event::SetupStarted(setup) => {
            e::setup_started(&setup.neighbor, &setup.neighbor_port, &setup.macs)
        }
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
            None => e::radios_reprovisioned(radios.count.into(), &radios.menu),
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
            PowerWait::SupplierRebooting => e::waiting_for_supplier(&wait.device),
            PowerWait::ConsumerUpdating => e::waiting_for_consumer(&wait.device),
            PowerWait::Other(other) => e::waiting_for(&wait.device, other),
        },
        Event::UpdateStart(start) => {
            e::update_start(&start.version, &start.architecture, &start.packages)
        }
        Event::UpdateStepPlanned(step) => {
            e::update_step(&step.version, &step.files, &step.uninstall)
        }
        Event::SshPasswordLogin(ssh) if ssh.restored => e::ssh_restored(&ssh.setting, &ssh.value),
        Event::SshPasswordLogin(ssh) => e::ssh_for_upload(&ssh.setting, &ssh.value),
        Event::PackageDownloaded(package) => e::downloaded(&package.file, package.bytes.0),
        Event::PackageUploading(package) => {
            e::uploading(&package.file, package.bytes.0, package.total.0)
        }
        Event::PackageUploaded(package) => e::uploaded(&package.file),
        Event::Reboot(reboot) => e::reboot(
            match &reboot.reason {
                RebootReason::Packages => e::Reboot::Packages,
                RebootReason::Firmware => e::Reboot::Firmware,
                RebootReason::Requested => e::Reboot::Requested,
                RebootReason::DeviceMode => e::Reboot::DeviceMode,
                RebootReason::Repartition => e::Reboot::Repartition,
                RebootReason::Partition => e::Reboot::Partition,
                RebootReason::Downgrade => e::Reboot::Downgrade,
                RebootReason::ApplyFallback => e::Reboot::ApplyFallback,
                RebootReason::Other(other) => e::Reboot::Other(other),
            },
            reboot.timeout / 60_000_000_000,
        ),
        Event::RouterOsRunning(running) => e::routeros_running(&running.version),
        Event::FirmwareUpgrade(firmware) if firmware.running => {
            e::firmware_running(&firmware.upgrade)
        }
        Event::FirmwareUpgrade(firmware) => {
            e::firmware_upgrade(&firmware.current, &firmware.upgrade)
        }
        Event::UpdateDone(done) => e::update_done(&done.from, &done.to),
        Event::Downgraded(done) => e::downgraded(&done.from, &done.to),
        Event::LeftoverCleared(cleared) => e::leftover_cleared({
            use LeftoverAction as A;
            match &cleared.action {
                A::StopApplyFallback => e::Cleared::StopApplyFallback,
                A::ApplyFallbackNow => e::Cleared::ApplyFallbackNow,
                A::StopUpdateFallback => e::Cleared::StopUpdateFallback,
                A::RestoreWatchdogs => e::Cleared::RestoreWatchdogs,
                A::RemovePackages => e::Cleared::RemovePackages,
                A::Other(other) => e::Cleared::Other(other),
            }
        }),
        Event::DeviceModeConfirm(confirm) => e::device_mode_confirm(
            &features(
                confirm
                    .changes
                    .iter()
                    .map(|c| (c.feature.as_str(), &c.reason)),
            ),
            confirm.timeout / 60_000_000_000,
        ),
        Event::PowerCycled(cycled) => e::power_cycled(
            &cycled.supplier,
            &cycled.port,
            &cycled.device,
            cycled.seconds.into(),
        ),
        Event::PartitionRunning(running) => e::partition_running(&running.name, &running.version),
        Event::PartitionSynced(synced) => match &synced.error {
            None => e::partition_synced(
                synced.partition.as_deref().unwrap_or_default(),
                synced.config_only,
            ),
            Some(error) => e::partition_not_synced(synced.partition.as_deref(), &problem(error)),
        },
        Event::UpdateFallbackArmed(armed) => {
            e::update_fallback_armed(&armed.partition, &armed.version, seconds(armed.delay) / 60)
        }
        Event::UpdateFallbackDisarmed(disarmed) => e::update_fallback_disarmed(&disarmed.partition),
        Event::UpdateWithoutFallback(without) => {
            e::update_without_fallback(without.partitions.into(), &without.neighbors)
        }
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
        Event::JournalNotWritten(failed) => e::journal_not_written(&problem(&failed.error)),
        Event::WatchdogKept(kept) => e::watchdog_kept(
            &kept.device,
            &watching!(kept.watching),
            seconds(kept.safety_net) / 60,
        ),
        Event::Resumed(resumed) => e::resumed(seconds(resumed.after)),
        Event::NetboxWritten(written) if written.deleted => e::netbox_deleted(&written.display),
        Event::NetboxWritten(written) if written.changes.is_empty() => {
            e::netbox_created(&written.display, &written.path)
        }
        Event::NetboxWritten(written) => e::netbox_changed(
            &written.display,
            written
                .changes
                .iter()
                .map(|c| (c.field.as_str(), c.value.as_deref(), c.previous.as_deref())),
        ),
        Event::NetboxUndone(undone) if undone.deleted => e::netbox_deleted(&undone.display),
        Event::NetboxUndone(undone) if undone.recreated => {
            e::netbox_recreated(&undone.display, &undone.path)
        }
        Event::NetboxUndone(undone) => e::netbox_restored(&undone.display),
        Event::RepartitionStarted(started) => {
            e::repartition_started(started.count.into(), started.files.into())
        }
        Event::Repartitioned(done) => e::repartitioned(
            &done
                .partitions
                .iter()
                .map(|p| device::partition(&p.name, &p.version, p.size_mib, false, p.running))
                .collect::<Vec<_>>(),
            &done.missing_files,
        ),
        Event::DeviceModeChanged(changed) => e::device_mode_changed(&features(
            changed
                .changes
                .iter()
                .map(|c| (c.feature.as_str(), &c.reason)),
        )),
    };
    html!({ text })
}

/// The features of a device-mode change, each with why it is needed (also on the device page).
pub(crate) fn features<'a>(
    changes: impl Iterator<Item = (&'a str, &'a DeviceModeReason)>,
) -> Vec<String> {
    use crate::i18n::common::{DeviceModeNeed, device_mode_feature};
    changes
        .map(|(feature, reason)| {
            device_mode_feature(
                feature,
                match reason {
                    DeviceModeReason::ApplyFallback => DeviceModeNeed::ApplyFallback,
                    DeviceModeReason::ManagementProxy => DeviceModeNeed::ManagementProxy,
                    DeviceModeReason::UpdateFallback => DeviceModeNeed::UpdateFallback,
                    DeviceModeReason::Other(other) => DeviceModeNeed::Other(other),
                },
            )
        })
        .collect()
}
