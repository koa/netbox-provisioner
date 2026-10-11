use crate::{
    components::{
        adjust_target::{AdjustTarget, SelectedCredentials, SelectedTarget},
        job::JobView,
    },
    error::FrontendError,
    graphql::{
        authenticated::{
            ApplyDeviceCfg, BootstrapCandidates, BootstrapDevice, DeviceJobs, DeviceOverview,
            UpdateRouteros, apply_device_cfg, bootstrap_candidates, bootstrap_device,
            device_jobs, device_overview, update_routeros,
        },
        query_authenticated, query_authenticated_response,
    },
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, CodeBlock, CodeBlockCode, ExpandableSection,
    SimpleSelect, Spinner, SpinnerSize, TextInput,
};
use yew::{Component, Context, Html, Properties, html, html::Scope, platform::spawn_local};

pub struct ShowDevice {
    id: u32,
    error: Option<FrontendError>,
    data: Option<ShowDeviceData>,
    alternate_target: SelectedTarget,
    bootstrap: BootstrapState,
    /// the job shown (an update, an apply), started here or running when the page was opened
    job: Option<i64>,
    job_running: bool,
    /// what the bootstrap neighbor sees on its port; `None` while it is asked
    candidates: Option<Result<Vec<Candidate>, String>>,
    /// a MAC typed in instead of picking a candidate
    manual_mac: String,
    /// the RouterOS channel to update in; `None` until the device's default is known
    channel: Option<device_overview::Channel>,
}
type Candidate =
    bootstrap_candidates::BootstrapCandidatesTopologyDeviceByIdBootstrapPlanDiscovered;
#[derive(Debug, Default)]
enum BootstrapState {
    #[default]
    Idle,
    Running,
    Done(bootstrap_device::BootstrapDeviceBootstrap),
}
/// the planned changes and what applying them would do that NetBox can't show (PoE switched off)
#[derive(Debug)]
struct GeneratedCfg {
    script: Box<str>,
    warnings: Vec<String>,
}
#[derive(Debug, Default)]
pub struct ShowDeviceData {
    configured_name: Box<str>,
    /// board and planned changes (`None`: the generator failed); `None` if the device couldn't
    /// be reached
    access: Option<(Box<str>, Option<GeneratedCfg>)>,
    routeros: Option<device_overview::DeviceOverviewTopologyDeviceByIdAccessRouteros>,
    bootstrap_plan: Option<device_overview::DeviceOverviewTopologyDeviceByIdBootstrapPlan>,
    bootstrap_problem: Option<String>,
}
#[derive(Debug, Properties, Clone, PartialEq)]
pub struct ShowDeviceProps {
    pub id: u32,
}

#[derive(Debug)]
pub enum ShowDeviceMessage {
    Error(FrontendError),
    Data {
        data: ShowDeviceData,
        error: Option<FrontendError>,
    },
    AdjustTarget(SelectedTarget),
    Apply,
    Candidates(Result<Vec<Candidate>, String>),
    ManualMac(String),
    /// set up the device with this MAC
    Bootstrap(String),
    Bootstrapped(Result<bootstrap_device::BootstrapDeviceBootstrap, FrontendError>),
    /// the label of a channel entry (`channel_label`)
    SelectChannel(Box<str>),
    Update,
    /// bring the packages in line at the running version
    AdjustPackages,
    JobStarted(Result<i64, FrontendError>),
    JobFinished,
}

