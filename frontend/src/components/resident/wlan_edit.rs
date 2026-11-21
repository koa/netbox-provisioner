//! Scope: Changing a resident's WLAN: its name and password, on or off, and the preview in
//! words; not planning and saving (`change_flow`), nor the network card it sits in
//! (`components::resident`).
use super::change_flow::{self, ask_plan, devices_alert, save, saving};
use crate::{
    error::FrontendError,
    graphql::resident::{
        change::{ResidentChange, ResidentPlan, SaveState, SavedChange, WlanInput},
        residents::ResidentWlan,
    },
};
use i18n::resident as t;
use patternfly_yew::prelude::{
    Button, ButtonVariant, Form, FormGroup, FormHelperText, Switch, TextInput, TextInputType,
};
use yew::{Callback, Component, Context, Html, Properties, html};

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
    enabled: bool,
    show_psk: bool,
}

pub enum WlanEditMsg {
    Open,
    Close,
    Ssid(String),
    Psk(String),
    Enabled(bool),
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
                enabled: wlan.enabled,
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
                WlanEditMsg::Enabled(enabled) => {
                    draft.enabled = enabled;
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
            WlanEdit::Saving { saved, .. } => {
                saving(saved.as_ref(), link.callback(|_| WlanEditMsg::Close))
            }
            WlanEdit::Failed { error, .. } => html! {
                <div class="resident-edit">
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
                    <div class="resident-edit-actions">
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
        let unchanged = input.ssid.is_none() && input.psk.is_none() && input.enabled.is_none();
        html! {
            <div class="resident-edit">
                <Form>
                    <FormGroup label={t::SSID}>
                        <TextInput
                            placeholder={t::SSID_PLACEHOLDER}
                            value={draft.ssid.clone()}
                            onchange={link.callback(WlanEditMsg::Ssid)}
                        />
                    </FormGroup>
                    {psk}
                    <Switch
                        label={t::ENABLED}
                        checked={draft.enabled}
                        onchange={link.callback(WlanEditMsg::Enabled)}
                    />
                </Form>
                <div class="resident-edit-actions">
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
            enabled: (draft.enabled != wlan.enabled).then_some(draft.enabled),
        }
    }

    fn ask_plan(&self, ctx: &Context<Self>, draft: &Draft) {
        let change = ResidentChange::Wlan(Self::input(ctx, draft));
        ask_plan(ctx.link(), ctx.props().resident, change, WlanEditMsg::Plan);
    }

    fn save(&self, ctx: &Context<Self>, draft: &Draft, fingerprint: String) {
        let change = ResidentChange::Wlan(Self::input(ctx, draft));
        save(
            ctx.link(),
            ctx.props().resident,
            change,
            fingerprint,
            WlanEditMsg::Saved,
        );
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
        let enabled = wlan.enabled.map(|enabled| match enabled {
            true => t::wlan_on(&wlan.ssid_before),
            false => t::wlan_off(&wlan.ssid_before),
        });
        ssid.into_iter().chain(psk).chain(enabled)
    });
    // what the connected devices notice: switched on, none are connected yet
    let clients = if plan.wlans.iter().any(|wlan| wlan.enabled == Some(false)) {
        Some(t::CLIENTS_DISCONNECT)
    } else if plan
        .wlans
        .iter()
        .any(|wlan| wlan.ssid.is_some() || wlan.psk_set)
    {
        Some(t::CLIENTS_RECONNECT)
    } else {
        None
    };
    change_flow::preview(
        changes,
        devices_alert(plan, clients),
        link.callback(|_| WlanEditMsg::Save),
        link.callback(|_| WlanEditMsg::Back),
    )
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
