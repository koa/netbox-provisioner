//! Updates across the network (`/rollout`, `docs/concepts/network-update.md`): the sites and
//! the channel chosen, what a rollout would do read from every RouterOS device of them
//! (`rolloutPlan`, only reading) and shown in the order it would update them.
use crate::{
    components::widgets::popup::Select,
    error::{FrontendError, ServerProblem},
    graphql::{
        authenticated::{
            list_devices::{ListSites, Site},
            rollout::{
                PlannedDevice, RolloutFallback, RolloutPlan, RolloutPlanQuery,
                RolloutPlanVariables, RolloutSkip,
            },
            types::Channel,
        },
        live::{Subscription, watch_data},
        request,
    },
    i18n::rollout as t,
    routes::AppRoute,
};
use cynic::{QueryBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner,
};
use std::{collections::BTreeSet, fmt};
use yew::{Component, Context, Html, html, platform::spawn_local};
use yew_nested_router::components::Link;

pub struct Rollout {
    sites: Option<Result<Vec<Site>, FrontendError>>,
    chosen: BTreeSet<i32>,
    channel: ChannelChoice,
    plan: Plan,
    /// the sites again when NetBox changed
    _subscription: Subscription,
}

enum Plan {
    None,
    Planning,
    Done(RolloutPlan),
    Failed(FrontendError),
}

pub enum RolloutMsg {
    Sites(Result<Vec<Site>, FrontendError>),
    Toggle(i32),
    Channel(ChannelChoice),
    Plan,
    Planned(Result<RolloutPlan, FrontendError>),
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
        Self {
            sites: None,
            chosen: BTreeSet::new(),
            channel: ChannelChoice::default(),
            plan: Plan::None,
            _subscription: subscription,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            RolloutMsg::Sites(sites) => self.sites = Some(sites),
            RolloutMsg::Toggle(site) => {
                if !self.chosen.remove(&site) {
                    self.chosen.insert(site);
                }
            }
            RolloutMsg::Channel(channel) => self.channel = channel,
            RolloutMsg::Plan => {
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
            RolloutMsg::Planned(Ok(plan)) => self.plan = Plan::Done(plan),
            RolloutMsg::Planned(Err(error)) => self.plan = Plan::Failed(error),
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
        let planning = matches!(self.plan, Plan::Planning);
        let label = match self.plan {
            Plan::None => t::PLAN,
            _ => t::PLAN_AGAIN,
        };
        let plan = match &self.plan {
            Plan::None => Html::default(),
            Plan::Planning => html!(<Spinner/>),
            Plan::Failed(error) => error.to_html(),
            Plan::Done(plan) => plan_view(plan),
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
                            disabled={planning || self.chosen.is_empty()}
                            onclick={ctx.link().callback(|_| RolloutMsg::Plan)}
                        >{label}</Button>
                        if self.chosen.is_empty() {
                            {" "}{t::NO_SITE}
                        }
                    </p>
                    {plan}
                </CardBody>
            </Card>
        }
    }
}

fn plan_view(plan: &RolloutPlan) -> Html {
    let updated = plan.devices.iter().filter(|d| d.skip.is_none()).count();
    html! {
        <>
            <p>
                {t::target(plan.channel.name(), &plan.target)}{" · "}
                {t::summary(updated, plan.devices.len() - updated)}
            </p>
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
                    </tr>
                </thead>
                <tbody>
                    { for plan.devices.iter().enumerate().map(|(index, device)| row(index, device, &plan.target)) }
                </tbody>
            </table>
        </>
    }
}

fn row(index: usize, device: &PlannedDevice, target: &str) -> Html {
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
            t::space(device.download.map(|d| d.0), device.free_space.map(|f| f.0))
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
        </tr>
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
