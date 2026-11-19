//! Scope: Changing a resident's WLAN: its name and password, the preview in words, saving and
//! how saving went; not the network card it sits in (`components::resident`).
use crate::{
    error::{FrontendError, ServerProblem},
    graphql::{
        request_resident,
        resident::{
            change::{
                ResidentPlan, SaveState, SaveWlan, SaveWlanVariables, SavedChange,
                SavedChangeQuery, SavedChangeVariables, WlanInput, WlanPlanQuery,
                WlanPlanVariables,
            },
            residents::ResidentWlan,
        },
    },
};
use cynic::{MutationBuilder, QueryBuilder};
use gloo::timers::future::TimeoutFuture;
use i18n::resident as t;
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Form, FormGroup, FormHelperText, TextInput,
    TextInputType,
};
use yew::{Callback, Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct WlanEditProps {
    /// the resident it is changed for (a resident's own, or the one of the support view)
    pub resident: i32,
    pub wlan: ResidentWlan,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

/// Where the change stands: from closed over the form and its preview to saving.
pub enum WlanEdit {
    Closed,
    Editing(Draft),
    /// the plan of `draft`, waiting for the plan or showing it
    Preview {
        draft: Draft,
        plan: Option<ResidentPlan>,
    },
    Saving {
        draft: Draft,
        saved: Option<SavedChange>,
    },
    Failed {
        draft: Draft,
        error: FrontendError,
    },
}

/// The form's values; the password shown in clear or not.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    ssid: String,
    psk: String,
    show_psk: bool,
}

pub enum WlanEditMsg {
    Open,
    Close,
    Ssid(String),
    Psk(String),
    ShowPsk,
    RandomPsk,
    Preview,
    Plan(Result<ResidentPlan, FrontendError>),
    /// from the preview back to the form
    Back,
    Save,
    Saved(Result<SavedChange, FrontendError>),
}

impl Component for WlanEdit {
    type Message = WlanEditMsg;
    type Properties = WlanEditProps;

    fn create(_ctx: &Context<Self>) -> Self {
        WlanEdit::Closed
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let wlan = &ctx.props().wlan;
        let state = std::mem::replace(self, WlanEdit::Closed);
        *self = match (state, msg) {
            (_, WlanEditMsg::Open) => WlanEdit::Editing(Draft {
                ssid: wlan.ssid.clone(),
                psk: wlan.psk.clone().unwrap_or_default(),
                show_psk: false,
            }),
            (_, WlanEditMsg::Close) => WlanEdit::Closed,
            (WlanEdit::Editing(mut draft), msg) => match msg {
                WlanEditMsg::Ssid(ssid) => {
                    draft.ssid = ssid;
                    WlanEdit::Editing(draft)
                }
                WlanEditMsg::Psk(psk) => {
                    draft.psk = psk;
                    WlanEdit::Editing(draft)
                }
                WlanEditMsg::ShowPsk => {
                    draft.show_psk = !draft.show_psk;
                    WlanEdit::Editing(draft)
                }
                WlanEditMsg::RandomPsk => {
                    if let Some(psk) = random_psk() {
                        draft.psk = psk;
                        draft.show_psk = true;
                    }
                    WlanEdit::Editing(draft)
                }
                WlanEditMsg::Preview => {
                    self.ask_plan(ctx, &draft);
                    WlanEdit::Preview { draft, plan: None }
                }
                _ => WlanEdit::Editing(draft),
            },
            (WlanEdit::Preview { draft, .. }, WlanEditMsg::Plan(plan)) => match plan {
                Ok(plan) => WlanEdit::Preview {
                    draft,
                    plan: Some(plan),
                },
                Err(error) => WlanEdit::Failed { draft, error },
            },
            (
                WlanEdit::Preview { draft, .. } | WlanEdit::Failed { draft, .. },
                WlanEditMsg::Back,
            ) => WlanEdit::Editing(draft),
            (
                WlanEdit::Preview {
                    draft,
                    plan: Some(plan),
                },
                WlanEditMsg::Save,
            ) => {
                self.save(ctx, &draft, plan.fingerprint);
                WlanEdit::Saving { draft, saved: None }
            }
            (WlanEdit::Saving { draft, .. }, WlanEditMsg::Saved(saved)) => match saved {
                Ok(saved) => {
                    if saved.state == SaveState::Saved {
                        ctx.props().on_saved.emit(());
                    }
                    WlanEdit::Saving {
                        draft,
                        saved: Some(saved),
                    }
                }
                Err(error) => WlanEdit::Failed { draft, error },
            },
            (state, _) => state,
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        match self {
            WlanEdit::Closed => html! {
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::CHANGE_WLAN}
                    onclick={link.callback(|_| WlanEditMsg::Open)}
                />
            },
            WlanEdit::Editing(draft) => self.form(ctx, draft),
            WlanEdit::Preview { plan: None, .. } => html!(<p>{t::PLANNING}</p>),
            WlanEdit::Preview {
                plan: Some(plan), ..
            } => preview(ctx, plan),
            WlanEdit::Saving { saved, .. } => saving(ctx, saved.as_ref()),
            WlanEdit::Failed { error, .. } => html! {
                <div class="wlan-edit">
                    {error.to_html()}
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::BACK}
                        onclick={link.callback(|_| WlanEditMsg::Back)}
                    />
                </div>
            },
        }
    }
}

