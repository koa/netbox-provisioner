//! Scope: RouterOS on the device page: version and packages, update, downgrade, the watchdogs
//! an update suspends, reboot and power cycle; not starting the jobs (the page).
use crate::{
    components::{
        device::power_cycle::PowerCycle,
        widgets::{confirm::Confirm, popup::Select},
    },
    error::ServerProblem,
    graphql::authenticated::{
        device::{ChannelUpdate, PowerSupply, Routeros as Ros, Watchdog},
        types::Channel,
    },
    i18n::{common::CANCEL, device as t},
};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant, TextInput};
use yew::{Callback, Component, Context, Html, Properties, html};

/// An update or a downgrade asked for, with the choices about its way back.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateRequest {
    /// the channel to update in; `None`: only the packages, at the running version
    pub channel: Option<Channel>,
    /// suspend the watchdogs that may fire meanwhile (only asked when one watches something)
    pub suspend_watchdogs: bool,
    /// although nothing brings the device back (no two partitions, no neighbor)
    pub without_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct RouterosProps {
    pub routeros: Ros,
    /// the watchdogs that may fire while this device updates
    pub watchdogs: Vec<Watchdog>,
    /// the device feeding it over PoE and its port: a cold boot is possible
    pub power_supply: Option<PowerSupply>,
    /// nothing brings the device back from an update: only once confirmed
    pub no_way_back: bool,
    /// a job runs, or writing is locked
    pub disabled: bool,
    /// writing to devices is allowed (`allow_apply`)
    pub allow_apply: bool,
    /// the supplier's job switching the power off and on, once started
    pub power_job: Option<i32>,
    pub on_update: Callback<UpdateRequest>,
    /// downgrade to the version, confirmed; `channel` of the request is `None`
    pub on_downgrade: Callback<(String, UpdateRequest)>,
    /// „Neu starten“, confirmed
    pub on_reboot: Callback<()>,
    /// „Strom aus und ein“, confirmed
    pub on_power_cycle: Callback<()>,
}

/// RouterOS as a `<dt>`/`<dd>` pair.
pub struct Routeros {
    /// the RouterOS channel to update in; `None` until the device's default is known
    channel: Option<Channel>,
    /// the older version to downgrade to, typed or the one before an update of this run
    downgrade_version: String,
    /// „Auf … downgraden“ was clicked, waiting for the confirmation
    confirm_downgrade: bool,
    /// „Neu starten“ was clicked, waiting for the confirmation
    confirm_reboot: bool,
    /// suspend the watchdogs that may fire while this one updates
    suspend_watchdogs: bool,
    /// update although nothing brings the device back (no two partitions, no neighbor)
    without_fallback: bool,
}

pub enum RouterosMsg {
    /// the label of a channel entry (`channel_label`)
    SelectChannel(Box<str>),
    Update,
    /// bring the packages in line at the running version
    AdjustPackages,
    DowngradeVersion(String),
    ConfirmDowngrade(bool),
    Downgrade,
    ConfirmReboot(bool),
    Reboot,
    SuspendWatchdogs(bool),
    WithoutFallback(bool),
}

