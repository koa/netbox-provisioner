//! The jobs saved before a restart of the provisioner that didn't go on (`savedJobs`): their
//! device is gone from NetBox, or the file couldn't be read. Each stays until the operator
//! discards it; what the job left on the device stays there either way.
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{
            DiscardSavedJob, SavedJobs as SavedJobsQuery, discard_saved_job,
            saved_jobs::{self, ApplyState as A, HeldBack, SavedJobKind, SavedJobsSavedJobs as Saved, SavedJobsSavedJobsPhase as Phase, UpdateState as U},
        },
        query_authenticated,
    },
    i18n::{job::phase as p, saved_jobs as t},
};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use yew::{Component, Context, Html, html, platform::spawn_local};

pub struct SavedJobs {
    saved: Vec<Saved>,
    allow_apply: bool,
    error: Option<FrontendError>,
}

pub enum SavedJobsMsg {
    Loaded(Result<saved_jobs::ResponseData, FrontendError>),
    Discard(i64),
    Discarded(Result<(), FrontendError>),
}

impl SavedJobs {
    fn load(ctx: &Context<Self>) {
        let scope = ctx.link().clone();
        spawn_local(async move {
            let loaded = query_authenticated::<SavedJobsQuery, _>(scope.clone(), saved_jobs::Variables {}).await;
            scope.send_message(SavedJobsMsg::Loaded(loaded));
        });
    }
}

impl Component for SavedJobs {
    type Message = SavedJobsMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        Self::load(ctx);
        Self {
            saved: Vec::new(),
            allow_apply: false,
            error: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            SavedJobsMsg::Loaded(Ok(data)) => {
                self.saved = data.saved_jobs;
                self.allow_apply = data.allow_apply;
            }
            SavedJobsMsg::Loaded(Err(error)) => self.error = Some(error),
            SavedJobsMsg::Discard(device_id) => {
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let discarded = query_authenticated::<DiscardSavedJob, _>(
                        scope.clone(),
                        discard_saved_job::Variables { device_id },
                    )
                    .await
                    .map(|_| ());
                    scope.send_message(SavedJobsMsg::Discarded(discarded));
                });
                return false;
            }
            SavedJobsMsg::Discarded(result) => {
                self.error = result.err();
                Self::load(ctx);
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let error = self.error.as_ref().map(FrontendError::to_html);
        let saved = self.saved.iter().map(|saved| {
            let device_id = saved.device_id;
            let title = match &saved.device_name {
                Some(name) => t::title(name),
                None => t::title_unreadable(device_id),
            };
            let text = match &saved.unreadable {
                Some(why) => t::unreadable(why),
                None => t::saved(
                    match saved.kind {
                        Some(SavedJobKind::APPLY) => t::APPLY,
                        _ => t::UPDATE,
                    },
                    &saved.phase.as_ref().map(phase).unwrap_or_default(),
                    saved.age.unwrap_or_default() / 1_000_000_000,
                ),
            };
            let why = match saved.held_back {
                Some(HeldBack::DEVICE_GONE) => Some(t::DEVICE_GONE),
                Some(HeldBack::BATCH_INCOMPLETE) => Some(t::BATCH_INCOMPLETE),
                _ => None,
            };
            html! {
                <Alert inline=true r#type={AlertType::Warning} {title}>
                    <p>{text}</p>
                    if let Some(why) = why {
                        <p>{why}</p>
                    }
                    <p>{t::LEFT_ON_DEVICE}</p>
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::DISCARD}
                        disabled={!self.allow_apply}
                        onclick={ctx.link().callback(move |_| SavedJobsMsg::Discard(device_id))}
                    />
                </Alert>
            }
        });
        html! {<>{error}{for saved}</>}
    }
}

/// The state it was saved in; only those that go on after a restart are ever saved.
fn phase(phase: &Phase) -> String {
    let state = match phase {
        Phase::ApplyPhase(phase) => match &phase.apply {
            A::SCRIPT_RUNNING => p::SCRIPT_RUNNING,
            A::STOPPING_FALLBACK => p::STOPPING_FALLBACK,
            other => return format!("{other:?}"),
        },
        Phase::UpdatePhase(phase) => match &phase.update {
            U::REBOOTING => p::REBOOTING,
            U::NOT_UPDATED => p::NOT_UPDATED,
            U::STOPPING_FALLBACK => p::STOPPING_FALLBACK,
            other => return format!("{other:?}"),
        },
        other => return format!("{other:?}"),
    };
    state.to_string()
}
