//! Updates across the network (`/rollout`, `docs/concepts/network-update.md`): the sites and
//! the channel chosen, what a rollout would do read from every RouterOS device of them
//! (`rolloutPlan`, only reading) and shown in the order it would update them, the pilots
//! changeable. Started (`startRollout`), the page follows the job (its own plan, read again,
//! and the state of each device) and steers it: on after a pause, a device skipped, the end.
//! Opened again, it follows the newest rollout.
use crate::{
    components::widgets::popup::Select,
    error::{FrontendError, ServerProblem},
    graphql::{
        POLL, RETRY, Sendable,
        authenticated::{
            job::{CancelJob, CancelJobVariables},
            list_devices::{ListSites, Site},
            rollout::{
                ContinueRollout, ContinueRolloutVariables, PlannedDevice, RolloutDevice,
                RolloutDeviceState as State, RolloutDone, RolloutEvent, RolloutEvents,
                RolloutEventsVariables, RolloutFallback, RolloutJob, RolloutPause, RolloutPaused,
                RolloutPlan, RolloutPlanQuery, RolloutPlanVariables, RolloutSkip, RolloutStatus,
                SkipRolloutDevice, SkipRolloutDeviceVariables, StartRollout, StartRolloutVariables,
            },
            types::{Channel, ErrorInfo, JobState},
        },
        live::{Subscription, watch_data},
        request,
    },
    i18n::rollout as t,
    routes::AppRoute,
};
use cynic::{MutationBuilder, QueryBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner,
    SpinnerSize,
};
use std::{
    cell::Cell,
    collections::{BTreeSet, HashMap},
    fmt,
    rc::Rc,
};
use yew::{
    Component, Context, Html, html,
    html::Scope,
    platform::{spawn_local, time::sleep},
};
use yew_nested_router::components::Link;

pub struct Rollout {
    sites: Option<Result<Vec<Site>, FrontendError>>,
    chosen: BTreeSet<i32>,
    channel: ChannelChoice,
    allow_apply: bool,
    plan: Plan,
    /// the devices chosen as pilots in the plan shown
    pilots: BTreeSet<i32>,
    job: Option<Following>,
    /// what a click on start, continue, skip or cancel failed with
    error: Option<FrontendError>,
    /// the poll of the job followed goes on while set
    alive: Rc<Cell<bool>>,
    /// the sites again when NetBox changed
    _subscription: Subscription,
}

enum Plan {
    None,
    Planning,
    Done(RolloutPlan),
    Failed(FrontendError),
}

/// The rollout job followed, as far as its events tell.
struct Following {
    id: i32,
    state: JobState,
    error: Option<ErrorInfo>,
    plan: Option<RolloutPlan>,
    /// the newest event of each device
    devices: HashMap<i32, RolloutDevice>,
    /// waiting for „Weiter“
    paused: Option<RolloutPaused>,
    done: Option<RolloutDone>,
    lost: bool,
    poll_error: Option<FrontendError>,
}

pub enum RolloutMsg {
    Status(Result<RolloutStatus, FrontendError>),
    Sites(Result<Vec<Site>, FrontendError>),
    Toggle(i32),
    Channel(ChannelChoice),
    Plan,
    Planned(Result<RolloutPlan, FrontendError>),
    TogglePilot(i32),
    Start,
    Started(Result<i32, FrontendError>),
    Loaded(RolloutJob),
    PollFailed(FrontendError),
    Lost,
    Continue,
    Skip(i32),
    Cancel,
    Acted(Result<(), FrontendError>),
}

/// The channel to update to; `None`: the config's (`routeros_channel`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChannelChoice(Option<Channel>);

impl fmt::Display for ChannelChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(channel) => f.write_str(channel.name()),
            None => f.write_str(t::DEFAULT_CHANNEL),
        }
    }
}

