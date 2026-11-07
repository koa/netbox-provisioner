//! Scope: The device page; not its cards (`components::device`).
use crate::routes::AppRoute;
use crate::target::SelectedTarget;
use crate::{
    components::{
        device::{
            adjust_target::AdjustTarget,
            batch_apply::BatchJob,
            changes::{Changes, GeneratedCfg},
            device_mode::DeviceMode,
            history::History,
            interfaces::Interfaces,
            leftovers::{Leftover, Leftovers},
            partitions::Partitions,
            routeros::{Routeros, UpdateRequest},
            setup::Setup,
        },
        job::JobView,
    },
    error::{FrontendError, ServerProblem},
    graphql::{
        Response,
        authenticated::{
            device::{
                AllowApply, ApplyDeviceCfg, BootPartition, BootPartitionVariables,
                BootstrapCandidates, BootstrapDevice, BootstrapPlan, BootstrapVariables,
                ChangeDeviceMode, ClearLeftover, ClearLeftoverVariables, Connected,
                DeviceMode as Mode, DeviceOverview, Discovered, DowngradeRouteros,
                DowngradeVariables, Leftovers as Left, LogEntry, Partitions as Parts, PowerCycle,
                PowerSupply, RebootDevice, RefreshDevice, RepartitionDevice, Routeros as Ros,
                TargetVariables, UpdateRouteros, UpdateVariables, Watchdog,
            },
            types::{DeviceVariables, ErrorInfo, JobState, LeftoverAction, Mac},
        },
        live::{Subscription, watch},
        path_segment, request,
    },
    i18n::{device as t, target},
    reading_time::{read_at, same_reading},
};
use cynic::{MutationBuilder, QueryBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode, Spinner, SpinnerSize,
};
use yew::{Component, Context, Html, Properties, html, html::Scope, platform::spawn_local};
use yew_nested_router::components::Link;

pub struct ShowDevice {
    id: u32,
    error: Option<FrontendError>,
    data: Option<ShowDeviceData>,
    alternate_target: SelectedTarget,
    /// the job shown (an update, an apply, a setup), started here or running when the page was opened
    job: Option<i32>,
    /// the device `job` works on when it isn't this one: a setup over this device as the neighbor
    job_through: Option<String>,
    job_running: bool,
    /// the jobs of a batch started here (this device first), shown side by side instead of `job`
    batch_jobs: Vec<BatchJob>,
    /// how many of them are over
    batch_finished: usize,
    /// what the bootstrap neighbor sees on its port; `None` while it is asked
    candidates: Option<Result<Vec<Discovered>, String>>,
    /// the supplier's job switching the power off and on, next to this device's own
    power_job: Option<i32>,
    /// the form for another address or other credentials is open (in the head; on an
    /// unreachable device it is always shown, in the warning)
    show_target: bool,
    /// counts the jobs over (NetBox written, the device changed): the interfaces are read again
    jobs_finished: u32,
    /// the job running is an apply (alone or in a batch)
    applying: bool,
    /// counts the applies over: the interfaces written to are no longer changed
    applied: u32,
    /// the overview subscribed to (`graphql::live`): shown again whenever the device changed
    live: Option<Subscription>,
    /// writing to devices is allowed (`allow_apply`); else the buttons that write are disabled
    allow_apply: bool,
}
#[derive(Debug, Default)]
pub struct ShowDeviceData {
    configured_name: Box<str>,
    /// board and planned changes (`None`: the generator failed); `None` if the device couldn't
    /// be reached
    access: Option<(Box<str>, Option<GeneratedCfg>)>,
    /// the device the connection reached, `None` if it couldn't be reached
    connected: Option<Connected>,
    /// why it couldn't be reached
    access_problem: Option<Box<str>>,
    routeros: Option<Ros>,
    /// what a job that didn't see its work to the end left on the device
    leftovers: Option<Left>,
    /// the newest entries of the device's log, oldest first
    log: Vec<LogEntry>,
    /// `None` before RouterOS 7.17, or when it couldn't be read
    device_mode: Option<Mode>,
    /// `None` without partitions (16 MiB of flash), or when they couldn't be read
    partitions: Option<Parts>,
    bootstrap_plan: Option<BootstrapPlan>,
    bootstrap_problem: Option<ErrorInfo>,
    /// the watchdogs that may fire while this one updates, of the devices powering it and of
    /// those reaching the network through it
    watchdogs: Vec<Watchdog>,
    /// the neighbors it stays reachable through after an update that cut it off from OSPF
    update_neighbors: Vec<String>,
    /// the device feeding it over PoE and its port: a cold boot is possible
    power_supply: Option<PowerSupply>,
    /// the job running on the device, and the device it works on when that isn't this one (a
    /// setup over this device as the neighbor): also one started elsewhere shows
    running_job: Option<(i32, Option<String>)>,
    /// the jobs of the device over, also those that ran elsewhere: when one more is, the
    /// history and the interfaces are read again
    jobs_over: Vec<i32>,
}
#[derive(Debug, Properties, Clone, PartialEq)]
pub struct ShowDeviceProps {
    pub id: u32,
}

