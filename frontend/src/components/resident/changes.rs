//! Scope: The tab of a resident's saved changes: each with how far it has reached its devices,
//! read again every 15 s while shown, and taking one back (its parts go to the view as
//! drafts); not saving them (`save_dialog`).
use crate::{
    error::FrontendError,
    graphql::{
        request_resident,
        resident::changes::{
            ChangeDevice, ChangeState, ChangesQuery, ChangesVariables, HoldReason, RolloutState,
            UndoChangeQuery, UndoChangeVariables, UndoStep,
        },
    },
};
use cynic::QueryBuilder;
use gloo::timers::callback::Interval;
use i18n::{auto_rollout as rollout, resident as t};
use patternfly_yew::prelude::{Button, ButtonVariant, Color, Label};
use yew::{Callback, Component, Context, Html, Properties, html, platform::spawn_local};

/// How often the tab reads the changes again, in ms.
const EVERY: u32 = 15_000;

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ChangesProps {
    pub resident: i32,
    /// what takes a change back: the view makes drafts of it and opens the preview
    pub on_undo: Callback<Vec<UndoStep>>,
}

pub struct ChangesTab {
    changes: Option<Result<Vec<ChangeState>, FrontendError>>,
    /// taking a change back failed, or it can't be anymore (none of it left)
    undo_failed: Option<Option<FrontendError>>,
    _every: Interval,
}

pub enum ChangesMsg {
    Read,
    Answer(Result<Vec<ChangeState>, FrontendError>),
    /// take the change with this id back
    Undo(i32),
    UndoAnswer(Result<Vec<UndoStep>, FrontendError>),
}

impl Component for ChangesTab {
    type Message = ChangesMsg;
    type Properties = ChangesProps;

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(ChangesMsg::Read);
        let link = ctx.link().clone();
        ChangesTab {
            changes: None,
            undo_failed: None,
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
            ChangesMsg::Undo(change) => {
                let scope = ctx.link().clone();
                let variables = UndoChangeVariables {
                    resident: ctx.props().resident,
                    change,
                };
                self.undo_failed = None;
                spawn_local(async move {
                    let answer = request_resident(scope.clone(), UndoChangeQuery::build(variables))
                        .await
                        .map(|data| data.undo_change);
                    scope.send_message(ChangesMsg::UndoAnswer(answer));
                });
                true
            }
            ChangesMsg::UndoAnswer(Ok(steps)) if steps.is_empty() => {
                self.undo_failed = Some(None);
                true
            }
            ChangesMsg::UndoAnswer(Ok(steps)) => {
                ctx.props().on_undo.emit(steps);
                false
            }
            ChangesMsg::UndoAnswer(Err(error)) => {
                self.undo_failed = Some(Some(error));
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
        let undo_failed = self.undo_failed.as_ref().map(|error| match error {
            Some(error) => error.to_html(),
            None => html!(<p>{t::UNDO_GONE}</p>),
        });
        html! {
            <>
                <p>{t::CHANGES_HINT}</p>
                {undo_failed}
                { for changes.iter().map(|saved| change(ctx, saved)) }
            </>
        }
    }
}

/// One saved change as a card, with taking it back where it can be.
fn change(ctx: &Context<ChangesTab>, change: &ChangeState) -> Html {
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
    let undo = change.undoable.then(|| {
        let id = change.id;
        html! {
            <div class="pf-v6-c-card__footer">
                <Button variant={ButtonVariant::Secondary} label={t::UNDO_CHANGE}
                    onclick={ctx.link().callback(move |_| ChangesMsg::Undo(id))}/>
            </div>
        }
    });
    html! {
        <div class="pf-v6-c-card pf-m-compact resident-change" key={change.id}>
            <div class="pf-v6-c-card__header">
                <div class="pf-v6-c-card__title">{head}</div>
            </div>
            <div class="pf-v6-c-card__body">{body}</div>
            {undo}
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
        HoldReason::Rewire => rollout::HoldReason::Rewire,
        HoldReason::Other(other) => rollout::HoldReason::Other(other),
    })
}