impl Component for Routeros {
    type Message = RouterosMsg;
    type Properties = RouterosProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut routeros = Self {
            channel: None,
            downgrade_version: String::new(),
            confirm_downgrade: false,
            confirm_reboot: false,
            suspend_watchdogs: true,
            without_fallback: false,
        };
        routeros.preset(ctx.props());
        routeros
    }

    fn changed(&mut self, ctx: &Context<Self>, _old: &Self::Properties) -> bool {
        self.preset(ctx.props());
        true
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let props = ctx.props();
        match msg {
            RouterosMsg::SelectChannel(label) => {
                self.channel = props
                    .routeros
                    .channels
                    .iter()
                    .find(|c| channel_label(c) == label)
                    .map(|c| c.channel.clone());
            }
            RouterosMsg::Update => {
                if let Some(channel) = self.channel.clone() {
                    props.on_update.emit(self.request(props, Some(channel)));
                }
            }
            RouterosMsg::AdjustPackages => props.on_update.emit(self.request(props, None)),
            RouterosMsg::DowngradeVersion(version) => self.downgrade_version = version,
            RouterosMsg::ConfirmDowngrade(confirm) => self.confirm_downgrade = confirm,
            RouterosMsg::Downgrade => {
                self.confirm_downgrade = false;
                let version = self.downgrade_version.trim().to_string();
                props
                    .on_downgrade
                    .emit((version, self.request(props, None)));
            }
            RouterosMsg::ConfirmReboot(confirm) => self.confirm_reboot = confirm,
            RouterosMsg::Reboot => {
                self.confirm_reboot = false;
                props.on_reboot.emit(());
            }
            RouterosMsg::SuspendWatchdogs(suspend) => self.suspend_watchdogs = suspend,
            RouterosMsg::WithoutFallback(without) => self.without_fallback = without,
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let routeros = &props.routeros;
        let update_locked = props.no_way_back && !self.without_fallback;
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
        let adjust_packages =
            routeros.adjust_packages && !selected.is_some_and(|c| c.update && !c.upgrade);
        let reboot = Confirm {
            asking: self.confirm_reboot,
            question: t::REBOOT_CONFIRM.into(),
            now: t::REBOOT_NOW,
            ask: t::REBOOT.into(),
            disabled: props.disabled,
        }
        .view(
            ctx.link().callback(|_| RouterosMsg::Reboot),
            ctx.link().callback(RouterosMsg::ConfirmReboot),
        );
        html! {<>
            <dt>{t::ROUTEROS}</dt><dd>
                { routeros.version.clone() }
                {t::packages(&routeros.packages, &routeros.wanted)}
                <div class="routeros-update">
                    <Select<Box<str>>
                        selected={selected.map(channel_label)}
                        entries={entries}
                        onselect={ctx.link().callback(RouterosMsg::SelectChannel)}
                    />
                    if let Some(action) = action {
                        {" "}
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={action}
                            disabled={update_locked || props.disabled}
                            onclick={ctx.link().callback(|_| RouterosMsg::Update)}
                        />
                    } else if !adjust_packages {
                        {t::UP_TO_DATE}
                    }
                    if adjust_packages {
                        {" "}
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={t::ADJUST_PACKAGES}
                            disabled={update_locked || props.disabled}
                            onclick={ctx.link().callback(|_| RouterosMsg::AdjustPackages)}
                        />
                    }
                </div>
                if !routeros.version.starts_with("6.") {
                    { self.downgrade_view(ctx, update_locked || props.disabled) }
                }
                if props.no_way_back {
                    { self.no_way_back_view(ctx) }
                }
                { self.watchdogs_view(ctx) }
                <div class="device-reboot">
                    {reboot}
                    if let Some(supply) = &props.power_supply {
                        {" "}
                        <PowerCycle
                            supply={supply.clone()}
                            allow_apply={props.allow_apply}
                            job={props.power_job}
                            on_cycle={props.on_power_cycle.clone()}
                        />
                    }
                </div>
            </dd>
        </>}
    }
}

impl Routeros {
    /// The channel and the version to go back to, from the device's default and the update
    /// before, until chosen.
    fn preset(&mut self, props: &RouterosProps) {
        if self.channel.is_none() {
            self.channel = Some(props.routeros.default_channel.clone());
        }
        if self.downgrade_version.is_empty()
            && let Some(previous) = props.routeros.previous.clone()
        {
            self.downgrade_version = previous;
        }
    }

    /// An update in `channel` with the choices made.
    fn request(&self, props: &RouterosProps, channel: Option<Channel>) -> UpdateRequest {
        UpdateRequest {
            channel,
            suspend_watchdogs: self.suspend(props),
            without_fallback: self.without_fallback,
        }
    }

    /// Whether an update suspends the watchdogs: asked for, and one watches something.
    fn suspend(&self, props: &RouterosProps) -> bool {
        self.suspend_watchdogs
            && props
                .watchdogs
                .iter()
                .any(|w| w.watching.watch_address.is_some() || !w.watching.poe_pings.is_empty())
    }

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
                        onclick={ctx.link().callback(|_| RouterosMsg::Downgrade)}
                    />
                    {" "}
                    <Button
                        variant={ButtonVariant::Link}
                        label={CANCEL}
                        onclick={ctx.link().callback(|_| RouterosMsg::ConfirmDowngrade(false))}
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
                    onchange={ctx.link().callback(RouterosMsg::DowngradeVersion)}
                />
                {" "}
                <Button
                    variant={ButtonVariant::Secondary}
                    label={label}
                    disabled={disabled || version.is_empty()}
                    onclick={ctx.link().callback(|_| RouterosMsg::ConfirmDowngrade(true))}
                />
            </div>
        }
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
                        onchange={ctx.link().callback(move |_| RouterosMsg::WithoutFallback(!without))}
                    />
                    {" "}{t::UPDATE_WITHOUT_WAY_BACK}
                </label>
            </Alert>
        }
    }

    /// The watchdogs that may fire while this device updates, where one watches something or
    /// can't be read, with the choice to suspend them during an update.
    fn watchdogs_view(&self, ctx: &Context<Self>) -> Html {
        let watchdogs = &ctx.props().watchdogs;
        let watching: Vec<Html> = watchdogs
            .iter()
            .flat_map(|w| {
                let address = w.watching.watch_address.as_deref().map(
                    |address| html!(<p>{t::watchdog_watching(&w.device, w.supplier, address)}</p>),
                );
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
                            onchange={ctx.link().callback(move |_| RouterosMsg::SuspendWatchdogs(!suspend))}
                        />
                        {" "}{t::SUSPEND_WATCHDOGS}
                    </label>
                }
            </Alert>
        }
    }
}

/// What the channel select shows, e.g. `stable: 7.20.8`.
fn channel_label(channel: &ChannelUpdate) -> Box<str> {
    t::channel(channel.channel.name(), &channel.newest).into()
}