#[derive(Debug)]
pub enum ShowDeviceMessage {
    Error(FrontendError),
    Data {
        data: Box<ShowDeviceData>,
        error: Option<FrontendError>,
    },
    AdjustTarget(SelectedTarget),
    Apply,
    Candidates(Result<Vec<Discovered>, String>),
    /// set up the device with this MAC
    Bootstrap(String),
    /// update in a channel, or only the packages
    Update(UpdateRequest),
    /// take care of something a job left on the device
    ClearLeftover(Leftover),
    /// downgrade to the version (`channel` of the request is `None`)
    Downgrade(String, UpdateRequest),
    Reboot,
    PowerCycle,
    PowerCycleStarted(Result<i32, FrontendError>),
    ChangeDeviceMode,
    Repartition,
    BootPartition(String),
    /// open or close the form for another address or other credentials
    ShowTarget(bool),
    JobStarted(Result<i32, FrontendError>),
    /// a job of another device running through this one (its id and that device)
    JobThrough(i32, String),
    JobFinished,
    /// read the device anew (`refreshDevice`): every page showing it does
    Reload,
    AllowApply(bool),
    /// „Anwenden“ of the interfaces: to the card „Änderungen“
    ShowChanges,
    BatchStarted(Vec<BatchJob>),
    BatchJobFinished,
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
            job: None,
            job_through: None,
            job_running: false,
            batch_jobs: Vec::new(),
            batch_finished: 0,
            candidates: None,
            power_job: None,
            show_target: false,
            jobs_finished: 0,
            applying: false,
            applied: 0,
            live: None,
            allow_apply: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ShowDeviceMessage::Error(e) => {
                self.error = Some(e);
                true
            }
            ShowDeviceMessage::Data { data, error } => {
                self.take_data(ctx, *data, error);
                true
            }
            ShowDeviceMessage::Reload => {
                let (scope, id) = (ctx.link().clone(), self.id);
                spawn_local(async move {
                    let refresh = RefreshDevice::build(DeviceVariables { id: id as i32 });
                    if let Err(error) = request(scope.clone(), refresh).await {
                        scope.send_message(ShowDeviceMessage::Error(error));
                    }
                });
                false
            }
            ShowDeviceMessage::AllowApply(allow) => {
                self.allow_apply = allow;
                true
            }
            ShowDeviceMessage::Apply => {
                self.job_running = true;
                self.applying = true;
                apply(ctx.link().clone(), self.id, &self.alternate_target);
                true
            }
            ShowDeviceMessage::Update(request) => {
                self.job_running = true;
                update(ctx.link().clone(), self.id, request, &self.alternate_target);
                true
            }
            ShowDeviceMessage::ClearLeftover(leftover) => {
                self.job_running = true;
                clear(
                    ctx.link().clone(),
                    self.id,
                    leftover,
                    &self.alternate_target,
                );
                true
            }
            ShowDeviceMessage::Downgrade(version, request) => {
                self.job_running = true;
                downgrade(
                    ctx.link().clone(),
                    self.id,
                    version,
                    request,
                    &self.alternate_target,
                );
                true
            }
            ShowDeviceMessage::Reboot => {
                self.job_running = true;
                reboot(ctx.link().clone(), self.id, &self.alternate_target);
                true
            }
            ShowDeviceMessage::PowerCycle => {
                cut_power(ctx.link().clone(), self.id);
                true
            }
            ShowDeviceMessage::PowerCycleStarted(result) => {
                match result {
                    Ok(id) => self.power_job = Some(id),
                    Err(error) => self.error = Some(error),
                }
                true
            }
            ShowDeviceMessage::ChangeDeviceMode => {
                self.job_running = true;
                change_mode(ctx.link().clone(), self.id, &self.alternate_target);
                true
            }
            ShowDeviceMessage::Repartition => {
                self.job_running = true;
                repartition(ctx.link().clone(), self.id, &self.alternate_target);
                true
            }
            ShowDeviceMessage::BootPartition(partition) => {
                self.job_running = true;
                boot_from(
                    ctx.link().clone(),
                    self.id,
                    partition,
                    &self.alternate_target,
                );
                true
            }
            ShowDeviceMessage::ShowTarget(show) => {
                self.show_target = show;
                true
            }
            ShowDeviceMessage::JobThrough(id, device) => {
                self.job = Some(id);
                self.job_through = Some(device);
                self.job_running = true;
                self.batch_jobs.clear();
                true
            }
            ShowDeviceMessage::JobStarted(result) => {
                match result {
                    Ok(id) => {
                        self.job = Some(id);
                        self.job_through = None;
                        self.job_running = true;
                        self.batch_jobs.clear();
                    }
                    Err(error) => {
                        self.job_running = false;
                        self.error = Some(error);
                    }
                }
                true
            }
            ShowDeviceMessage::BatchStarted(jobs) => {
                self.job = None;
                self.batch_jobs = jobs;
                self.batch_finished = 0;
                self.job_running = true;
                self.applying = true;
                true
            }
            ShowDeviceMessage::BatchJobFinished => {
                self.batch_finished += 1;
                if self.batch_finished >= self.batch_jobs.len() {
                    ctx.link().send_message(ShowDeviceMessage::JobFinished);
                }
                false
            }
            ShowDeviceMessage::JobFinished => {
                self.job_running = false;
                self.jobs_finished += 1;
                if self.applying {
                    self.applying = false;
                    self.applied += 1;
                }
                true
            }
            ShowDeviceMessage::ShowChanges => {
                let card = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.query_selector(".device-cfg").ok().flatten());
                if let Some(card) = card {
                    card.scroll_into_view();
                }
                false
            }
            ShowDeviceMessage::Candidates(candidates) => {
                self.candidates = Some(candidates);
                true
            }
            ShowDeviceMessage::Bootstrap(mac) => {
                self.job_running = true;
                bootstrap(ctx.link().clone(), self.id, mac);
                true
            }
            ShowDeviceMessage::AdjustTarget(t) => {
                if self.alternate_target != t {
                    self.alternate_target = t;
                    self.start_live(ctx);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let error = self.error.as_ref().map(|e| e.to_html());
        let job = self.job_view(ctx);
        let content = match &self.data {
            Some(data) => self.data_view(ctx, data, job),
            // a job started before the overview is there (it reloads after each job)
            None => html! {<>{job}<Spinner size={SpinnerSize::Md}/></>},
        };
        html! {
            <div class="device-page">
            {error}
            {content}
            </div>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            self.start_live(ctx);
            let scope = ctx.link().clone();
            spawn_local(async move {
                let message = match request(scope.clone(), AllowApply::build(())).await {
                    Ok(data) => ShowDeviceMessage::AllowApply(data.allow_apply),
                    Err(error) => ShowDeviceMessage::Error(error),
                };
                scope.send_message(message);
            });
        }
    }
}

