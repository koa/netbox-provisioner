//! Scope: A site's automatic rollout: its state, the last round, clearing an error and releasing
//! a manual device; not the rounds (backend `auto_rollout`).
//!
//! Shown for a site with the custom field `auto_rollout` only, subscribed: a round, a cleared
//! error or a release shows at once. Clearing and releasing need `allowApply`, like applying.
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{
            auto_rollout::{
                AcceptAutoRolloutDiff, ClearAutoRolloutError, Lack, LastRound, ReleaseAutoRollout,
                ReleaseVariables, RoundDevice, RoundOutcome, SiteAutoRollout, SiteAutoRolloutQuery,
                SiteError, SiteVariables,
            },
            device::AllowApply,
        },
        live::{Subscription, watch_data},
        request,
    },
};
use cynic::{MutationBuilder, QueryBuilder, SubscriptionBuilder};
use i18n::auto_rollout as t;
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle,
};
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct AutoRolloutProps {
    pub site: u32,
}

pub struct AutoRollout {
    state: Option<SiteAutoRollout>,
    allow_apply: bool,
    error: Option<FrontendError>,
    _subscription: Subscription,
}

pub enum AutoRolloutMsg {
    Loaded(Option<Box<SiteAutoRollout>>),
    AllowApply(bool),
    Error(FrontendError),
    Clear,
    Release(i32),
    Accept(i32),
}

impl Component for AutoRollout {
    type Message = AutoRolloutMsg;
    type Properties = AutoRolloutProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let variables = SiteVariables {
            site: ctx.props().site as i32,
        };
        let subscription = watch_data(
            ctx.link(),
            SiteAutoRolloutQuery::build(variables),
            move |data| {
                scope.send_message(match data {
                    Ok(data) => AutoRolloutMsg::Loaded(
                        data.topology
                            .site_by_id
                            .and_then(|site| site.auto_rollout)
                            .map(Box::new),
                    ),
                    Err(error) => AutoRolloutMsg::Error(error),
                })
            },
        );
        let scope = ctx.link().clone();
        spawn_local(async move {
            if let Ok(data) = request(scope.clone(), AllowApply::build(())).await {
                scope.send_message(AutoRolloutMsg::AllowApply(data.allow_apply));
            }
        });
        Self {
            state: None,
            allow_apply: false,
            error: None,
            _subscription: subscription,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            AutoRolloutMsg::Loaded(state) => {
                self.state = state.map(|state| *state);
                self.error = None;
            }
            AutoRolloutMsg::AllowApply(allowed) => self.allow_apply = allowed,
            AutoRolloutMsg::Error(error) => self.error = Some(error),
            AutoRolloutMsg::Clear => {
                let scope = ctx.link().clone();
                let site = ctx.props().site as i32;
                spawn_local(async move {
                    let clear = ClearAutoRolloutError::build(SiteVariables { site });
                    if let Err(error) = request(scope.clone(), clear).await {
                        scope.send_message(AutoRolloutMsg::Error(error));
                    }
                });
                return false;
            }
            AutoRolloutMsg::Release(device) => {
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let devices = vec![device];
                    let release = ReleaseAutoRollout::build(ReleaseVariables { devices });
                    if let Err(error) = request(scope.clone(), release).await {
                        scope.send_message(AutoRolloutMsg::Error(error));
                    }
                });
                return false;
            }
            AutoRolloutMsg::Accept(device) => {
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let devices = vec![device];
                    let accept = AcceptAutoRolloutDiff::build(ReleaseVariables { devices });
                    if let Err(error) = request(scope.clone(), accept).await {
                        scope.send_message(AutoRolloutMsg::Error(error));
                    }
                });
                return false;
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let Some(state) = &self.state else {
            return self
                .error
                .as_ref()
                .map(FrontendError::to_html)
                .unwrap_or_default();
        };
        let error = state.error.as_ref().map(|error| {
            let reason = match &error.reason {
                SiteError::NotInLine => t::SiteError::NotInLine,
                SiteError::ApplyFailed => t::SiteError::ApplyFailed,
                SiteError::Other(other) => t::SiteError::Other(other),
            };
            let devices = names(&error.devices);
            let onclick = ctx.link().callback(|_| AutoRolloutMsg::Clear);
            html! {
                <Alert inline=true r#type={AlertType::Danger}
                    title={t::error(reason, &devices, &t::ago(error.age.into()))}>
                    <Button variant={ButtonVariant::Secondary} disabled={!self.allow_apply}
                        {onclick}>
                        {t::CLEAR}
                    </Button>
                </Alert>
            }
        });
        let unmet = (!state.unmet.is_empty()).then(|| {
            html! {
                <Alert inline=true r#type={AlertType::Danger} title={t::UNMET}>
                    <dl class="neighbor-results">
                        { for state.unmet.iter().map(|unmet| html! {
                            <>
                                <dt>{unmet.device.name.clone()}</dt>
                                <dd>{unmet.lacks.iter().map(lack).collect::<Vec<_>>().join(", ")}</dd>
                            </>
                        }) }
                    </dl>
                </Alert>
            }
        });
        let ready = (state.seen && state.error.is_none() && state.unmet.is_empty())
            .then(|| html!(<p>{t::READY}</p>));
        let not_seen = (!state.seen).then(|| html!(<p>{t::NOT_SEEN}</p>));
        let last = state
            .last_round
            .as_ref()
            .map(|last| self.last_round(ctx, last, state));
        html! {
            <Card class="findings">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    {self.error.as_ref().map(FrontendError::to_html)}
                    {unmet}
                    {error}
                    {ready}
                    {not_seen}
                    {last}
                </CardBody>
            </Card>
        }
    }
}