impl WlanEdit {
    fn form(&self, ctx: &Context<Self>, draft: &Draft) -> Html {
        let link = ctx.link();
        let wlan = &ctx.props().wlan;
        let psk_type = if draft.show_psk {
            TextInputType::Text
        } else {
            TextInputType::Password
        };
        let show_label = if draft.show_psk {
            t::HIDE_PSK
        } else {
            t::SHOW_PSK
        };
        // an open WLAN keeps its security: only an admin gives it a password
        let psk = wlan.psk.is_some().then(|| {
            html! {
                <FormGroup label={t::PSK} helper_text={FormHelperText::from(t::PSK_HINT)}>
                    <TextInput
                        r#type={psk_type}
                        placeholder={t::PSK_PLACEHOLDER}
                        value={draft.psk.clone()}
                        onchange={link.callback(WlanEditMsg::Psk)}
                    />
                    <div class="wlan-edit-actions">
                        <Button
                            variant={ButtonVariant::Link}
                            label={show_label}
                            onclick={link.callback(|_| WlanEditMsg::ShowPsk)}
                        />
                        <Button
                            variant={ButtonVariant::Link}
                            label={t::RANDOM_PSK}
                            onclick={link.callback(|_| WlanEditMsg::RandomPsk)}
                        />
                    </div>
                </FormGroup>
            }
        });
        let input = Self::input(ctx, draft);
        let unchanged = input.ssid.is_none() && input.psk.is_none();
        html! {
            <div class="wlan-edit">
                <Form>
                    <FormGroup label={t::SSID}>
                        <TextInput
                            placeholder={t::SSID_PLACEHOLDER}
                            value={draft.ssid.clone()}
                            onchange={link.callback(WlanEditMsg::Ssid)}
                        />
                    </FormGroup>
                    {psk}
                </Form>
                <div class="wlan-edit-actions">
                    <Button
                        variant={ButtonVariant::Primary}
                        label={t::PREVIEW}
                        disabled={unchanged}
                        onclick={link.callback(|_| WlanEditMsg::Preview)}
                    />
                    <Button
                        variant={ButtonVariant::Link}
                        label={t::DISCARD}
                        onclick={link.callback(|_| WlanEditMsg::Close)}
                    />
                </div>
            </div>
        }
    }

    /// The change as the form has it: only what differs from the WLAN now.
    fn input(ctx: &Context<Self>, draft: &Draft) -> WlanInput {
        let wlan = &ctx.props().wlan;
        WlanInput {
            wlan: wlan.id,
            ssid: (draft.ssid != wlan.ssid).then(|| draft.ssid.clone()),
            psk: (wlan.psk.as_ref() != Some(&draft.psk) && wlan.psk.is_some())
                .then(|| draft.psk.clone()),
        }
    }

