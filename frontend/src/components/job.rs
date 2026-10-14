//! A job of the backend (an update, an apply, a setup): its events while it runs, asked for every second, worded
//! by `i18n::job`; with a button to cancel it.
use crate::{
    error::FrontendError,
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
const POLL: Duration = Duration::from_secs(1);

pub struct JobView {
    job: Option<JobEventsJob>,
    error: Option<FrontendError>,
    /// asking stops once the component is gone
    alive: Rc<Cell<bool>>,
    cancelling: bool,
}

#[derive(Properties, Clone, PartialEq)]
pub struct JobViewProps {
    pub id: i64,
    /// the job is over (done, failed or cancelled)
    #[prop_or_default]
    pub on_finished: Callback<()>,
}

pub enum JobMessage {
    Loaded(JobEventsJob),
    Error(FrontendError),
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
                    ctx.props().on_finished.emit(());
                }
                self.job = Some(loaded);
                self.error = None;
            }
            JobMessage::Error(error) => self.error = Some(error),
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
        let error = self.error.as_ref().map(|e| e.to_html());
        let Some(job) = &self.job else {
            return html!(<>{error}<Spinner size={SpinnerSize::Md}/></>);
        };
        let running = job.state == JobState::RUNNING;
        let (title, kind) = title(job);
        html! {
            <Alert inline=true title={title} r#type={kind}>
                {error}
                if let Some(error) = &job.error { <p>{ error.clone() }</p> }
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

/// Asks for the job's new events until it is over.
fn poll(scope: Scope<JobView>, id: i64, alive: Rc<Cell<bool>>) {
    spawn_local(async move {
        let mut after = 0;
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
                    scope.send_message(JobMessage::Error(FrontendError::MissingData));
                    return;
                }
                Err(error) => scope.send_message(JobMessage::Error(error)),
            }
            sleep(POLL).await;
        }
    });
}

fn title(job: &JobEventsJob) -> (String, AlertType) {
    let what = match &job.kind {
        JobKind::UPDATE => t::UPDATE,
        JobKind::APPLY => t::APPLY,
        JobKind::BOOTSTRAP => t::BOOTSTRAP,
        JobKind::REBOOT => t::REBOOT,
        JobKind::Other(other) => other,
    };
    let events = || job.events.iter().map(|event| &event.event);
    if job.state == JobState::DONE {
        if events().any(|e| matches!(e, Event::ApplyPlanned(planned) if planned.script.trim().is_empty())) {
            return (t::NOTHING_TO_APPLY.into(), AlertType::Success);
        }
        let incomplete = events().any(|e| match e {
            Event::ApplyRunning(running) => running.error.is_some(),
            Event::ApplyChecked(checked) => checked.error.is_some() || !checked.remaining.trim().is_empty(),
            Event::FallbackLeft(_) => true,
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
            describe(event)
        };
        items.push(html!(<li>
            <span class="job-time">{ elapsed(event.at) }</span>
            { content }
        </li>));
    }
    items
}

fn console(text: &str) -> Html {
    html!(<pre class="job-console">{ text.to_string() }</pre>)
}

fn describe(event: &JobEventsJobEvents) -> Html {
    use t::event as e;
    let text = match &event.event {
        Event::ApplyPlanned(planned) if planned.script.trim().is_empty() => e::NOTHING_TO_DO.into(),
        Event::ApplyPlanned(planned) => return script(t::SCRIPT, &planned.script),
        Event::FallbackArmed(armed) => e::fallback_armed(seconds(armed.delay)),
        Event::ApplyRunning(running) => match &running.error {
            Some(error) => e::script_stopped(error, seconds(running.timeout)),
            None => e::script_running(seconds(running.timeout)),
        },
        Event::ApplyChecked(checked) => match &checked.error {
            Some(error) => e::not_checked(error),
            None if checked.remaining.trim().is_empty() => e::IN_LINE.into(),
            None => return script(t::REMAINING, &checked.remaining),
        },
        Event::FallbackStopped(_) => e::FALLBACK_STOPPED.into(),
        Event::FallbackLeft(left) => e::fallback_left(
            match &left.reason {
                FallbackLeftReason::CANCELLED => e::FallbackLeft::Cancelled,
                FallbackLeftReason::NOT_REACHED => e::FallbackLeft::NotReached,
                FallbackLeftReason::NOT_STOPPED => e::FallbackLeft::NotStopped(left.error.as_deref()),
                FallbackLeftReason::Other(other) => e::FallbackLeft::Other(other),
            },
            seconds(left.fires_in),
        ),
        Event::SetupStarted(setup) => e::setup_started(&setup.neighbor, &setup.neighbor_port, &setup.macs),
        Event::ConsoleOutput(output) => return console(&output.text),
        Event::WaitingForApi(wait) => e::waiting_for_api(seconds(wait.timeout), &wait.address),
        Event::SetupDone(done) => e::setup_done(&done.address),
        Event::RadiosReprovisioned(radios) => match &radios.error {
            None => e::radios_reprovisioned(radios.count, &radios.menu),
            Some(error) => e::radios_not_reprovisioned(&radios.menu, error),
        },
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
        Event::PackageUploaded(package) => e::uploaded(&package.file),
        Event::Reboot(reboot) => e::reboot(
            match &reboot.reason {
                RebootReason::PACKAGES => e::Reboot::Packages,
                RebootReason::FIRMWARE => e::Reboot::Firmware,
                RebootReason::REQUESTED => e::Reboot::Requested,
                RebootReason::Other(other) => e::Reboot::Other(other),
            },
            reboot.timeout / 60_000_000_000,
        ),
        Event::RouterOsRunning(running) => e::routeros_running(&running.version),
        Event::FirmwareUpgrade(firmware) if firmware.running => e::firmware_running(&firmware.upgrade),
        Event::FirmwareUpgrade(firmware) => e::firmware_upgrade(&firmware.current, &firmware.upgrade),
        Event::UpdateDone(done) => e::update_done(&done.from, &done.to),
    };
    html!({ text })
}
