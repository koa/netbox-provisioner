//! A job of the backend (an update): its events while it runs, asked for every second, worded
//! here; with a button to cancel it.
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{
            CancelJob, JobEvents, cancel_job,
            job_events::{
                self, JobEventsJob, JobEventsJobEvents, JobEventsJobEventsEvent as Event, JobKind,
                JobState, PowerWait, RebootReason,
            },
        },
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant, Spinner, SpinnerSize};
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
                    { for job.events.iter().map(|event| html!(<li>
                        <span class="job-time">{ elapsed(event.at) }</span>
                        { describe(event) }
                    </li>)) }
                </ul>
                if running {
                    <p>
                        <Spinner size={SpinnerSize::Md}/>{" "}
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={if self.cancelling { "Wird abgebrochen …" } else { "Abbrechen" }}
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
        JobKind::UPDATE => "RouterOS-Update",
        JobKind::Other(other) => other,
    };
    match &job.state {
        JobState::RUNNING => (format!("{what} läuft"), AlertType::Info),
        JobState::DONE => {
            let done = job.events.iter().find_map(|event| match &event.event {
                Event::UpdateDone(done) => Some(format!(": RouterOS {} → {}", done.from, done.to)),
                _ => None,
            });
            (format!("{what} abgeschlossen{}", done.unwrap_or_default()), AlertType::Success)
        }
        JobState::FAILED => (format!("{what} fehlgeschlagen"), AlertType::Danger),
        JobState::CANCELLED => (format!("{what} abgebrochen"), AlertType::Warning),
        JobState::Other(other) => (format!("{what}: {other}"), AlertType::Warning),
    }
}

/// `m:ss` since the start of the job.
fn elapsed(nanos: u64) -> String {
    let seconds = nanos / 1_000_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "keine".into()
    } else {
        items.join(", ")
    }
}

fn describe(event: &JobEventsJobEvents) -> Html {
    let text = match &event.event {
        Event::PoeChain(chain) => {
            let mut parts = Vec::new();
            if !chain.suppliers.is_empty() {
                parts.push(format!("über PoE versorgt von {}", chain.suppliers.join(", ")));
            }
            if !chain.consumers.is_empty() {
                parts.push(format!("versorgt über PoE {}", chain.consumers.join(", ")));
            }
            format!("Das Gerät wird {}", parts.join(" und "))
        }
        Event::WaitingForPower(wait) => match &wait.reason {
            PowerWait::SUPPLIER_REBOOTING => format!(
                "Wartet, bis {} (versorgt das Gerät über PoE) nach seinem Neustart zurück ist",
                wait.device
            ),
            PowerWait::CONSUMER_UPDATING => format!(
                "Wartet mit dem Neustart, bis {} (über PoE versorgt) sein Update abgeschlossen hat",
                wait.device
            ),
            PowerWait::Other(other) => format!("Wartet auf {} ({other})", wait.device),
        },
        Event::UpdateStart(start) => format!(
            "RouterOS {} ({}), Pakete: {}",
            start.version,
            start.architecture,
            list(&start.packages)
        ),
        Event::UpdateStepPlanned(step) => {
            let mut text = format!("Update auf {}: {}", step.version, list(&step.files));
            if !step.uninstall.is_empty() {
                text.push_str(&format!("; entfernt {}", step.uninstall.join(", ")));
            }
            text
        }
        Event::SshPasswordLogin(ssh) if ssh.restored => {
            format!("SSH: {}={} wieder gesetzt", ssh.setting, ssh.value)
        }
        Event::SshPasswordLogin(ssh) => format!(
            "SSH: {}={} für den Upload (der Benutzer hat SSH-Schlüssel)",
            ssh.setting, ssh.value
        ),
        Event::PackageDownloaded(package) => format!(
            "{}: {:.1} MB heruntergeladen",
            package.file,
            package.bytes as f64 / 1_000_000.0
        ),
        Event::PackageUploaded(package) => format!("{}: auf das Gerät kopiert", package.file),
        Event::Reboot(reboot) => format!(
            "Neustart {}, wartet bis zu {} min, bis das Gerät zurück ist",
            match &reboot.reason {
                RebootReason::PACKAGES => "zum Installieren der Pakete",
                RebootReason::FIRMWARE => "für die neue Firmware",
                RebootReason::Other(other) => other,
            },
            reboot.timeout / 60_000_000_000
        ),
        Event::RouterOsRunning(running) => format!("RouterOS {} läuft", running.version),
        Event::FirmwareUpgrade(firmware) if firmware.running => {
            format!("Firmware {} läuft", firmware.upgrade)
        }
        Event::FirmwareUpgrade(firmware) => {
            format!("Firmware {} → {}", firmware.current, firmware.upgrade)
        }
        Event::UpdateDone(done) => format!("Fertig: RouterOS {} → {}", done.from, done.to),
    };
    html!({ text })
}