impl Component for Rollout {
    type Message = RolloutMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let subscription = watch_data(ctx.link(), ListSites::build(()), move |data| {
            scope.send_message(RolloutMsg::Sites(data.map(|data| data.topology.all_sites)));
        });
        let scope = ctx.link().clone();
        spawn_local(async move {
            let status = request(scope.clone(), RolloutStatus::build(())).await;
            scope.send_message(RolloutMsg::Status(status));
        });
        Self {
            sites: None,
            chosen: BTreeSet::new(),
            channel: ChannelChoice::default(),
            allow_apply: false,
            plan: Plan::None,
            pilots: BTreeSet::new(),
            job: None,
            error: None,
            alive: Rc::new(Cell::new(false)),
            _subscription: subscription,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            RolloutMsg::Status(Ok(status)) => {
                self.allow_apply = status.allow_apply;
                if let (Some(job), None) = (status.rollout_job, &self.job) {
                    self.follow(ctx, job.id);
                }
            }
            RolloutMsg::Status(Err(error)) => self.error = Some(error),
            RolloutMsg::Sites(sites) => self.sites = Some(sites),
            RolloutMsg::Toggle(site) => {
                if !self.chosen.remove(&site) {
                    self.chosen.insert(site);
                }
            }
            RolloutMsg::Channel(channel) => self.channel = channel,
            RolloutMsg::Plan => {
                self.stop_following();
                self.plan = Plan::Planning;
                let scope = ctx.link().clone();
                let variables = RolloutPlanVariables {
                    sites: self.chosen.iter().copied().collect(),
                    channel: self.channel.0.clone(),
                };
                spawn_local(async move {
                    let plan = request(scope.clone(), RolloutPlanQuery::build(variables)).await;
                    scope.send_message(RolloutMsg::Planned(plan.map(|data| data.rollout_plan)));
                });
            }
            RolloutMsg::Planned(Ok(plan)) => {
                self.pilots = pilots_of(&plan);
                self.plan = Plan::Done(plan);
            }
            RolloutMsg::Planned(Err(error)) => self.plan = Plan::Failed(error),
            RolloutMsg::TogglePilot(device) => {
                if !self.pilots.remove(&device) {
                    self.pilots.insert(device);
                }
            }
            RolloutMsg::Start => {
                let Plan::Done(plan) = &self.plan else {
                    return false;
                };
                let pilots =
                    (self.pilots != pilots_of(plan)).then(|| self.pilots.iter().copied().collect());
                let variables = StartRolloutVariables {
                    sites: self.chosen.iter().copied().collect(),
                    channel: self.channel.0.clone(),
                    pilots,
                };
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let started = request(scope.clone(), StartRollout::build(variables)).await;
                    scope.send_message(RolloutMsg::Started(
                        started.map(|data| data.start_rollout.id),
                    ));
                });
            }
            RolloutMsg::Started(Ok(id)) => {
                self.error = None;
                self.plan = Plan::None;
                self.follow(ctx, id);
            }
            RolloutMsg::Started(Err(error)) => self.error = Some(error),
            RolloutMsg::Loaded(loaded) => {
                if let Some(job) = &mut self.job {
                    job.take(loaded);
                }
            }
            RolloutMsg::PollFailed(error) => {
                if let Some(job) = &mut self.job {
                    job.poll_error = Some(error);
                }
            }
            RolloutMsg::Lost => {
                if let Some(job) = &mut self.job {
                    job.lost = true;
                }
            }
            RolloutMsg::Continue => {
                let Some(job) = self.job.as_ref().map(|job| job.id) else {
                    return false;
                };
                act(
                    ctx,
                    ContinueRollout::build(ContinueRolloutVariables { job }),
                );
            }
            RolloutMsg::Skip(device) => {
                let Some(job) = self.job.as_ref().map(|job| job.id) else {
                    return false;
                };
                let variables = SkipRolloutDeviceVariables { job, device };
                act(ctx, SkipRolloutDevice::build(variables));
            }
            RolloutMsg::Cancel => {
                let Some(id) = self.job.as_ref().map(|job| job.id) else {
                    return false;
                };
                act(ctx, CancelJob::build(CancelJobVariables { id }));
            }
            RolloutMsg::Acted(result) => self.error = result.err(),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let sites = match &self.sites {
            None => html!(<Spinner/>),
            Some(Err(error)) => error.to_html(),
            Some(Ok(sites)) => html! {
                <ul class="rollout-sites">
                    { for sites.iter().map(|site| {
                        let id = site.id;
                        html! {
                            <li>
                                <label>
                                    <input
                                        type="checkbox"
                                        checked={self.chosen.contains(&id)}
                                        onchange={ctx.link().callback(move |_| RolloutMsg::Toggle(id))}
                                    />
                                    {" "}{site.name.clone()}
                                </label>
                            </li>
                        }
                    }) }
                </ul>
            },
        };
        let channels = [
            None,
            Some(Channel::LongTerm),
            Some(Channel::Stable),
            Some(Channel::Testing),
            Some(Channel::Development),
        ]
        .into_iter()
        .map(ChannelChoice)
        .collect::<Vec<_>>();
        let running = self
            .job
            .as_ref()
            .is_some_and(|job| job.state == JobState::Running && !job.lost);
        let planning = matches!(self.plan, Plan::Planning);
        let label = match (&self.plan, &self.job) {
            (Plan::None, None) => t::PLAN,
            _ => t::PLAN_AGAIN,
        };
        let below = match (&self.job, &self.plan) {
            (Some(job), _) => self.job_view(ctx, job),
            (None, Plan::None) => Html::default(),
            (None, Plan::Planning) => html!(<Spinner/>),
            (None, Plan::Failed(error)) => error.to_html(),
            (None, Plan::Done(plan)) => self.plan_view(ctx, plan),
        };
        html! {
            <Card>
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{t::INTRO}</p>
                    <h3>{t::SITES}</h3>
                    {sites}
                    <h3>{t::CHANNEL}</h3>
                    <Select<ChannelChoice>
                        entries={channels}
                        selected={Some(self.channel.clone())}
                        onselect={ctx.link().callback(RolloutMsg::Channel)}
                    />
                    <p>
                        <Button
                            variant={ButtonVariant::Secondary}
                            disabled={planning || running || self.chosen.is_empty()}
                            onclick={ctx.link().callback(|_| RolloutMsg::Plan)}
                        >{label}</Button>
                        if self.chosen.is_empty() && !running {
                            {" "}{t::NO_SITE}
                        }
                    </p>
                    {self.error.as_ref().map(FrontendError::to_html)}
                    {below}
                </CardBody>
            </Card>
        }
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        self.alive.set(false);
    }
}