impl ShowDevice {
    /// Takes a new overview: the setup's candidates asked again where it is unreachable, a job
    /// running elsewhere shown.
    fn take_data(
        &mut self,
        ctx: &Context<Self>,
        data: ShowDeviceData,
        error: Option<FrontendError>,
    ) {
        if data.access.is_none() && data.bootstrap_plan.is_some() {
            self.candidates = None;
            fetch_candidates(ctx.link().clone(), self.id);
        }
        if self
            .data
            .as_ref()
            .is_some_and(|before| before.jobs_over != data.jobs_over)
        {
            self.jobs_finished += 1;
        }
        if let Some((id, through)) = data.running_job.clone()
            && self.job != Some(id)
            && self.batch_jobs.is_empty()
        {
            self.job = Some(id);
            self.job_through = through;
            self.job_running = true;
        }
        let mut data = data;
        let as_of = |data: &ShowDeviceData| {
            data.access
                .as_ref()
                .and_then(|(_, cfg)| cfg.as_ref())
                .map(|cfg| cfg.as_of)
        };
        let before = self.data.as_ref().and_then(as_of);
        if let Some(cfg) = data.access.as_mut().and_then(|(_, cfg)| cfg.as_mut()) {
            cfg.as_of = same_reading(before, cfg.as_of);
        }
        self.data = Some(data);
        self.error = error;
    }
    /// The job shown, or those of a batch side by side.
    fn job_view(&self, ctx: &Context<Self>) -> Option<Html> {
        if self.batch_jobs.is_empty() {
            return self.job.map(|id| html! {<>
                if let Some(device) = &self.job_through {
                    <p>{t::job_through(device)}</p>
                }
                <JobView key={id} id={id} on_finished={ctx.link().callback(|_| ShowDeviceMessage::JobFinished)}/>
            </>});
        }
        Some(html! {
            <div class="batch-jobs">
                { for self.batch_jobs.iter().map(|job| html! {
                    <div class="batch-job" key={job.job}>
                        <h3>
                            if let Some(site) = job.site {
                                <Link<AppRoute>
                                    to={AppRoute::device(site as u32, job.device as u32)}
                                >
                                    {&job.name}
                                </Link<AppRoute>>
                            } else {
                                {&job.name}
                            }
                        </h3>
                        <JobView id={job.job} on_finished={ctx.link().callback(|_| ShowDeviceMessage::BatchJobFinished)}/>
                    </div>
                })}
            </div>
        })
    }
    /// The page once the overview is there, `job` below the head.
    fn data_view(&self, ctx: &Context<Self>, data: &ShowDeviceData, job: Option<Html>) -> Html {
        // a job runs, or writing is locked
        let running = self.job_running || !self.allow_apply;
        let locked = (!self.allow_apply).then(|| {
            html! {
                <Alert inline=true title={t::LOCKED} r#type={AlertType::Info}>
                    <p>{t::LOCKED_BEFORE}<code>{"allow_apply: true"}</code>{t::LOCKED_AFTER}</p>
                </Alert>
            }
        });
        let routeros = data.routeros.as_ref().map(|routeros| {
            html! {
                <Routeros
                    routeros={routeros.clone()}
                    watchdogs={data.watchdogs.clone()}
                    power_supply={data.power_supply.clone()}
                    no_way_back={no_way_back(data)}
                    disabled={running}
                    allow_apply={self.allow_apply}
                    power_job={self.power_job}
                    on_update={ctx.link().callback(ShowDeviceMessage::Update)}
                    on_downgrade={ctx.link().callback(|(version, request)| {
                        ShowDeviceMessage::Downgrade(version, request)
                    })}
                    on_reboot={ctx.link().callback(|()| ShowDeviceMessage::Reboot)}
                    on_power_cycle={ctx.link().callback(|()| ShowDeviceMessage::PowerCycle)}
                />
            }
        });
        let device_mode = data.device_mode.as_ref().map(|mode| {
            html! {
                <DeviceMode
                    mode={mode.clone()}
                    disabled={running}
                    on_change={ctx.link().callback(|()| ShowDeviceMessage::ChangeDeviceMode)}
                />
            }
        });
        let partitions = data.partitions.as_ref().map(|partitions| {
            html! {
                <Partitions
                    partitions={partitions.clone()}
                    disabled={running}
                    on_split={ctx.link().callback(|()| ShowDeviceMessage::Repartition)}
                    on_boot={ctx.link().callback(ShowDeviceMessage::BootPartition)}
                />
            }
        });
        let device =
            (routeros.is_some() || device_mode.is_some() || partitions.is_some()).then(|| {
                html! {
                    <Card class="device-state">
                        <CardHeader><CardTitle>{t::DEVICE}</CardTitle></CardHeader>
                        <CardBody>
                            <dl class="device-overview">
                                {routeros}
                                {device_mode}
                                {partitions}
                            </dl>
                        </CardBody>
                    </Card>
                }
            });
        html! {<>
            {locked}
            {self.head_view(ctx, data)}
            if let Some(left) = &data.leftovers {
                <Leftovers
                    left={left.clone()}
                    disabled={running}
                    on_clear={ctx.link().callback(ShowDeviceMessage::ClearLeftover)}
                />
            }
            {job}
            <Interfaces
                id={self.id}
                refresh={self.jobs_finished}
                applied={self.applied}
                target={self.alternate_target.clone()}
                on_written={ctx.link().callback(|()| ShowDeviceMessage::JobFinished)}
                on_apply={ctx.link().callback(|()| ShowDeviceMessage::ShowChanges)}
            />
            <div class="device-columns">
                {device}
                <Changes
                    id={self.id}
                    reachable={data.access.is_some()}
                    cfg={data.access.as_ref().and_then(|(_, cfg)| cfg.clone())}
                    access_problem={data.access_problem.clone()}
                    disabled={running}
                    on_reload={ctx.link().callback(|()| ShowDeviceMessage::Reload)}
                    on_apply={ctx.link().callback(|()| ShowDeviceMessage::Apply)}
                    on_batch_started={ctx.link().callback(ShowDeviceMessage::BatchStarted)}
                    setup={html! {
                        <Setup
                            name={data.configured_name.to_string()}
                            plan={data.bootstrap_plan.clone()}
                            problem={data.bootstrap_problem.clone()}
                            candidates={self.candidates.clone()}
                            disabled={running}
                            on_setup={ctx.link().callback(ShowDeviceMessage::Bootstrap)}
                        />
                    }}
                >
                    {self.target_form(ctx)}
                </Changes>
            </div>
            {log_view(&data.log)}
            <History id={self.id} refresh={self.jobs_finished}/>
        </>}
    }
    /// The form for another address or other credentials.
    fn target_form(&self, ctx: &Context<Self>) -> Html {
        html! {
            <AdjustTarget onchange={ctx.link().callback(ShowDeviceMessage::AdjustTarget)} value={self.alternate_target.clone()} />
        }
    }
    /// The head: name, board, the device the connection reached and how it is reached.
    fn head_view(&self, ctx: &Context<Self>, data: &ShowDeviceData) -> Html {
        let reachable = data.access.is_some();
        let board = data
            .access
            .as_ref()
            .map(|(board, _)| board.as_ref())
            .unwrap_or_default();
        html! {
            <Card class="device-head">
                <CardHeader><CardTitle>{ data.configured_name.as_ref() }</CardTitle></CardHeader>
                <CardBody>
                    <dl class="device-overview">
                        if reachable {
                            <dt>{t::BOARD}</dt><dd>{ board }</dd>
                        }
                        if let Some(connected) = &data.connected {
                            <dt>{t::CONNECTED}</dt><dd>
                                { t::connected(&connected.address, connected.neighbor.as_deref(), &connected.identity) }
                                if !connected.same_name {
                                    <Alert inline=true r#type={AlertType::Warning}
                                        title={t::other_identity(&connected.identity, &data.configured_name)}/>
                                }
                            </dd>
                        }
                        <dt>{target::ACCESS}</dt><dd>
                            { target::describe(&self.alternate_target) }
                            if reachable {
                                {" "}
                                <Button
                                    variant={ButtonVariant::Link}
                                    label={if self.show_target { target::HIDE } else { target::SHOW }}
                                    onclick={ctx.link().callback({
                                        let show = !self.show_target;
                                        move |_| ShowDeviceMessage::ShowTarget(show)
                                    })}
                                />
                            }
                        </dd>
                    </dl>
                    if reachable && self.show_target {
                        {self.target_form(ctx)}
                    }
                </CardBody>
            </Card>
        }
    }
    /// Subscribes to the overview with the target chosen, ending the subscription before.
    fn start_live(&mut self, ctx: &Context<Self>) {
        let (id, scope) = (self.id, ctx.link().clone());
        let variables = target_variables(id, &self.alternate_target);
        self.live = None;
        let overview = DeviceOverview::build(variables);
        self.live = Some(watch(ctx.link(), overview, move |response| {
            scope.send_message(match response {
                Ok(response) => overview_message(id, response),
                Err(error) => ShowDeviceMessage::Error(error),
            })
        }));
    }
}

