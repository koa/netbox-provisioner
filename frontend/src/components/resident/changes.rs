//! Scope: The tab of a resident's saved changes: each with how far it has reached its devices,
//! read again every 15 s while shown; not saving them (`save_dialog`).
use crate::{
    error::FrontendError,
    graphql::{
        request_resident,
        resident::changes::{
            ChangeDevice, ChangeState, ChangesQuery, ChangesVariables, HoldReason, RolloutState,
        },
    },
};
use cynic::QueryBuilder;
use gloo::timers::callback::Interval;
use i18n::{auto_rollout as rollout, resident as t};
use patternfly_yew::prelude::{Button, ButtonVariant, Color, Label};
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};

/// How often the tab reads the changes again, in ms.
const EVERY: u32 = 15_000;

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ChangesProps {
    pub resident: i32,
}

pub struct ChangesTab {
    changes: Option<Result<Vec<ChangeState>, FrontendError>>,
    _every: Interval,
}

pub enum ChangesMsg {
    Read,
    Answer(Result<Vec<ChangeState>, FrontendError>),
}

impl Component for ChangesTab {
    type Message = ChangesMsg;
    type Properties = ChangesProps;

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(ChangesMsg::Read);
        let link = ctx.link().clone();
        ChangesTab {
            changes: None,
            _every: Interval::new(EVERY, move || link.send_message(ChangesMsg::Read)),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ChangesMsg::Read => {
                let scope = ctx.link().clone();
                let variables = ChangesVariables {
                    resident: ctx.props().resident,
                };
                spawn_local(async move {
                    let answer = request_resident(scope.clone(), ChangesQuery::build(variables))
                        .await
                        .map(|data| data.changes);
                    scope.send_message(ChangesMsg::Answer(answer));
                });
                false
            }
            ChangesMsg::Answer(answer) => {
                self.changes = Some(answer);
                true
            }
        }
    }

    fn changed(&mut self, ctx: &Context<Self>, _old: &Self::Properties) -> bool {
        ctx.link().send_message(ChangesMsg::Read);
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let changes = match &self.changes {
            None => return html!(<p>{t::READING_CHANGES}</p>),
            Some(Err(error)) => {
                let again = ctx.link().callback(|_| ChangesMsg::Read);
                return html! {
                    <>
                        {error.to_html()}
                        <Button variant={ButtonVariant::Link} label={t::READ_AGAIN}
                            onclick={again}/>
                    </>
                };
            }
            Some(Ok(changes)) => changes,
        };
        if changes.is_empty() {
            return html!(<p>{t::NO_CHANGES}</p>);
        }
        html! {
            <>
                <p>{t::CHANGES_HINT}</p>
                { for changes.iter().map(change) }
            </>
        }
    }
}

/// One saved change as a card.
fn change(change: &ChangeState) -> Html {
    let head = t::change_head(&rollout::ago(change.age.into()), &change.by);
    let done = change
        .devices
        .iter()
        .filter(|device| device.state == RolloutState::RolledOut)
        .count();
    let body = match change.devices.is_empty() {
        true => html!(<p>{t::NETBOX_ONLY}</p>),
        false => html! {
            <>
                <p>{t::change_progress(done, change.devices.len())}</p>
                <dl class="neighbor-results">
                    { for change.devices.iter().map(device) }
                </dl>
            </>
        },
    };
    html! {
        <div class="pf-v6-c-card pf-m-compact resident-change" key={change.id}>
            <div class="pf-v6-c-card__header">
                <div class="pf-v6-c-card__title">{head}</div>
            </div>
            <div class="pf-v6-c-card__body">{body}</div>
        </div>
    }
}

/// A device of a change with its state, and why it waits for an admin.
fn device(device: &ChangeDevice) -> Html {
    let (state, color) = match &device.state {
        RolloutState::RolledOut => (rollout::RolloutState::RolledOut, Color::Green),
        RolloutState::Pending => (rollout::RolloutState::Pending, Color::Blue),
        RolloutState::Waiting if !device.reasons.is_empty() => {
            (rollout::RolloutState::WaitingForAdmin, Color::Orange)
        }
        RolloutState::Waiting => (rollout::RolloutState::Waiting, Color::Orange),
        RolloutState::Deferred => (rollout::RolloutState::Deferred, Color::Orange),
        RolloutState::Failed => (rollout::RolloutState::Failed, Color::Red),
        RolloutState::Manual | RolloutState::Other(_) => {
            (rollout::RolloutState::Manual, Color::Grey)
        }
    };
    let reasons = device
        .reasons
        .iter()
        .map(hold_reason)
        .collect::<Vec<_>>()
        .join("; ");
    html! {
        <>
            <dt>{device.device.name.clone()}</dt>
            <dd>
                <Label label={t::rollout_state(state)} {color} compact=true/>
                { (!reasons.is_empty()).then(|| html!(<p>{reasons}</p>)) }
            </dd>
        </>
    }
}

fn hold_reason(reason: &HoldReason) -> String {
    rollout::hold_reason(match reason {
        HoldReason::PoeCut => rollout::HoldReason::PoeCut,
        HoldReason::CapOff => rollout::HoldReason::CapOff,
        HoldReason::ApiClientsOutside => rollout::HoldReason::ApiClientsOutside,
        HoldReason::RadioLink => rollout::HoldReason::RadioLink,
        HoldReason::Other(other) => rollout::HoldReason::Other(other),
    })
}
