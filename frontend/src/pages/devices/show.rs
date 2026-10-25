use crate::{
    components::{
        adjust_target::{AdjustTarget, SelectedTarget},
        batch_apply::{BatchApply, BatchJob},
        interfaces::Interfaces,
        job::JobView,
        menu::popup::Select,
    },
    error::{FrontendError, ServerProblem},
    i18n::{
        common::{CANCEL, DeviceModeNeed, device_mode_feature},
        device::{self as t, setup},
        target,
    },
    graphql::{
        authenticated::{
            ApplyDeviceCfg, BootstrapCandidates, BootstrapDevice, ChangeDeviceMode, DeviceJobs,
            BootPartition, DeviceOverview, DowngradeRouteros, downgrade_routeros, PowerCycle, RebootDevice, power_cycle, RepartitionDevice, UpdateRouteros, apply_device_cfg,
            reboot_device, bootstrap_candidates, bootstrap_device, change_device_mode, device_jobs,
            boot_partition, device_overview, repartition_device, update_routeros,
        },
        query_authenticated, query_authenticated_response,
    },
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode, Spinner, SpinnerSize, TextInput,
};
use yew::{Component, Context, Html, Properties, html, html::Scope, platform::spawn_local};
use yew_nested_router::components::Link;
use crate::pages::routes::AppRoute;

pub struct ShowDevice {
    id: u32,
    error: Option<FrontendError>,
    data: Option<ShowDeviceData>,
    alternate_target: SelectedTarget,
    /// the job shown (an update, an apply, a setup), started here or running when the page was opened
    job: Option<i64>,
    job_running: bool,
    /// the jobs of a batch started here (this device first), shown side by side instead of `job`
    batch_jobs: Vec<BatchJob>,
    /// how many of them are over
    batch_finished: usize,
    /// what the bootstrap neighbor sees on its port; `None` while it is asked
    candidates: Option<Result<Vec<Candidate>, String>>,
    /// a MAC typed in instead of picking a candidate
    manual_mac: String,
    /// the RouterOS channel to update in; `None` until the device's default is known
    channel: Option<device_overview::Channel>,
    /// the older version to downgrade to, typed or the one before an update of this run
    downgrade_version: String,
    /// "Auf … downgraden" was clicked, waiting for the confirmation
    confirm_downgrade: bool,
    /// "Neu starten" was clicked, waiting for the confirmation
    confirm_reboot: bool,
    /// "Strom aus und ein" was clicked, waiting for the confirmation
    confirm_power_cycle: bool,
    /// the supplier's job switching the power off and on, next to this device's own
    power_job: Option<i64>,
    /// "Device-Mode anpassen" was clicked, waiting for the confirmation
    confirm_device_mode: bool,
    /// "In zwei Partitionen aufteilen" was clicked, waiting for the confirmation
    confirm_repartition: bool,
    /// the partition to boot from, asked before
    confirm_boot: Option<String>,
    /// the form for another address or other credentials is open (in the head; on an
    /// unreachable device it is always shown, in the warning)
    show_target: bool,
    /// suspend the watchdogs that may fire while this one updates
    suspend_watchdogs: bool,
    /// update although nothing brings the device back (no two partitions, no neighbor)
    without_fallback: bool,
    /// counts the jobs over (NetBox written, the device changed): the interfaces are read again
    jobs_finished: u32,
    /// the job running is an apply (alone or in a batch)
    applying: bool,
    /// counts the applies over: the interfaces written to are no longer changed
    applied: u32,
}
type Watchdog = device_overview::DeviceOverviewTopologyDeviceByIdWatchdogs;
type Connected = device_overview::DeviceOverviewTopologyDeviceByIdAccessConnected;
type Candidate =
    bootstrap_candidates::BootstrapCandidatesTopologyDeviceByIdBootstrapPlanDiscovered;