/// Whether an update would leave the device without a way back: no second partition with
/// the running RouterOS to switch back to, and no neighbor reaching it (also when the
/// partitions couldn't be read, as the backend then finds none either).
fn no_way_back(data: &ShowDeviceData) -> bool {
    let partitions = data
        .partitions
        .as_ref()
        .map(|p| p.partitions.as_slice())
        .unwrap_or_default();
    let two = matches!(partitions, [a, b] if (a.running && a.active) || (b.running && b.active));
    !two && data.update_neighbors.is_empty()
}

/// The newest entries of the device's log, folded.
fn log_view(log: &[LogEntry]) -> Html {
    if log.is_empty() {
        return Html::default();
    }
    html! {
        <Card class="device-log">
            <CardHeader><CardTitle>{t::LOG}</CardTitle></CardHeader>
            <CardBody>
                <details>
                    <summary>{t::log_entries(log.len())}</summary>
                    <CodeBlock>
                        <CodeBlockCode>
                            { for log.iter().map(|entry| format!("{} {} {}\n", entry.time, entry.topics, entry.message)) }
                        </CodeBlockCode>
                    </CodeBlock>
                </details>
            </CardBody>
        </Card>
    }
}

/// The variables of an operation on device `id` reached through `target`.
fn target_variables(id: u32, target: &SelectedTarget) -> TargetVariables {
    TargetVariables {
        id: id as i32,
        target: target.input(),
    }
}

