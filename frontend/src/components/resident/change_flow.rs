//! Scope: What every change of a resident shares: asking its plan, saving it, asking how saving
//! goes and showing that; not a change's form nor the wording of its plan (`wlan_edit`,
//! `port_vlan_edit`).
use crate::{
    error::{FrontendError, ServerProblem},
    graphql::{
        request_resident,
        resident::change::{
            ChangePlanQuery, ChangePlanVariables, ResidentChange, ResidentPlan, SaveChange,
            SaveChangeVariables, SaveState, SavedChange, SavedChangeQuery, SavedChangeVariables,
        },
    },
};
use cynic::{MutationBuilder, QueryBuilder};
use gloo::timers::future::TimeoutFuture;
use i18n::resident as t;
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use yew::{Callback, Component, Html, MouseEvent, html, html::Scope, platform::spawn_local};

/// Asks the plan of `change` for the preview and hands it to the component as `to_msg`.
pub fn ask_plan<C: Component>(
    scope: &Scope<C>,
    resident: i32,
    change: ResidentChange,
    to_msg: fn(Result<ResidentPlan, FrontendError>) -> C::Message,
) {
    let scope = scope.clone();
    spawn_local(async move {
        let variables = ChangePlanVariables { resident, change };
        let plan = request_resident(scope.clone(), ChangePlanQuery::build(variables))
            .await
            .map(|data| data.change_plan);
        scope.send_message(to_msg(plan));
    });
}

/// Saves `change` and then asks how saving goes until it is through, each answer as `to_msg`.
pub fn save<C: Component>(
    scope: &Scope<C>,
    resident: i32,
    change: ResidentChange,
    fingerprint: String,
    to_msg: fn(Result<SavedChange, FrontendError>) -> C::Message,
) {
    let scope = scope.clone();
    spawn_local(async move {
        let variables = SaveChangeVariables {
            resident,
            change,
            fingerprint,
        };
        let mut saved = request_resident(scope.clone(), SaveChange::build(variables))
            .await
            .map(|data| data.save_change);
        while let Ok(change) = &saved
            && change.state == SaveState::Saving
        {
            scope.send_message(to_msg(Ok(change.clone())));
            TimeoutFuture::new(1_000).await;
            let job = change.job;
            saved = request_resident(
                scope.clone(),
                SavedChangeQuery::build(SavedChangeVariables { job }),
            )
            .await
            // none once the provisioner forgot the job (restarted)
            .and_then(|data| data.saved_change.ok_or(FrontendError::MissingData));
        }
        scope.send_message(to_msg(saved));
    });
}

/// The devices whose configuration the plan changes, and what the devices at it notice
/// (`clients`); a plan changing no device's configuration says it changes NetBox only.
pub fn devices_alert(plan: &ResidentPlan, clients: Option<&str>) -> Html {
    let devices = plan
        .devices
        .iter()
        .map(|device| device.name.as_str())
        .collect::<Vec<_>>();
    let title = if devices.is_empty() {
        t::NETBOX_ONLY.to_string()
    } else {
        t::devices_change(&devices)
    };
    html! {
        <Alert inline=true r#type={AlertType::Info} {title}>
            { for clients.map(|text| html!(<p>{text}</p>)) }
        </Alert>
    }
}

/// How saving goes: saving, saved (the devices get it from the rollout) or why it failed.
pub fn saving(saved: Option<&SavedChange>, close: Callback<MouseEvent>) -> Html {
    match saved.map(|saved| (&saved.state, saved.error.as_ref())) {
        None | Some((SaveState::Saving, _)) => html!(<p>{t::SAVING}</p>),
        Some((SaveState::Saved, _)) => html! {
            <Alert inline=true r#type={AlertType::Success} title={t::SAVED}>
                <p>{t::SAVED_HINT}</p>
                <Button variant={ButtonVariant::Link} label={t::CLOSE} onclick={close}/>
            </Alert>
        },
        Some((_, error)) => {
            let text = error
                .map(|error| i18n::error::server(&ServerProblem::from(error)))
                .unwrap_or_default();
            html! {
                <Alert inline=true r#type={AlertType::Danger} title={t::NOT_SAVED}>
                    <p>{text}</p>
                    <Button variant={ButtonVariant::Link} label={t::CLOSE} onclick={close}/>
                </Alert>
            }
        }
    }
}