impl Component for ShowDevice {
    type Message = ShowDeviceMessage;
    type Properties = ShowDeviceProps;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            id: ctx.props().id,
            error: None,
            data: None,
            alternate_target: Default::default(),
            bootstrap: Default::default(),
            job: None,
            job_running: false,
            candidates: None,
            manual_mac: String::new(),
            channel: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ShowDeviceMessage::Error(e) => {
                self.error = Some(e);
                true
            }
            ShowDeviceMessage::Data { data, error } => {
                if data.access.is_none() && data.bootstrap_plan.is_some() {
                    self.candidates = None;
                    fetch_candidates(ctx.link().clone(), self.id);
                }
                if self.channel.is_none() {
                    self.channel = data.routeros.as_ref().map(|r| r.default_channel.clone());
                }
                self.data = Some(data);
                self.error = error;
                true
            }
            ShowDeviceMessage::SelectChannel(label) => {
                self.channel = self
                    .data
                    .as_ref()
                    .and_then(|data| data.routeros.as_ref())
                    .and_then(|r| r.channels.iter().find(|c| channel_label(c) == label))
                    .map(|c| c.channel.clone());
                true
            }
            ShowDeviceMessage::Apply => {
                self.job_running = true;
                apply(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::Update => {
                let Some(channel) = self.channel.clone() else {
                    return false;
                };
                self.job_running = true;
                update(ctx.link().clone(), self.id, Some(channel), self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::AdjustPackages => {
                self.job_running = true;
                update(ctx.link().clone(), self.id, None, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::JobStarted(result) => {
                match result {
                    Ok(id) => {
                        self.job = Some(id);
                        self.job_running = true;
                    }
                    Err(error) => {
                        self.job_running = false;
                        self.error = Some(error);
                    }
                }
                true
            }
            ShowDeviceMessage::JobFinished => {
                self.job_running = false;
                // the device changed: show what is left now
                fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::Candidates(candidates) => {
                self.candidates = Some(candidates);
                true
            }
            ShowDeviceMessage::ManualMac(mac) => {
                self.manual_mac = mac;
                true
            }
            ShowDeviceMessage::Bootstrap(mac) => {
                self.bootstrap = BootstrapState::Running;
                bootstrap(ctx.link().clone(), self.id, mac);
                true
            }
            ShowDeviceMessage::Bootstrapped(result) => {
                match result {
                    Ok(result) => self.bootstrap = BootstrapState::Done(result),
                    Err(error) => {
                        self.bootstrap = BootstrapState::Idle;
                        self.error = Some(error);
                    }
                }
                fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::AdjustTarget(t) => {
                if self.alternate_target != t {
                    self.alternate_target = t;
                    fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
                    true
                } else {
                    false
                }
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let error = self.error.as_ref().map(|e| e.to_html());
        let running = self.job_running;
        let data = self.data.as_ref().map(|data| {
            let (board, cfg_update) = data
                .access
                .as_ref()
                .map(|(board, cfg)| (board.as_ref(), cfg.as_ref()))
                .unwrap_or_default();
            let generated = cfg_update.is_some();
            let warnings = cfg_update.map(|cfg| cfg.warnings.as_slice()).unwrap_or_default();
            let cfg_update = cfg_update.map(|cfg| cfg.script.as_ref()).unwrap_or_default();
            let nothing_to_do = cfg_update.trim().is_empty();
            let update_running = self.job_running;
            let routeros = data.routeros.as_ref().map(|routeros| {
                let selected = routeros
                    .channels
                    .iter()
                    .find(|c| Some(&c.channel) == self.channel.as_ref());
                let entries: Vec<Box<str>> = routeros.channels.iter().map(channel_label).collect();
                let action = selected.filter(|c| c.update).map(|c| {
                    if c.upgrade {
                        format!("Auf {} aktualisieren", c.newest)
                    } else {
                        "Pakete anpassen".to_string()
                    }
                });
                // without changing the version, whatever the channel; unless that is what the
                // channel's update does anyway
                let adjust_packages = routeros.adjust_packages
                    && !selected.is_some_and(|c| c.update && !c.upgrade);
                html! {<>
                <dt>{"RouterOS"}</dt><dd>
                    { routeros.version.clone() }
                    {format!(" (Pakete: {}; nach dem Update: {})", routeros.packages.join(", "), routeros.wanted.join(", "))}
                    <div class="routeros-update">
                        <SimpleSelect<Box<str>>
                            selected={selected.map(channel_label)}
                            entries={entries}
                            onselect={ctx.link().callback(ShowDeviceMessage::SelectChannel)}
                        />
                        if let Some(action) = action {
                            {" "}
                            <Button
                                variant={ButtonVariant::Secondary}
                                label={action}
                                disabled={update_running || running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::Update)}
                            />
                        } else if !adjust_packages {
                            {" aktuell"}
                        }
                        if adjust_packages {
                            {" "}
                            <Button
                                variant={ButtonVariant::Secondary}
                                label="Pakete anpassen"
                                disabled={update_running || running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::AdjustPackages)}
                            />
                        }
                    </div>
                </dd>
            </>}
            });
            let bootstrap_running = matches!(self.bootstrap, BootstrapState::Running);
            let update = if data.access.is_none() {
                let setup = match (&data.bootstrap_plan, &data.bootstrap_problem) {
                    (Some(plan), _) => {
                        let name = data.configured_name.clone();
                        let candidates = match &self.candidates {
                            None => html!(<p><Spinner size={SpinnerSize::Md}/>{format!(" {} sucht Geräte an {} …", plan.neighbor, plan.neighbor_port)}</p>),
                            Some(Err(error)) => html!(<p>{format!("{} kann nicht nach Geräten suchen: {error}", plan.neighbor)}</p>),
                            Some(Ok(found)) if found.is_empty() => html!(<p>{format!("{} sieht an {} kein Gerät.", plan.neighbor, plan.neighbor_port)}</p>),
                            Some(Ok(found)) => html! {
                                <table class="bootstrap-candidates">
                                    <thead><tr>
                                        <th>{"Identity"}</th><th>{"Board"}</th><th>{"RouterOS"}</th><th>{"MAC"}</th><th>{"Port"}</th><th/>
                                    </tr></thead>
                                    <tbody>
                                    { for found.iter().map(|c| {
                                        let mac = c.mac.clone();
                                        html! {<tr>
                                            <td>{&c.identity}</td><td>{&c.board}</td><td>{&c.version}</td>
                                            <td><code>{&c.mac}</code></td><td>{&c.interface}</td>
                                            <td><Button
                                                variant={ButtonVariant::Secondary}
                                                label={format!("Als {name} einrichten")}
                                                disabled={bootstrap_running}
                                                onclick={ctx.link().callback(move |_| ShowDeviceMessage::Bootstrap(mac.clone()))}
                                            /></td>
                                        </tr>}
                                    })}
                                    </tbody>
                                </table>
                            },
                        };
                        let manual = self.manual_mac.clone();
                        html! {<>
                            <p>{format!(
                                "Ersteinrichtung: {} öffnet an {} MAC-Telnet zum gewählten Gerät, meldet sich an, setzt das Passwort und gibt dessen {} die Adresse {}.",
                                plan.neighbor, plan.neighbor_port, plan.port, plan.address
                            )}</p>
                            {candidates}
                            <div class="bootstrap-manual">
                                <TextInput
                                    value={self.manual_mac.clone()}
                                    placeholder="MAC-Adresse"
                                    onchange={ctx.link().callback(ShowDeviceMessage::ManualMac)}
                                />
                                <Button
                                    variant={ButtonVariant::Secondary}
                                    label="Mit dieser MAC einrichten"
                                    disabled={bootstrap_running || manual.trim().is_empty()}
                                    onclick={ctx.link().callback(move |_| ShowDeviceMessage::Bootstrap(manual.clone()))}
                                />
                            </div>
                            if bootstrap_running { <p><Spinner size={SpinnerSize::Md}/>{" Ersteinrichtung läuft …"}</p> }
                        </>}
                    }
                    (None, Some(problem)) => html!(<p>{format!("Keine Ersteinrichtung möglich: {problem}")}</p>),
                    (None, None) => html!(),
                };
                html!(<Alert inline=true title="Gerät nicht erreichbar" r#type={AlertType::Warning}>
                    <p>{"Unter „Change Target“ eine andere Adresse oder andere Zugangsdaten wählen."}</p>
                    {setup}
                </Alert>)
            } else if !generated {
                html!(<Alert inline=true title="Keine Konfiguration erzeugt" r#type={AlertType::Warning}>
                    <p>{"Der Fehler steht oben; ältere RouterOS-Versionen melden Felder, die das Modell nicht kennt."}</p>
                </Alert>)
            } else if nothing_to_do {
                html!(<Alert inline=true title="Gerät entspricht NetBox" r#type={AlertType::Success}/>)
            } else {
                html! {<>
                    { for warnings.iter().map(|warning| html!(
                        <Alert inline=true title={warning.clone()} r#type={AlertType::Warning}/>
                    ))}
                    <CodeBlock><CodeBlockCode>{ cfg_update }</CodeBlockCode></CodeBlock>
                </>}
            };
            html! {<dl class="device-overview">
            <dt>{"Name"}</dt><dd>{ data.configured_name.as_ref() }</dd>
            <dt>{"Board"}</dt><dd>{ board }</dd>
            {routeros}
            <dt>{"Änderungen"}</dt><dd>
                {update}
                <Button
                    variant={ButtonVariant::Primary}
                    label="Anwenden"
                    disabled={nothing_to_do || running || update_running}
                    onclick={ctx.link().callback(|_| ShowDeviceMessage::Apply)}
                />
            </dd>
            </dl>}
        });
        let job = self.job.map(|id| html! {
            <JobView key={id} id={id} on_finished={ctx.link().callback(|_| ShowDeviceMessage::JobFinished)}/>
        });
        let bootstrapped = match &self.bootstrap {
            BootstrapState::Done(result) => Some(match (&result.address, &result.error) {
                (Some(address), _) => html! {
                    <Alert inline=true title={format!("Ersteinrichtung abgeschlossen, das Gerät antwortet an {address}")} r#type={AlertType::Success}>
                        <CodeBlock><CodeBlockCode>{ result.transcript.clone() }</CodeBlockCode></CodeBlock>
                    </Alert>
                },
                (None, error) => html! {
                    <Alert inline=true title="Ersteinrichtung fehlgeschlagen" r#type={AlertType::Danger}>
                        <p>{ error.clone().unwrap_or_default() }</p>
                        <CodeBlock><CodeBlockCode>{ result.transcript.clone() }</CodeBlockCode></CodeBlock>
                    </Alert>
                },
            }),
            _ => None,
        };
        html! {
            <>
            <ExpandableSection toggle_text_hidden="Change Target" toggle_text_expanded="Hide Target Selection">
                <AdjustTarget onchange={ctx.link().callback(ShowDeviceMessage::AdjustTarget)} value={self.alternate_target.clone()} />
            </ExpandableSection>
            {error}
            {job}
            {bootstrapped}
            {data}
            </>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
            running_job(ctx.link().clone(), self.id);
        }
    }
}

fn fetch_overview(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) = match target.credentials {
            SelectedCredentials::Default => (None, None),
            SelectedCredentials::Named(name) => (Some(name.into_string()), None),
            SelectedCredentials::Adhoc { username, password } => (
                None,
                Some(device_overview::AdhocCredentials {
                    username: Some(username.to_string()),
                    password: Some(password.to_string()),
                }),
            ),
        };
        match query_authenticated_response::<DeviceOverview, _>(
            scope.clone(),
            device_overview::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        {
            Ok(response) => {
                if let Some(data) = response.data {
                    let error = response
                        .errors
                        .filter(|e| !e.is_empty())
                        .map(FrontendError::Graphql);
                    let device = data.topology.device_by_id;
                    let data = device
                        .map(|mut d| ShowDeviceData {
                            configured_name: d.name.into_boxed_str(),
                            routeros: d.access.as_mut().and_then(|a| a.routeros.take()),
                            access: d.access.map(|a| {
                                (
                                    a.device_stats.routerboard.device_type.into_boxed_str(),
                                    a.generate_cfg.map(|cfg| GeneratedCfg {
                                        script: cfg.script.into_boxed_str(),
                                        warnings: cfg.warnings,
                                    }),
                                )
                            }),
                            bootstrap_plan: d.bootstrap_plan,
                            bootstrap_problem: d.bootstrap_problem,
                        })
                        .unwrap_or_default();

                    scope.send_message(ShowDeviceMessage::Data {
                        data,
                        error,
                    });
                } else if let Some(errors) = response.errors {
                    scope.send_message(ShowDeviceMessage::Error(FrontendError::Graphql(errors)))
                } else {
                    scope.send_message(ShowDeviceMessage::Error(FrontendError::MissingData))
                }
            }
            Err(e) => {
                scope.send_message(ShowDeviceMessage::Error(e));
            }
        }
    });
}

fn apply(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) = match target.credentials {
            SelectedCredentials::Default => (None, None),
            SelectedCredentials::Named(name) => (Some(name.into_string()), None),
            SelectedCredentials::Adhoc { username, password } => (
                None,
                Some(apply_device_cfg::AdhocCredentials {
                    username: Some(username.to_string()),
                    password: Some(password.to_string()),
                }),
            ),
        };
        let result = query_authenticated::<ApplyDeviceCfg, _>(
            scope.clone(),
            apply_device_cfg::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| data.apply_cfg.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

fn fetch_candidates(scope: Scope<ShowDevice>, id: u32) {
    spawn_local(async move {
        let result = query_authenticated::<BootstrapCandidates, _>(
            scope.clone(),
            bootstrap_candidates::Variables { id: id as i64 },
        )
        .await
        .map_err(|e| e.to_string())
        .map(|data| {
            data.topology
                .device_by_id
                .and_then(|d| d.bootstrap_plan)
                .map(|plan| plan.discovered)
                .unwrap_or_default()
        });
        scope.send_message(ShowDeviceMessage::Candidates(result));
    });
}

fn bootstrap(scope: Scope<ShowDevice>, id: u32, mac: String) {
    spawn_local(async move {
        let result = query_authenticated::<BootstrapDevice, _>(
            scope.clone(),
            bootstrap_device::Variables { id: id as i64, mac },
        )
        .await
        .map(|data| data.bootstrap);
        scope.send_message(ShowDeviceMessage::Bootstrapped(result));
    });
}

/// What the channel select shows, e.g. `stable: 7.20.8`.
fn channel_label(
    channel: &device_overview::DeviceOverviewTopologyDeviceByIdAccessRouterosChannels,
) -> Box<str> {
    let name = match &channel.channel {
        device_overview::Channel::LONG_TERM => "long-term",
        device_overview::Channel::STABLE => "stable",
        device_overview::Channel::TESTING => "testing",
        device_overview::Channel::DEVELOPMENT => "development",
        device_overview::Channel::Other(other) => other,
    };
    format!("{name}: {}", channel.newest).into()
}

/// Updates to the newest release of `channel`, or without one only adjusts the packages.
fn update(
    scope: Scope<ShowDevice>,
    id: u32,
    channel: Option<device_overview::Channel>,
    target: SelectedTarget,
) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) = match target.credentials {
            SelectedCredentials::Default => (None, None),
            SelectedCredentials::Named(name) => (Some(name.into_string()), None),
            SelectedCredentials::Adhoc { username, password } => (
                None,
                Some(update_routeros::AdhocCredentials {
                    username: Some(username.to_string()),
                    password: Some(password.to_string()),
                }),
            ),
        };
        let result = query_authenticated::<UpdateRouteros, _>(
            scope.clone(),
            update_routeros::Variables {
                id: id as i64,
                channel: channel.map(|channel| match channel {
                    device_overview::Channel::LONG_TERM => update_routeros::Channel::LONG_TERM,
                    device_overview::Channel::STABLE => update_routeros::Channel::STABLE,
                    device_overview::Channel::TESTING => update_routeros::Channel::TESTING,
                    device_overview::Channel::DEVELOPMENT => update_routeros::Channel::DEVELOPMENT,
                    device_overview::Channel::Other(other) => update_routeros::Channel::Other(other),
                }),
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| data.update_routeros.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Shows the job the device runs, should one have been started before the page was opened.
fn running_job(scope: Scope<ShowDevice>, id: u32) {
    spawn_local(async move {
        let Ok(data) = query_authenticated::<DeviceJobs, _>(
            scope.clone(),
            device_jobs::Variables { device_id: id as i64 },
        )
        .await
        else {
            return;
        };
        if let Some(job) = data
            .jobs
            .into_iter()
            .find(|job| matches!(job.state, device_jobs::JobState::RUNNING))
        {
            scope.send_message(ShowDeviceMessage::JobStarted(Ok(job.id)));
        }
    });
}