impl Rollout {
    /// Follows rollout job `id` from its first event on.
    fn follow(&mut self, ctx: &Context<Self>, id: i32) {
        self.stop_following();
        self.alive = Rc::new(Cell::new(true));
        self.job = Some(Following {
            id,
            state: JobState::Running,
            error: None,
            plan: None,
            devices: HashMap::new(),
            paused: None,
            done: None,
            lost: false,
            poll_error: None,
        });
        poll(ctx.link().clone(), id, self.alive.clone());
    }
    fn stop_following(&mut self) {
        self.alive.set(false);
        self.job = None;
    }

    /// The plan before the start: the pilots changeable, the start below.
    fn plan_view(&self, ctx: &Context<Self>, plan: &RolloutPlan) -> Html {
        let updated = plan.devices.iter().filter(|d| d.skip.is_none()).count();
        let pilot = |device: &PlannedDevice| {
            if device.skip.is_some() {
                return Html::default();
            }
            let id = device.id;
            html! {
                <label>
                    <input
                        type="checkbox"
                        checked={self.pilots.contains(&id)}
                        onchange={ctx.link().callback(move |_| RolloutMsg::TogglePilot(id))}
                    />
                    {" "}{t::PILOT}
                </label>
            }
        };
        html! {
            <>
                <p>
                    {t::target(plan.channel.name(), &plan.target)}{" · "}
                    {t::summary(updated, plan.devices.len() - updated)}
                </p>
                {table(plan, t::PILOT, pilot)}
                <p>
                    <Button
                        variant={ButtonVariant::Primary}
                        disabled={!self.allow_apply || updated == 0}
                        onclick={ctx.link().callback(|_| RolloutMsg::Start)}
                    >{t::START}</Button>
                    if !self.allow_apply {
                        {" "}{t::NOT_ALLOWED}
                    }
                </p>
            </>
        }
    }