/// the planned changes and what applying them would do that NetBox can't show (PoE switched off)
#[derive(Debug)]
struct GeneratedCfg {
    script: Box<str>,
    warnings: Vec<Warning>,
}
type Warning = device_overview::DeviceOverviewTopologyDeviceByIdAccessGenerateCfgWarnings;
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
    routeros: Option<device_overview::DeviceOverviewTopologyDeviceByIdAccessRouteros>,
    /// the newest entries of the device's log, oldest first
    log: Vec<device_overview::DeviceOverviewTopologyDeviceByIdAccessLog>,
    /// `None` before RouterOS 7.17, or when it couldn't be read
    device_mode: Option<device_overview::DeviceOverviewTopologyDeviceByIdAccessDeviceMode>,
    /// `None` without partitions (16 MiB of flash), or when they couldn't be read
    partitions: Option<device_overview::DeviceOverviewTopologyDeviceByIdAccessPartitions>,
    bootstrap_plan: Option<device_overview::DeviceOverviewTopologyDeviceByIdBootstrapPlan>,
    bootstrap_problem: Option<device_overview::Problem>,
    /// the watchdogs that may fire while this one updates, of the devices powering it and of
    /// those reaching the network through it
    watchdogs: Vec<Watchdog>,
    /// the neighbors it stays reachable through after an update that cut it off from OSPF
    update_neighbors: Vec<String>,
    /// the device feeding it over PoE and its port: a cold boot is possible
    power_supply: Option<device_overview::DeviceOverviewTopologyDeviceByIdPowerSupply>,
    /// writing to devices is allowed (`allow_apply`); else the buttons that write are disabled
    allow_apply: bool,
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
    Candidates(Result<Vec<Candidate>, String>),
    ManualMac(String),
    /// set up the device with this MAC
    Bootstrap(String),
    /// the label of a channel entry (`channel_label`)
    SelectChannel(Box<str>),
    Update,
    /// bring the packages in line at the running version
    AdjustPackages,
    DowngradeVersion(String),
    /// ask before downgrading (`true`), or not anymore
    ConfirmDowngrade(bool),
    Downgrade,
    /// ask before rebooting (`true`), or not anymore
    ConfirmReboot(bool),
    Reboot,
    ConfirmPowerCycle(bool),
    PowerCycle,
    PowerCycleStarted(Result<i64, FrontendError>),
    /// ask before changing the device mode (`true`), or not anymore
    ConfirmDeviceMode(bool),
    ChangeDeviceMode,
    /// ask before splitting the flash (`true`), or not anymore
    ConfirmRepartition(bool),
    Repartition,
    /// ask before booting from a partition (`Some`), or not anymore
    ConfirmBoot(Option<String>),
    BootPartition(String),
    /// open or close the form for another address or other credentials
    ShowTarget(bool),
    SuspendWatchdogs(bool),
    WithoutFallback(bool),
    JobStarted(Result<i64, FrontendError>),
    JobFinished,
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
            job_running: false,
            batch_jobs: Vec::new(),
            batch_finished: 0,
            candidates: None,
            manual_mac: String::new(),
            channel: None,
            downgrade_version: String::new(),
            confirm_downgrade: false,
            confirm_reboot: false,
            confirm_power_cycle: false,
            power_job: None,
            confirm_device_mode: false,
            confirm_repartition: false,
            confirm_boot: None,
            show_target: false,
            suspend_watchdogs: true,
            without_fallback: false,
            jobs_finished: 0,
            applying: false,
            applied: 0,
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
                if self.downgrade_version.is_empty()
                    && let Some(previous) = data.routeros.as_ref().and_then(|r| r.previous.clone())
                {
                    self.downgrade_version = previous;
                }
                self.data = Some(*data);
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
                self.applying = true;
                apply(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::Update => {
                let Some(channel) = self.channel.clone() else {
                    return false;
                };
                self.job_running = true;
                let suspend = self.suspend();
                let without = self.without_fallback;
                update(ctx.link().clone(), self.id, Some(channel), suspend, without, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::AdjustPackages => {
                self.job_running = true;
                let suspend = self.suspend();
                let without = self.without_fallback;
                update(ctx.link().clone(), self.id, None, suspend, without, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::SuspendWatchdogs(suspend) => {
                self.suspend_watchdogs = suspend;
                true
            }
            ShowDeviceMessage::WithoutFallback(without) => {
                self.without_fallback = without;
                true
            }
            ShowDeviceMessage::DowngradeVersion(version) => {
                self.downgrade_version = version;
                true
            }
            ShowDeviceMessage::ConfirmDowngrade(confirm) => {
                self.confirm_downgrade = confirm;
                true
            }
            ShowDeviceMessage::Downgrade => {
                self.confirm_downgrade = false;
                self.job_running = true;
                let version = self.downgrade_version.trim().to_string();
                let suspend = self.suspend();
                let without = self.without_fallback;
                downgrade(ctx.link().clone(), self.id, version, suspend, without, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::ConfirmReboot(confirm) => {
                self.confirm_reboot = confirm;
                true
            }
            ShowDeviceMessage::Reboot => {
                self.confirm_reboot = false;
                self.job_running = true;
                reboot(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::ConfirmPowerCycle(confirm) => {
                self.confirm_power_cycle = confirm;
                true
            }
            ShowDeviceMessage::PowerCycle => {
                self.confirm_power_cycle = false;
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
            ShowDeviceMessage::ConfirmDeviceMode(confirm) => {
                self.confirm_device_mode = confirm;
                true
            }
            ShowDeviceMessage::ChangeDeviceMode => {
                self.confirm_device_mode = false;
                self.job_running = true;
                change_mode(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::ConfirmRepartition(confirm) => {
                self.confirm_repartition = confirm;
                true
            }
            ShowDeviceMessage::Repartition => {
                self.confirm_repartition = false;
                self.job_running = true;
                repartition(ctx.link().clone(), self.id, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::ConfirmBoot(partition) => {
                self.confirm_boot = partition;
                true
            }
            ShowDeviceMessage::BootPartition(partition) => {
                self.confirm_boot = None;
                self.job_running = true;
                boot_from(ctx.link().clone(), self.id, partition, self.alternate_target.clone());
                true
            }
            ShowDeviceMessage::ShowTarget(show) => {
                self.show_target = show;
                true
            }
            ShowDeviceMessage::JobStarted(result) => {
                match result {
                    Ok(id) => {
                        self.job = Some(id);
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
                // the device changed: show what is left now
                fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
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
            ShowDeviceMessage::ManualMac(mac) => {
                self.manual_mac = mac;
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
        let job = if self.batch_jobs.is_empty() {
            self.job.map(|id| html! {
                <JobView key={id} id={id} on_finished={ctx.link().callback(|_| ShowDeviceMessage::JobFinished)}/>
            })
        } else {
            Some(html! {
                <div class="batch-jobs">
                    { for self.batch_jobs.iter().map(|job| html! {
                        <div class="batch-job" key={job.job}>
                            <h3>
                                if let Some(site) = job.site {
                                    <Link<AppRoute> to={AppRoute::device(site as u32, job.device as u32)}>{&job.name}</Link<AppRoute>>
                                } else {
                                    {&job.name}
                                }
                            </h3>
                            <JobView id={job.job} on_finished={ctx.link().callback(|_| ShowDeviceMessage::BatchJobFinished)}/>
                        </div>
                    })}
                </div>
            })
        };
        // a job started before the overview is there (it reloads after each job)
        let job_fallback = job.clone();
        let target_form = html! {
            <AdjustTarget onchange={ctx.link().callback(ShowDeviceMessage::AdjustTarget)} value={self.alternate_target.clone()} />
        };
        let data = self.data.as_ref().map(|data| {
            // a job runs, or writing is locked
            let running = self.job_running || !data.allow_apply;
            let locked = (!data.allow_apply).then(|| html! {
                <Alert inline=true title={t::LOCKED} r#type={AlertType::Info}>
                    <p>{t::LOCKED_BEFORE}<code>{"allow_apply: true"}</code>{t::LOCKED_AFTER}</p>
                </Alert>
            });
            let (board, cfg_update) = data
                .access
                .as_ref()
                .map(|(board, cfg)| (board.as_ref(), cfg.as_ref()))
                .unwrap_or_default();
            let generated = cfg_update.is_some();
            let warnings = cfg_update.map(|cfg| cfg.warnings.as_slice()).unwrap_or_default();
            let cfg_update = cfg_update.map(|cfg| cfg.script.as_ref()).unwrap_or_default();
            let nothing_to_do = cfg_update.trim().is_empty();
            // nothing brings the device back from an update: only once confirmed
            let no_way_back = self.no_way_back(data);
            let update_locked = no_way_back && !self.without_fallback;
            let routeros = data.routeros.as_ref().map(|routeros| {
                let selected = routeros
                    .channels
                    .iter()
                    .find(|c| Some(&c.channel) == self.channel.as_ref());
                let entries: Vec<Box<str>> = routeros.channels.iter().map(channel_label).collect();
                let action = selected.filter(|c| c.update).map(|c| {
                    if c.upgrade {
                        t::update_to(&c.newest)
                    } else {
                        t::ADJUST_PACKAGES.to_string()
                    }
                });
                // without changing the version, whatever the channel; unless that is what the
                // channel's update does anyway
                let adjust_packages = routeros.adjust_packages
                    && !selected.is_some_and(|c| c.update && !c.upgrade);
                html! {<>
                <dt>{t::ROUTEROS}</dt><dd>
                    { routeros.version.clone() }
                    {t::packages(&routeros.packages, &routeros.wanted)}
                    <div class="routeros-update">
                        <Select<Box<str>>
                            selected={selected.map(channel_label)}
                            entries={entries}
                            onselect={ctx.link().callback(ShowDeviceMessage::SelectChannel)}
                        />
                        if let Some(action) = action {
                            {" "}
                            <Button
                                variant={ButtonVariant::Secondary}
                                label={action}
                                disabled={update_locked || running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::Update)}
                            />
                        } else if !adjust_packages {
                            {t::UP_TO_DATE}
                        }
                        if adjust_packages {
                            {" "}
                            <Button
                                variant={ButtonVariant::Secondary}
                                label={t::ADJUST_PACKAGES}
                                disabled={update_locked || running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::AdjustPackages)}
                            />
                        }
                    </div>
                    if !routeros.version.starts_with("6.") {
                        { self.downgrade_view(ctx, update_locked || running) }
                    }
                    if no_way_back {
                        { self.no_way_back_view(ctx) }
                    }
                    { self.watchdogs_view(ctx, &data.watchdogs) }
                    <div class="device-reboot">
                        if self.confirm_reboot {
                            {t::REBOOT_CONFIRM}
                            <Button
                                variant={ButtonVariant::Danger}
                                label={t::REBOOT_NOW}
                                disabled={running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::Reboot)}
                            />
                            {" "}
                            <Button
                                variant={ButtonVariant::Link}
                                label={CANCEL}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmReboot(false))}
                            />
                        } else {
                            <Button
                                variant={ButtonVariant::Secondary}
                                label={t::REBOOT}
                                disabled={running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmReboot(true))}
                            />
                        }
                        if let Some(supply) = &data.power_supply {
                            {" "}
                            { self.power_cycle_view(ctx, supply, data.allow_apply) }
                        }
                    </div>
                </dd>
            </>}
            });
            let device_mode = data.device_mode.as_ref().map(|mode| {
                let missing: Vec<String> = mode
                    .changes
                    .iter()
                    .map(|change| {
                        use device_overview::DeviceModeReason as R;
                        device_mode_feature(
                            &change.feature,
                            match &change.reason {
                                R::APPLY_FALLBACK => DeviceModeNeed::ApplyFallback,
                                R::MANAGEMENT_PROXY => DeviceModeNeed::ManagementProxy,
                                R::UPDATE_FALLBACK => DeviceModeNeed::UpdateFallback,
                                R::Other(other) => DeviceModeNeed::Other(other),
                            },
                        )
                    })
                    .collect();
                html! {<>
                <dt>{t::DEVICE_MODE}</dt><dd>
                    { mode.mode.clone() }
                    if !missing.is_empty() {
                        { t::device_mode_missing(&missing) }
                        <div class="device-mode">
                            if self.confirm_device_mode {
                                {t::DEVICE_MODE_CONFIRM}
                                <Button
                                    variant={ButtonVariant::Danger}
                                    label={t::DEVICE_MODE_NOW}
                                    disabled={running}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ChangeDeviceMode)}
                                />
                                {" "}
                                <Button
                                    variant={ButtonVariant::Link}
                                    label={CANCEL}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmDeviceMode(false))}
                                />
                            } else {
                                <Button
                                    variant={ButtonVariant::Secondary}
                                    label={t::DEVICE_MODE_ADJUST}
                                    disabled={running}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmDeviceMode(true))}
                                />
                            }
                        </div>
                    }
                </dd>
            </>}
            });
            let partitions = data.partitions.as_ref().map(|partitions| {
                html! {<>
                <dt>{t::PARTITIONS}</dt><dd>
                    { for partitions.partitions.iter().map(|p| {
                        // a RouterOS to boot, e.g. the one from before an update
                        let bootable = !p.running && !p.version.is_empty();
                        let name = p.name.clone();
                        html!(
                        <div>
                            { t::partition(&p.name, &p.version, p.size_mib, p.active, p.running) }
                            if bootable && self.confirm_boot.as_ref() == Some(&p.name) {
                                {" "}{t::PARTITION_BOOT_CONFIRM}
                                <Button
                                    variant={ButtonVariant::Danger}
                                    label={t::PARTITION_BOOT_NOW}
                                    disabled={running}
                                    onclick={ctx.link().callback(move |_| ShowDeviceMessage::BootPartition(name.clone()))}
                                />
                                {" "}
                                <Button
                                    variant={ButtonVariant::Link}
                                    label={CANCEL}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmBoot(None))}
                                />
                            } else if bootable {
                                {" "}
                                <Button
                                    variant={ButtonVariant::Secondary}
                                    label={t::partition_boot(&p.name)}
                                    disabled={running}
                                    onclick={ctx.link().callback(move |_| ShowDeviceMessage::ConfirmBoot(Some(name.clone())))}
                                />
                            }
                        </div>
                    )})}
                    if partitions.can_split {
                        <div class="partitions-split">
                            if self.confirm_repartition {
                                {t::PARTITIONS_CONFIRM}
                                <Button
                                    variant={ButtonVariant::Danger}
                                    label={t::PARTITIONS_NOW}
                                    disabled={running}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::Repartition)}
                                />
                                {" "}
                                <Button
                                    variant={ButtonVariant::Link}
                                    label={CANCEL}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmRepartition(false))}
                                />
                            } else {
                                <Button
                                    variant={ButtonVariant::Secondary}
                                    label={t::PARTITIONS_SPLIT}
                                    disabled={running}
                                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmRepartition(true))}
                                />
                            }
                        </div>
                    }
                </dd>
            </>}
            });
            let bootstrap_running = running;
            let update = if data.access.is_none() {
                let setup = match (&data.bootstrap_plan, &data.bootstrap_problem) {
                    (Some(plan), _) => {
                        let name = data.configured_name.clone();
                        let candidates = match &self.candidates {
                            None => html!(<p><Spinner size={SpinnerSize::Md}/>{setup::searching(&plan.neighbor, &plan.neighbor_port)}</p>),
                            Some(Err(error)) => html!(<p>{setup::search_failed(&plan.neighbor, error)}</p>),
                            Some(Ok(found)) if found.is_empty() => html!(<p>{setup::nothing_found(&plan.neighbor, &plan.neighbor_port)}</p>),
                            Some(Ok(found)) => html! {
                                <table class="bootstrap-candidates">
                                    <thead><tr>
                                        <th>{setup::IDENTITY}</th><th>{setup::BOARD}</th><th>{setup::ROUTEROS}</th><th>{setup::MAC}</th><th>{setup::PORT}</th><th/>
                                    </tr></thead>
                                    <tbody>
                                    { for found.iter().map(|c| {
                                        let mac = c.mac.clone();
                                        html! {<tr>
                                            <td>{&c.identity}</td><td>{&c.board}</td><td>{&c.version}</td>
                                            <td><code>{&c.mac}</code></td><td>{&c.interface}</td>
                                            <td><Button
                                                variant={ButtonVariant::Secondary}
                                                label={setup::set_up_as(&name)}
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
                            <p>{setup::plan(&plan.neighbor, &plan.neighbor_port, &plan.port, &plan.address)}</p>
                            <p>{setup::RESCUE}</p>
                            {candidates}
                            <div class="bootstrap-manual">
                                <TextInput
                                    value={self.manual_mac.clone()}
                                    placeholder={setup::MAC_PLACEHOLDER}
                                    onchange={ctx.link().callback(ShowDeviceMessage::ManualMac)}
                                />
                                <Button
                                    variant={ButtonVariant::Secondary}
                                    label={setup::WITH_MAC}
                                    disabled={bootstrap_running || manual.trim().is_empty()}
                                    onclick={ctx.link().callback(move |_| ShowDeviceMessage::Bootstrap(manual.clone()))}
                                />
                            </div>
                        </>}
                    }
                    (None, Some(problem)) => html!(<p>{setup::impossible(&ServerProblem::from(problem).text())}</p>),
                    (None, None) => html!(),
                };
                html!(<Alert inline=true title={t::UNREACHABLE} r#type={AlertType::Warning}>
                    if let Some(problem) = &data.access_problem {
                        <p>{problem.as_ref()}</p>
                    }
                    {setup}
                    <p>{t::UNREACHABLE_HINT}</p>
                    {target_form.clone()}
                </Alert>)
            } else if !generated {
                html!(<Alert inline=true title={t::NOT_GENERATED} r#type={AlertType::Warning}>
                    <p>{t::NOT_GENERATED_HINT}</p>
                </Alert>)
            } else if nothing_to_do {
                html!(<Alert inline=true title={t::IN_LINE} r#type={AlertType::Success}/>)
            } else {
                html! {<>
                    { for warnings.iter().map(|warning| html!(
                        <Alert inline=true title={describe_warning(warning)} r#type={AlertType::Warning}/>
                    ))}
                    <CodeBlock><CodeBlockCode>{ cfg_update }</CodeBlockCode></CodeBlock>
                </>}
            };
            let reachable = data.access.is_some();
            let head = html! {
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
                            {target_form.clone()}
                        }
                    </CardBody>
                </Card>
            };
            let device = (routeros.is_some() || device_mode.is_some() || partitions.is_some()).then(|| html! {
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
            });
            html! {<>
            {locked}
            {head}
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
                <Card class="device-cfg">
                    <CardHeader><CardTitle>{t::CHANGES}</CardTitle></CardHeader>
                    <CardBody>
                        {update}
                        if reachable {
                            <Button
                                variant={ButtonVariant::Primary}
                                label={t::APPLY}
                                disabled={nothing_to_do || running}
                                onclick={ctx.link().callback(|_| ShowDeviceMessage::Apply)}
                            />
                            <BatchApply
                                id={self.id}
                                disabled={running}
                                on_started={ctx.link().callback(ShowDeviceMessage::BatchStarted)}
                            />
                        }
                    </CardBody>
                </Card>
            </div>
            if !data.log.is_empty() {
                <Card class="device-log">
                    <CardHeader><CardTitle>{t::LOG}</CardTitle></CardHeader>
                    <CardBody>
                        <details>
                            <summary>{t::log_entries(data.log.len())}</summary>
                            <CodeBlock>
                                <CodeBlockCode>
                                    { for data.log.iter().map(|entry| format!("{} {} {}\n", entry.time, entry.topics, entry.message)) }
                                </CodeBlockCode>
                            </CodeBlock>
                        </details>
                    </CardBody>
                </Card>
            }
            </>}
        });
        html! {
            <div class="device-page">
            {error}
            if let Some(data) = data {
                {data}
            } else {
                {job_fallback}
                <Spinner size={SpinnerSize::Md}/>
            }
            </div>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            fetch_overview(ctx.link().clone(), self.id, self.alternate_target.clone());
            running_job(ctx.link().clone(), self.id);
        }
    }
}

impl ShowDevice {
    /// A version to type (or the one before an update of this run) and „Auf … downgraden“,
    /// asked to confirm.
    fn downgrade_view(&self, ctx: &Context<Self>, disabled: bool) -> Html {
        let version = self.downgrade_version.trim();
        if self.confirm_downgrade {
            return html! {
                <div class="routeros-downgrade">
                    {t::downgrade_confirm(version)}
                    <Button
                        variant={ButtonVariant::Danger}
                        label={t::DOWNGRADE_NOW}
                        disabled={disabled}
                        onclick={ctx.link().callback(|_| ShowDeviceMessage::Downgrade)}
                    />
                    {" "}
                    <Button
                        variant={ButtonVariant::Link}
                        label={CANCEL}
                        onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmDowngrade(false))}
                    />
                </div>
            };
        }
        let label = match version.is_empty() {
            true => t::DOWNGRADE.to_string(),
            false => t::downgrade_to(version),
        };
        html! {
            <div class="routeros-downgrade">
                <TextInput
                    value={self.downgrade_version.clone()}
                    placeholder={t::DOWNGRADE_PLACEHOLDER}
                    onchange={ctx.link().callback(ShowDeviceMessage::DowngradeVersion)}
                />
                {" "}
                <Button
                    variant={ButtonVariant::Secondary}
                    label={label}
                    disabled={disabled || version.is_empty()}
                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmDowngrade(true))}
                />
            </div>
        }
    }
    /// „Strom aus und ein“ over the device feeding it, asked before; also while the device's
    /// own job runs (a device mode change waits for it), and the supplier's job below it.
    fn power_cycle_view(
        &self,
        ctx: &Context<Self>,
        supply: &device_overview::DeviceOverviewTopologyDeviceByIdPowerSupply,
        allow_apply: bool,
    ) -> Html {
        let job = self.power_job.map(|id| html! {
            <JobView key={id} id={id} on_finished={yew::Callback::noop()}/>
        });
        html! {<>
            if self.confirm_power_cycle {
                {t::power_cycle_confirm(&supply.device.name, &supply.port)}
                <Button
                    variant={ButtonVariant::Danger}
                    label={t::POWER_CYCLE_NOW}
                    disabled={!allow_apply}
                    onclick={ctx.link().callback(|_| ShowDeviceMessage::PowerCycle)}
                />
                {" "}
                <Button
                    variant={ButtonVariant::Link}
                    label={CANCEL}
                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmPowerCycle(false))}
                />
            } else {
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::POWER_CYCLE}
                    disabled={!allow_apply}
                    onclick={ctx.link().callback(|_| ShowDeviceMessage::ConfirmPowerCycle(true))}
                />
            }
            {job}
        </>}
    }
    /// Whether an update would leave the device without a way back: no second partition with
    /// the running RouterOS to switch back to, and no neighbor reaching it (also when the
    /// partitions couldn't be read, as the backend then finds none either).
    fn no_way_back(&self, data: &ShowDeviceData) -> bool {
        let partitions = data.partitions.as_ref().map(|p| p.partitions.as_slice()).unwrap_or_default();
        let two = matches!(partitions, [a, b] if (a.running && a.active) || (b.running && b.active));
        !two && data.update_neighbors.is_empty()
    }
    fn no_way_back_view(&self, ctx: &Context<Self>) -> Html {
        let without = self.without_fallback;
        html! {
            <Alert inline=true title={t::NO_WAY_BACK_TITLE} r#type={AlertType::Danger}>
                <p>{t::NO_WAY_BACK}</p>
                <label class="suspend-watchdogs">
                    <input
                        type="checkbox"
                        checked={without}
                        onchange={ctx.link().callback(move |_| ShowDeviceMessage::WithoutFallback(!without))}
                    />
                    {" "}{t::UPDATE_WITHOUT_WAY_BACK}
                </label>
            </Alert>
        }
    }
    /// Whether an update suspends the watchdogs: asked for, and one watches something.
    fn suspend(&self) -> bool {
        self.suspend_watchdogs
            && self.data.as_ref().is_some_and(|data| {
                data.watchdogs.iter().any(|w| {
                    w.watching.watch_address.is_some() || !w.watching.poe_pings.is_empty()
                })
            })
    }
    /// The watchdogs that may fire while this device updates, where one watches something or
    /// can't be read, with the choice to suspend them during an update.
    fn watchdogs_view(&self, ctx: &Context<Self>, watchdogs: &[Watchdog]) -> Html {
        let watching: Vec<Html> = watchdogs
            .iter()
            .flat_map(|w| {
                let address = w.watching.watch_address.as_deref().map(|address| {
                    html!(<p>{t::watchdog_watching(&w.device, w.supplier, address)}</p>)
                });
                let ports = w.watching.poe_pings.iter().map(|ping| {
                    html!(<p>{t::watchdog_poe_ping(&w.device, &ping.port, &ping.address)}</p>)
                });
                address.into_iter().chain(ports).collect::<Vec<_>>()
            })
            .collect();
        let unread: Vec<Html> = watchdogs
            .iter()
            .filter_map(|w| {
                let problem = w.problem.as_ref()?;
                Some(html!(<p>{t::watchdog_unread(&w.device, &ServerProblem::from(problem).text())}</p>))
            })
            .collect();
        if watching.is_empty() && unread.is_empty() {
            return Html::default();
        }
        let suspend = self.suspend_watchdogs;
        html! {
            <Alert inline=true title={t::WATCHDOG_TITLE} r#type={AlertType::Warning}>
                { for watching.iter().cloned() }
                { for unread.iter().cloned() }
                if !watching.is_empty() {
                    <label class="suspend-watchdogs">
                        <input
                            type="checkbox"
                            checked={suspend}
                            onchange={ctx.link().callback(move |_| ShowDeviceMessage::SuspendWatchdogs(!suspend))}
                        />
                        {" "}{t::SUSPEND_WATCHDOGS}
                    </label>
                }
            </Alert>
        }
    }
}

