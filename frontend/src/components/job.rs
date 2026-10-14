//! A job of the backend (an update, an apply, a setup): its events while it runs, asked for every second, worded
//! here; with a button to cancel it.
use crate::{
    error::FrontendError,
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
        JobKind::APPLY => "Anwenden",
        JobKind::BOOTSTRAP => "Ersteinrichtung",
        JobKind::REBOOT => "Neustart",
        JobKind::Other(other) => other,
    };
    let events = || job.events.iter().map(|event| &event.event);
    if job.state == JobState::DONE {
        if events().any(|e| matches!(e, Event::ApplyPlanned(planned) if planned.script.trim().is_empty())) {
            return ("Nichts anzuwenden, das Gerät entspricht NetBox".into(), AlertType::Success);
        }
        let incomplete = events().any(|e| match e {
            Event::ApplyRunning(running) => running.error.is_some(),
            Event::ApplyChecked(checked) => checked.error.is_some() || !checked.remaining.trim().is_empty(),
            Event::FallbackLeft(_) => true,
            _ => false,
        });
        if incomplete {
            return ("Nicht alles wurde angewendet".into(), AlertType::Warning);
        }
        if events().any(|e| matches!(e, Event::ApplyChecked(_))) {
            return ("Angewendet, das Gerät entspricht jetzt NetBox".into(), AlertType::Success);
        }
        if let Some(address) = events().find_map(|e| match e {
            Event::SetupDone(done) => Some(&done.address),
            _ => None,
        }) {
            return (
                format!("Ersteinrichtung abgeschlossen, das Gerät antwortet an {address}"),
                AlertType::Success,
            );
        }
    }
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
    let text = match &event.event {
        Event::ApplyPlanned(planned) if planned.script.trim().is_empty() => {
            "Nichts zu tun: das Gerät entspricht NetBox".to_string()
        }
        Event::ApplyPlanned(planned) => return script("Skript:", &planned.script),
        Event::FallbackArmed(armed) => format!(
            "Backup gespeichert: das Gerät lädt es in {} s, wenn der Provisioner es danach nicht erreicht",
            seconds(armed.delay)
        ),
        Event::ApplyRunning(running) => match &running.error {
            Some(error) => format!(
                "Das Skript hat angehalten ({error}), die Zeilen davor sind angewendet; wartet bis zu {} s auf das Gerät",
                seconds(running.timeout)
            ),
            None => format!(
                "Das Skript läuft auf dem Gerät; wartet bis zu {} s, bis es NetBox entspricht",
                seconds(running.timeout)
            ),
        },
        Event::ApplyChecked(checked) => match &checked.error {
            Some(error) => format!("Das Gerät lässt sich nicht prüfen: {error}"),
            None if checked.remaining.trim().is_empty() => "Das Gerät entspricht NetBox".into(),
            None => return script("Noch offen:", &checked.remaining),
        },
        Event::FallbackStopped(_) => {
            "Fallback gestoppt: das Gerät ist mit der neuen Konfiguration erreichbar".into()
        }
        Event::FallbackLeft(left) => format!(
            "{}: das Gerät lädt in etwa {} s das Backup von vor dem Anwenden und startet neu",
            match (&left.reason, &left.error) {
                (FallbackLeftReason::CANCELLED, _) => "Abgebrochen, der Fallback bleibt".into(),
                (FallbackLeftReason::NOT_REACHED, _) => "Gerät nicht erreicht, der Fallback bleibt".into(),
                (FallbackLeftReason::NOT_STOPPED, Some(error)) => {
                    format!("Fallback lässt sich nicht stoppen ({error})")
                }
                (FallbackLeftReason::NOT_STOPPED, None) => "Fallback lässt sich nicht stoppen".into(),
                (FallbackLeftReason::Other(other), _) => format!("Fallback bleibt ({other})"),
            },
            seconds(left.fires_in)
        ),
        Event::SetupStarted(setup) => format!(
            "Konsole auf {}, MAC-Telnet an {} zu {}",
            setup.neighbor,
            setup.neighbor_port,
            setup.macs.join(", ")
        ),
        Event::ConsoleOutput(output) => return console(&output.text),
        Event::WaitingForApi(wait) => format!(
            "Wartet bis zu {} s, bis das Gerät an {} die API öffnet",
            seconds(wait.timeout),
            wait.address
        ),
        Event::SetupDone(done) => format!("Das Gerät antwortet an {}", done.address),
        Event::RadiosReprovisioned(radios) => match &radios.error {
            None => format!("{} Radios neu provisioniert ({})", radios.count, radios.menu),
            Some(error) => format!("{} nicht neu provisioniert: {error}", radios.menu),
        },
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
                RebootReason::REQUESTED => "auf Anfrage",
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