    /// The rollout job: its plan with the state of each device, the pause and the end.
    fn job_view(&self, ctx: &Context<Self>, job: &Following) -> Html {
        let running = job.state == JobState::Running && !job.lost;
        let state = |device: &PlannedDevice| device_state(ctx, job, device, running);
        let summary = match (&job.done, &job.state) {
            (Some(done), _) => html! {
                <Alert inline=true r#type={AlertType::Success} title={t::done(done.updated, done.skipped, done.failed)}/>
            },
            (None, JobState::Cancelled) => html! {
                <Alert inline=true r#type={AlertType::Warning} title={t::CANCELLED}/>
            },
            _ => Html::default(),
        };
        let paused = job.paused.as_ref().filter(|_| running).map(|paused| {
            let text = match (paused.reason, &paused.device) {
                (RolloutPause::Failed, Some(device)) => t::paused_failed(device),
                (RolloutPause::Failed, None) => t::paused_failed("?"),
                (RolloutPause::AfterPilots, _) => t::PAUSED_AFTER_PILOTS.into(),
            };
            let r#type = match paused.reason {
                RolloutPause::AfterPilots => AlertType::Info,
                RolloutPause::Failed => AlertType::Danger,
            };
            html! {
                <Alert inline=true {r#type} title={text}>
                    <Button variant={ButtonVariant::Primary} onclick={ctx.link().callback(|_| RolloutMsg::Continue)}>
                        {t::CONTINUE}
                    </Button>
                    {" "}
                    <Button variant={ButtonVariant::Secondary} onclick={ctx.link().callback(|_| RolloutMsg::Cancel)}>
                        {t::END}
                    </Button>
                </Alert>
            }
        });
        let cancel = (running && job.paused.is_none()).then(|| {
            html! {
                <p>
                    <Button variant={ButtonVariant::Secondary} onclick={ctx.link().callback(|_| RolloutMsg::Cancel)}>
                        {t::CANCEL}
                    </Button>
                </p>
            }
        });
        html! {
            <>
                {job.poll_error.as_ref().map(FrontendError::to_html)}
                if job.lost {
                    <Alert inline=true r#type={AlertType::Warning} title={t::LOST}/>
                }
                {job.error.as_ref().map(|error| html!(<p>{ServerProblem::from(error).text()}</p>))}
                {summary}
                {paused}
                if let Some(plan) = &job.plan {
                    <p>{t::target(plan.channel.name(), &plan.target)}</p>
                    {table(plan, t::STATE, state)}
                } else if running {
                    <p><Spinner size={SpinnerSize::Sm}/>{" "}{t::PLANNING_AGAIN}</p>
                }
                {cancel}
            </>
        }
    }
}

impl Following {
    fn take(&mut self, job: RolloutJob) {
        self.state = job.state;
        self.error = job.error;
        self.poll_error = None;
        for logged in job.events {
            match logged.event {
                RolloutEvent::RolloutPlanned(planned) => self.plan = Some(planned.plan),
                RolloutEvent::RolloutDevice(device) => {
                    self.devices.insert(device.device, device);
                }
                RolloutEvent::RolloutPaused(paused) => self.paused = Some(paused),
                RolloutEvent::RolloutContinued(_) => self.paused = None,
                RolloutEvent::RolloutDone(done) => self.done = Some(done),
                RolloutEvent::Other => {}
            }
        }
    }
}

/// The plan's devices in their order; `last` names the last column, `cell` fills it.
fn table(plan: &RolloutPlan, last: &str, cell: impl Fn(&PlannedDevice) -> Html) -> Html {
    html! {
        <table class="pf-v6-c-table pf-m-compact rollout-plan">
            <thead>
                <tr>
                    <th>{t::POSITION}</th>
                    <th>{t::DEVICE}</th>
                    <th>{t::SITE}</th>
                    <th>{t::VERSION}</th>
                    <th>{t::ARCHITECTURE}</th>
                    <th>{t::PACKAGES}</th>
                    <th>{t::FALLBACK}</th>
                    <th>{last}</th>
                </tr>
            </thead>
            <tbody>
                { for plan.devices.iter().enumerate().map(|(index, device)| row(index, device, &plan.target, cell(device))) }
            </tbody>
        </table>
    }
}

fn row(index: usize, device: &PlannedDevice, target: &str, last: Html) -> Html {
    let position = match &device.skip {
        None => t::position(index + 1, device.pilot, device.depth),
        Some(skip) => skip_text(skip).into(),
    };
    let version = match (&device.version, &device.skip) {
        (Some(version), None) => t::version_change(version, target),
        (Some(version), Some(_)) => version.clone(),
        (None, _) => String::new(),
    };
    let fallback = match device.fallback {
        RolloutFallback::Partition => t::FALLBACK_PARTITION,
        RolloutFallback::Neighbor => t::FALLBACK_NEIGHBOR,
        RolloutFallback::None => t::FALLBACK_NONE,
        RolloutFallback::Other(_) => "",
    };
    let packages = if device.files.is_empty() {
        String::new()
    } else {
        format!(
            "{} ({})",
            device.files.join(", "),
            t::space(
                device.download.map(|d| d.0),
                device.free_space.map(|f| f.0),
                device.in_ram,
            )
        )
    };
    let class = match (&device.skip, device.pilot) {
        (Some(_), _) => "rollout-left-out",
        (None, true) => "rollout-pilot",
        (None, false) => "",
    };
    html! {
        <tr {class}>
            <td>
                {position}
                if let Some(error) = &device.error {
                    <div class="rollout-error">{ServerProblem::from(error).text()}</div>
                }
            </td>
            <td>
                <Link<AppRoute> to={AppRoute::device(device.site_id as u32, device.id as u32)}>{device.name.clone()}</Link<AppRoute>>
            </td>
            <td>{device.site.clone()}</td>
            <td>{version}</td>
            <td>{device.architecture.clone().unwrap_or_default()}</td>
            <td>{packages}</td>
            <td>{fallback}</td>
            <td>{last}</td>
        </tr>
    }
}