fn fetch_overview(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| device_overview::AdhocCredentials {
                username,
                password,
            });
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
                    // why the device couldn't be reached goes with the warning, not the banner
                    let (access_errors, errors): (Vec<_>, Vec<_>) = response
                        .errors
                        .unwrap_or_default()
                        .into_iter()
                        .partition(|e| {
                            e.path.as_ref().is_some_and(|path| {
                                path.iter().map(|p| p.to_string()).eq(["topology", "deviceById", "access"])
                            })
                        });
                    let access_problem = access_errors
                        .first()
                        .map(|e| ServerProblem::of(e).text().into_boxed_str());
                    let error = Some(errors)
                        .filter(|e| !e.is_empty())
                        .map(FrontendError::Graphql);
                    let device = data.topology.device_by_id;
                    let data = device
                        .map(|mut d| ShowDeviceData {
                            configured_name: d.name.into_boxed_str(),
                            connected: d.access.as_ref().map(|a| a.connected.clone()),
                            access_problem,
                            routeros: d.access.as_mut().and_then(|a| a.routeros.take()),
                            log: d.access.as_mut().and_then(|a| a.log.take()).unwrap_or_default(),
                            device_mode: d.access.as_mut().and_then(|a| a.device_mode.take()),
                            partitions: d.access.as_mut().and_then(|a| a.partitions.take()),
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
                            watchdogs: d.watchdogs,
                            update_neighbors: d.update_neighbors,
                            power_supply: d.power_supply,
                            allow_apply: data.allow_apply,
                        })
                        .unwrap_or_default();

                    scope.send_message(ShowDeviceMessage::Data {
                        data: Box::new(data),
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
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| apply_device_cfg::AdhocCredentials {
                username,
                password,
            });
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
        let result = query_authenticated::<BootstrapDevice, _>(
            scope.clone(),
            bootstrap_device::Variables { id: id as i64, mac },
        )
        .await
        .map(|data| data.bootstrap.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
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
    t::channel(name, &channel.newest).into()
}

/// Updates to the newest release of `channel`, or without one only adjusts the packages;
/// `suspend`: with the suppliers' watchdogs suspended.
fn update(
    scope: Scope<ShowDevice>,
    id: u32,
    channel: Option<device_overview::Channel>,
    suspend: bool,
    without_fallback: bool,
    target: SelectedTarget,
) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| update_routeros::AdhocCredentials {
                username,
                password,
            });
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
                suspend_watchdogs: suspend.then_some(true),
                without_fallback: without_fallback.then_some(true),
            },
        )
        .await
        .map(|data| data.update_routeros.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Takes RouterOS back to the older `version`; `suspend`, `without_fallback` as for [`update`].
fn downgrade(
    scope: Scope<ShowDevice>,
    id: u32,
    version: String,
    suspend: bool,
    without_fallback: bool,
    target: SelectedTarget,
) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| downgrade_routeros::AdhocCredentials {
                username,
                password,
            });
        let result = query_authenticated::<DowngradeRouteros, _>(
            scope.clone(),
            downgrade_routeros::Variables {
                id: id as i64,
                version,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
                suspend_watchdogs: suspend.then_some(true),
                without_fallback: without_fallback.then_some(true),
            },
        )
        .await
        .map(|data| data.downgrade_routeros.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Reboots the device and waits until it is back.
fn reboot(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| reboot_device::AdhocCredentials {
                username,
                password,
            });
        let result = query_authenticated::<RebootDevice, _>(
            scope.clone(),
            reboot_device::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| data.reboot_device.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Switches the PoE of the device feeding it off and on: a job of that device.
fn cut_power(scope: Scope<ShowDevice>, id: u32) {
    spawn_local(async move {
        let result = query_authenticated::<PowerCycle, _>(scope.clone(), power_cycle::Variables { id: id as i64 })
            .await
            .map(|data| data.power_cycle.id);
        scope.send_message(ShowDeviceMessage::PowerCycleStarted(result));
    });
}

/// Turns on the features of the device mode the provisioner needs; the device then waits for
/// the confirmation at the device.
fn change_mode(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| change_device_mode::AdhocCredentials {
                username,
                password,
            });
        let result = query_authenticated::<ChangeDeviceMode, _>(
            scope.clone(),
            change_device_mode::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| data.change_device_mode.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Splits the flash in two partitions; the device reformats it and reboots.
fn repartition(scope: Scope<ShowDevice>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| repartition_device::AdhocCredentials {
                username,
                password,
            });
        let result = query_authenticated::<RepartitionDevice, _>(
            scope.clone(),
            repartition_device::Variables {
                id: id as i64,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| data.repartition_device.id);
        scope.send_message(ShowDeviceMessage::JobStarted(result));
    });
}

/// Boots the device from `partition`.
fn boot_from(scope: Scope<ShowDevice>, id: u32, partition: String, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| boot_partition::AdhocCredentials {
                username,
                password,
            });
        let result = query_authenticated::<BootPartition, _>(
            scope.clone(),
            boot_partition::Variables {
                id: id as i64,
                partition,
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| data.boot_partition.id);
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

/// A warning of `generateCfg`, as the operator reads it before applying.
fn describe_warning(warning: &Warning) -> String {
    match warning {
        Warning::PoeCut(cut) => t::warning::poe_cut(&cut.port, &cut.default_name),
        Warning::RadioBound(bound) => {
            t::warning::radio_bound(&bound.radio, bound.configuration.as_deref(), &bound.expected)
        }
        Warning::LegacyRadioManaged(managed) => {
            t::warning::legacy_radio_managed(&managed.radio, &managed.expected)
        }
        Warning::CapOffOutsideWlanGroup(cap) => t::warning::cap_off_outside_wlan_group(&cap.package),
        Warning::WatchTargetSilent(silent) => {
            t::warning::watch_target_silent(&silent.address, silent.port.as_deref(), &silent.device)
        }
    }
}
