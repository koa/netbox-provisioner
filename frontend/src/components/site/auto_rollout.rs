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
                ClearAutoRolloutError, LastRound, ReleaseAutoRollout, ReleaseVariables,
                RoundOutcome, SiteAutoRollout, SiteAutoRolloutQuery, SiteError, SiteVariables,
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
    Loaded(Option<SiteAutoRollout>),
    AllowApply(bool),
    Error(FrontendError),
    Clear,
    Release(Vec<i32>),
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
                        data.topology.site_by_id.and_then(|site| site.auto_rollout),
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
                self.state = state;
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
            AutoRolloutMsg::Release(devices) => {
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let release = ReleaseAutoRollout::build(ReleaseVariables { devices });
                    if let Err(error) = request(scope.clone(), release).await {
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
                    <Button variant={ButtonVariant::Secondary} disabled={!self.allow_apply} {onclick}>
                        {t::CLEAR}
                    </Button>
                </Alert>
            }
        });
        let ready = (state.seen && state.error.is_none()).then(|| html!(<p>{t::READY}</p>));
        let not_seen = (!state.seen).then(|| html!(<p>{t::NOT_SEEN}</p>));
        let last = state
            .last_round
            .as_ref()
            .map(|last| self.last_round(ctx, last, &state.released));
        html! {
            <Card class="findings">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    {self.error.as_ref().map(FrontendError::to_html)}
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
    fn last_round(&self, ctx: &Context<Self>, last: &LastRound, released: &[i32]) -> Html {
        let outcome = match &last.outcome {
            RoundOutcome::NothingToDo => t::Outcome::NothingToDo,
            RoundOutcome::NotInLine => t::Outcome::NotInLine,
            RoundOutcome::Applied => t::Outcome::Applied,
            RoundOutcome::HeldBack => t::Outcome::HeldBack,
            RoundOutcome::NoInternet => t::Outcome::NoInternet,
            RoundOutcome::Busy => t::Outcome::Busy,
            RoundOutcome::Failed => t::Outcome::Failed,
            RoundOutcome::NetboxUnreadable => t::Outcome::NetboxUnreadable,
            RoundOutcome::Other(other) => t::Outcome::Other(other),
        };
        let line = t::last_round(outcome, &t::ago(last.age.into()), &names(&last.applied));
        let deferred = (!last.deferred.is_empty())
            .then(|| html!(<p>{t::deferred(&names(&last.deferred))}</p>));
        let held_by = (!last.held_by.is_empty()).then(|| {
            let ids: Vec<i32> = last.held_by.iter().map(|device| device.id).collect();
            let all_released = ids.iter().all(|id| released.contains(id));
            let onclick = ctx
                .link()
                .callback(move |_| AutoRolloutMsg::Release(ids.clone()));
            html! {
                <Alert inline=true r#type={AlertType::Warning} title={t::HELD_BY}>
                    <p>{names(&last.held_by).join(", ")}</p>
                    if all_released {
                        <p>{t::RELEASED}</p>
                    } else {
                        <Button variant={ButtonVariant::Secondary} disabled={!self.allow_apply} {onclick}>
                            {t::RELEASE}
                        </Button>
                    }
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
}

fn names(devices: &[crate::graphql::authenticated::auto_rollout::RoundDevice]) -> Vec<String> {
    devices.iter().map(|device| device.name.clone()).collect()
}