/// What an answer of the overview shows.
fn overview_message(id: u32, response: Response<DeviceOverview>) -> ShowDeviceMessage {
    if let Some(data) = response.data {
        // why the device couldn't be reached goes with the warning, not the banner
        let (access_errors, errors): (Vec<_>, Vec<_>) = response
            .errors
            .unwrap_or_default()
            .into_iter()
            .partition(|e| {
                e.path.as_ref().is_some_and(|path| {
                    path.iter()
                        .map(path_segment)
                        .eq(["topology", "deviceById", "access"])
                })
            });
        let access_problem = access_errors
            .first()
            .map(|e| ServerProblem::of(e).text().into_boxed_str());
        let error = Some(errors)
            .filter(|e| !e.is_empty())
            .map(FrontendError::Graphql);
        let jobs = data
            .topology
            .device_by_id
            .as_ref()
            .map(|d| d.jobs.as_slice())
            .unwrap_or_default();
        let running_job = jobs
            .iter()
            .find(|job| job.state == JobState::Running)
            .map(|job| {
                let through = match (job.device_id, &job.device_name) {
                    (Some(device), Some(name)) if device != id as i32 => Some(name.clone()),
                    _ => None,
                };
                (job.id, through)
            });
        let jobs_over = jobs
            .iter()
            .filter(|job| job.state != JobState::Running)
            .map(|job| job.id)
            .collect();
        let device = data.topology.device_by_id;
        let data = device
            .map(|mut d| ShowDeviceData {
                configured_name: d.name.into_boxed_str(),
                connected: d.access.as_ref().map(|a| a.connected.clone()),
                access_problem,
                routeros: d.access.as_mut().and_then(|a| a.routeros.take()),
                log: d
                    .access
                    .as_mut()
                    .and_then(|a| a.log.take())
                    .unwrap_or_default(),
                leftovers: d.access.as_mut().and_then(|a| a.leftovers.take()),
                device_mode: d.access.as_mut().and_then(|a| a.device_mode.take()),
                partitions: d.access.as_mut().and_then(|a| a.partitions.take()),
                access: d.access.map(|a| {
                    (
                        a.device_stats.routerboard.device_type.into_boxed_str(),
                        a.generate_cfg.map(|cfg| GeneratedCfg {
                            script: cfg.script.into_boxed_str(),
                            warnings: cfg.warnings,
                            previous_password: cfg.previous_password,
                            as_of: read_at(cfg.age),
                        }),
                    )
                }),
                bootstrap_plan: d.bootstrap_plan,
                bootstrap_problem: d.bootstrap_problem,
                watchdogs: d.watchdogs,
                update_neighbors: d.update_neighbors,
                power_supply: d.power_supply,
                running_job,
                jobs_over,
            })
            .unwrap_or_default();
        ShowDeviceMessage::Data {
            data: Box::new(data),
            error,
        }
    } else if let Some(errors) = response.errors {
        ShowDeviceMessage::Error(FrontendError::Graphql(errors))
    } else {
        ShowDeviceMessage::Error(FrontendError::MissingData)
    }
}