impl AutoRollout {
    fn last_round(&self, ctx: &Context<Self>, last: &LastRound, state: &SiteAutoRollout) -> Html {
        let outcome = match &last.outcome {
            RoundOutcome::NothingToDo => t::Outcome::NothingToDo,
            RoundOutcome::NotInLine => t::Outcome::NotInLine,
            RoundOutcome::Applied => t::Outcome::Applied,
            RoundOutcome::HeldBack => t::Outcome::HeldBack,
            RoundOutcome::NoInternet => t::Outcome::NoInternet,
            RoundOutcome::Busy => t::Outcome::Busy,
            RoundOutcome::Failed => t::Outcome::Failed,
            RoundOutcome::NetboxUnreadable => t::Outcome::NetboxUnreadable,
            RoundOutcome::PreconditionUnmet => t::Outcome::PreconditionUnmet,
            RoundOutcome::Other(other) => t::Outcome::Other(other),
        };
        let line = t::last_round(outcome, &t::ago(last.age.into()), &names(&last.applied));
        let deferred = (!last.deferred.is_empty())
            .then(|| html!(<p>{t::deferred(&names(&last.deferred))}</p>));
        let held_by = (!last.held_by.is_empty()).then(|| {
            html! {
                <Alert inline=true r#type={AlertType::Warning} title={t::HELD_BY}>
                    <dl class="neighbor-results">
                        { for last.held_by.iter().map(|device| html! {
                            <>
                                <dt>{device.name.clone()}</dt>
                                <dd>{self.held_device(ctx, device, state)}</dd>
                            </>
                        }) }
                    </dl>
                </Alert>
            }
        });
        html! {
            <>
                <p>{line}</p>
                {deferred}
                {held_by}
            </>
        }
    }

    /// What an admin does about a manual device holding the rounds back.
    fn held_device(
        &self,
        ctx: &Context<Self>,
        device: &RoundDevice,
        state: &SiteAutoRollout,
    ) -> Html {
        let id = device.id;
        if state.released.contains(&id) {
            return html!({ t::RELEASED });
        }
        if !state.holding.contains(&id) {
            return html!({ t::ACCEPTED });
        }
        let release = ctx.link().callback(move |_| AutoRolloutMsg::Release(id));
        let accept = ctx.link().callback(move |_| AutoRolloutMsg::Accept(id));
        html! {
            <>
                <Button variant={ButtonVariant::Secondary} disabled={!self.allow_apply}
                    onclick={release}>
                    {t::RELEASE}
                </Button>
                {" "}
                <Button variant={ButtonVariant::Secondary} disabled={!self.allow_apply}
                    onclick={accept}>
                    {t::ACCEPT}
                </Button>
            </>
        }
    }
}

fn lack(lack: &Lack) -> String {
    t::lack(match lack {
        Lack::LoopbackIpv4 => t::Lack::LoopbackIpv4,
        Lack::LoopbackIpv6 => t::Lack::LoopbackIpv6,
        Lack::Connection => t::Lack::Connection,
        Lack::Scheduler => t::Lack::Scheduler,
        Lack::Other(other) => t::Lack::Other(other),
    })
}

fn names(devices: &[RoundDevice]) -> Vec<String> {
    devices.iter().map(|device| device.name.clone()).collect()
}