/// Where `device` stands in the rollout `job`; one not started yet can be skipped while it runs.
fn device_state(
    ctx: &Context<Rollout>,
    job: &Following,
    device: &PlannedDevice,
    running: bool,
) -> Html {
    if device.skip.is_some() {
        return Html::default();
    }
    let told = job.devices.get(&device.id);
    let text = match told.map(|told| told.state) {
        None => t::STATE_PENDING,
        Some(State::Waiting) => t::STATE_WAITING,
        Some(State::Started) => t::STATE_STARTED,
        Some(State::Done) => t::STATE_DONE,
        Some(State::Failed) => t::STATE_FAILED,
        Some(State::Cancelled) => t::STATE_CANCELLED,
        Some(State::Skipped) => t::STATE_SKIPPED,
    };
    let startable = matches!(told.map(|told| told.state), None | Some(State::Waiting));
    let id = device.id;
    html! {
        <>
            if told.is_some_and(|told| told.state == State::Started) && running {
                <Spinner size={SpinnerSize::Sm}/>{" "}
            }
            {text}
            if let Some(error) = told.and_then(|told| told.error.as_ref()) {
                <div class="rollout-error">{ServerProblem::from(error).text()}</div>
            }
            if startable && running {
                {" "}
                <Button variant={ButtonVariant::Link} onclick={ctx.link().callback(move |_| RolloutMsg::Skip(id))}>
                    {t::SKIP}
                </Button>
            }
        </>
    }
}

fn skip_text(skip: &RolloutSkip) -> &'static str {
    match skip {
        RolloutSkip::UpToDate => t::SKIP_UP_TO_DATE,
        RolloutSkip::Unreachable => t::SKIP_UNREACHABLE,
        RolloutSkip::Leftovers => t::SKIP_LEFTOVERS,
        RolloutSkip::NoFallback => t::SKIP_NO_FALLBACK,
        RolloutSkip::NoSpace => t::SKIP_NO_SPACE,
        RolloutSkip::Other(_) => "",
    }
}

fn pilots_of(plan: &RolloutPlan) -> BTreeSet<i32> {
    plan.devices
        .iter()
        .filter(|device| device.pilot)
        .map(|device| device.id)
        .collect()
}

/// Sends a mutation steering the rollout.
fn act(ctx: &Context<Rollout>, operation: impl Sendable + 'static) {
    let scope = ctx.link().clone();
    spawn_local(async move {
        let result = request(scope.clone(), operation).await.map(|_| ());
        scope.send_message(RolloutMsg::Acted(result));
    });
}

/// Asks for the job's new events until it is over, or the server doesn't know it anymore;
/// while asking fails, it asks again, less often.
fn poll(scope: Scope<Rollout>, id: i32, alive: Rc<Cell<bool>>) {
    spawn_local(async move {
        let mut after = 0;
        let mut failed = 0;
        while alive.get() {
            let variables = RolloutEventsVariables {
                id,
                after: Some(after),
            };
            match request(scope.clone(), RolloutEvents::build(variables)).await {
                Ok(RolloutEvents { job: Some(job) }) => {
                    failed = 0;
                    if let Some(last) = job.events.last() {
                        after = last.index + 1;
                    }
                    let over = job.state != JobState::Running;
                    if alive.get() {
                        scope.send_message(RolloutMsg::Loaded(job));
                    }
                    if over {
                        return;
                    }
                }
                Ok(RolloutEvents { job: None }) => {
                    if alive.get() {
                        scope.send_message(RolloutMsg::Lost);
                    }
                    return;
                }
                Err(error) => {
                    if alive.get() {
                        scope.send_message(RolloutMsg::PollFailed(error));
                    }
                    sleep(RETRY[failed.min(RETRY.len() - 1)]).await;
                    failed += 1;
                    continue;
                }
            }
            sleep(POLL).await;
        }
    });
}