fn apply(scope: Scope<ShowDevice>, id: u32, target: &SelectedTarget) {
    let apply = ApplyDeviceCfg::build(target_variables(id, target));
    spawn_local(async move {
        let result = request(scope.clone(), apply)
            .await
            .map(|data| data.apply_cfg.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

fn fetch_candidates(scope: Scope<ShowDevice>, id: u32) {
    spawn_local(async move {
        let query = BootstrapCandidates::build(DeviceVariables { id: id as i32 });
        let result = request(scope.clone(), query)
            .await
            .map_err(|e| e.message())
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
        let setup = BootstrapDevice::build(BootstrapVariables {
            id: id as i32,
            mac: Mac(mac),
        });
        let result = request(scope.clone(), setup)
            .await
            .map(|data| data.bootstrap.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Updates to the newest release of the request's channel, or without one only adjusts the
/// packages.
fn update(scope: Scope<ShowDevice>, id: u32, asked: UpdateRequest, target: &SelectedTarget) {
    let (id, target) = (id as i32, target.input());
    let update = UpdateRouteros::build(UpdateVariables {
        id,
        channel: asked.channel,
        target,
        suspend_watchdogs: asked.suspend_watchdogs.then_some(true),
        without_fallback: asked.without_fallback.then_some(true),
    });
    spawn_local(async move {
        let result = request(scope.clone(), update)
            .await
            .map(|data| data.update_routeros.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Takes RouterOS back to the older `version`, with the choices `asked` as for
/// [`update`].
fn downgrade(
    scope: Scope<ShowDevice>,
    id: u32,
    version: String,
    asked: UpdateRequest,
    target: &SelectedTarget,
) {
    let (id, target) = (id as i32, target.input());
    let downgrade = DowngradeRouteros::build(DowngradeVariables {
        id,
        version,
        target,
        suspend_watchdogs: asked.suspend_watchdogs.then_some(true),
        without_fallback: asked.without_fallback.then_some(true),
    });
    spawn_local(async move {
        let result = request(scope.clone(), downgrade)
            .await
            .map(|data| data.downgrade_routeros.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Takes care of something a job left on the device.
fn clear(scope: Scope<ShowDevice>, id: u32, leftover: Leftover, target: &SelectedTarget) {
    use LeftoverAction as A;
    let action = match leftover {
        Leftover::StopApplyFallback => A::StopApplyFallback,
        Leftover::ApplyFallbackNow => A::ApplyFallbackNow,
        Leftover::StopUpdateFallback => A::StopUpdateFallback,
        Leftover::RestoreWatchdogs => A::RestoreWatchdogs,
        Leftover::RemovePackages => A::RemovePackages,
    };
    let (id, target) = (id as i32, target.input());
    let clear = ClearLeftover::build(ClearLeftoverVariables { id, action, target });
    spawn_local(async move {
        let result = request(scope.clone(), clear)
            .await
            .map(|data| data.clear_leftover.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Reboots the device and waits until it is back.
fn reboot(scope: Scope<ShowDevice>, id: u32, target: &SelectedTarget) {
    let reboot = RebootDevice::build(target_variables(id, target));
    spawn_local(async move {
        let result = request(scope.clone(), reboot)
            .await
            .map(|data| data.reboot_device.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Switches the PoE of the device feeding it off and on: a job of that device.
fn cut_power(scope: Scope<ShowDevice>, id: u32) {
    let cycle = PowerCycle::build(DeviceVariables { id: id as i32 });
    spawn_local(async move {
        let result = request(scope.clone(), cycle)
            .await
            .map(|data| data.power_cycle.id);
        scope.send_message(ShowDeviceMessage::PowerCycleStarted(result));
    });
}

/// Turns on the features of the device mode the provisioner needs; the device then waits for
/// the confirmation at the device.
fn change_mode(scope: Scope<ShowDevice>, id: u32, target: &SelectedTarget) {
    let change = ChangeDeviceMode::build(target_variables(id, target));
    spawn_local(async move {
        let result = request(scope.clone(), change)
            .await
            .map(|data| data.change_device_mode.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Splits the flash in two partitions; the device reformats it and reboots.
fn repartition(scope: Scope<ShowDevice>, id: u32, target: &SelectedTarget) {
    let split = RepartitionDevice::build(target_variables(id, target));
    spawn_local(async move {
        let result = request(scope.clone(), split)
            .await
            .map(|data| data.repartition_device.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Boots the device from `partition`.
fn boot_from(scope: Scope<ShowDevice>, id: u32, partition: String, target: &SelectedTarget) {
    let (id, target) = (id as i32, target.input());
    let boot = BootPartition::build(BootPartitionVariables {
        id,
        partition,
        target,
    });
    spawn_local(async move {
        let result = request(scope.clone(), boot)
            .await
            .map(|data| data.boot_partition.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}