    fn ask_plan(&self, ctx: &Context<Self>, draft: &Draft) {
        let scope = ctx.link().clone();
        let variables = WlanPlanVariables {
            resident: ctx.props().resident,
            change: Self::input(ctx, draft),
        };
        spawn_local(async move {
            let plan = request_resident(scope.clone(), WlanPlanQuery::build(variables))
                .await
                .map(|data| data.wlan_plan);
            scope.send_message(WlanEditMsg::Plan(plan));
        });
    }

    /// Saves and then asks how saving goes until it is through.
    fn save(&self, ctx: &Context<Self>, draft: &Draft, fingerprint: String) {
        let scope = ctx.link().clone();
        let variables = SaveWlanVariables {
            resident: ctx.props().resident,
            change: Self::input(ctx, draft),
            fingerprint,
        };
        spawn_local(async move {
            let mut saved = request_resident(scope.clone(), SaveWlan::build(variables))
                .await
                .map(|data| data.save_wlan);
            while let Ok(change) = &saved
                && change.state == SaveState::Saving
            {
                scope.send_message(WlanEditMsg::Saved(Ok(change.clone())));
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
            scope.send_message(WlanEditMsg::Saved(saved));
        });
    }
}

/// The plan in words, the devices that change, and saving it.
fn preview(ctx: &Context<WlanEdit>, plan: &ResidentPlan) -> Html {
    let link = ctx.link();
    let changes = plan.wlans.iter().flat_map(|wlan| {
        let ssid = wlan
            .ssid
            .as_deref()
            .map(|ssid| t::ssid_change(&wlan.ssid_before, ssid));
        let psk = wlan.psk_set.then(|| t::psk_change(&wlan.ssid_before));
        ssid.into_iter().chain(psk)
    });
    let devices = plan
        .devices
        .iter()
        .map(|device| device.name.as_str())
        .collect::<Vec<_>>();
    html! {
        <div class="wlan-edit">
            <ul class="resident-facts">
                { for changes.map(|change| html!(<li>{change}</li>)) }
            </ul>
            <Alert inline=true r#type={AlertType::Info} title={t::devices_change(&devices)}>
                <p>{t::CLIENTS_RECONNECT}</p>
            </Alert>
            <div class="wlan-edit-actions">
                <Button
                    variant={ButtonVariant::Primary}
                    label={t::SAVE}
                    onclick={link.callback(|_| WlanEditMsg::Save)}
                />
                <Button
                    variant={ButtonVariant::Link}
                    label={t::BACK}
                    onclick={link.callback(|_| WlanEditMsg::Back)}
                />
            </div>
        </div>
    }
}

/// How saving goes: saving, saved (the devices get it from the rollout) or why it failed.
fn saving(ctx: &Context<WlanEdit>, saved: Option<&SavedChange>) -> Html {
    let close = ctx.link().callback(|_| WlanEditMsg::Close);
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

/// A password of 16 characters from the browser's random source, without characters one
/// mixes up when typing it off a screen (0 O, 1 l I); none without that source.
fn random_psk() -> Option<String> {
    const CHARACTERS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    const LENGTH: usize = 16;
    // bytes below the largest multiple of the characters only, so each is as likely
    let limit = 256 - 256 % CHARACTERS.len();
    let crypto = web_sys::window()?.crypto().ok()?;
    let mut psk = String::with_capacity(LENGTH);
    while psk.len() < LENGTH {
        let mut bytes = [0u8; 32];
        crypto.get_random_values_with_u8_array(&mut bytes).ok()?;
        psk.extend(
            bytes
                .iter()
                .map(|byte| usize::from(*byte))
                .filter(|byte| *byte < limit)
                .map(|byte| CHARACTERS[byte % CHARACTERS.len()] as char)
                .take(LENGTH - psk.len()),
        );
    }
    Some(psk)
}
