//! Scope: Changing the network of a resident's port or socket: the choice of its networks or
//! none, the preview in words, saving; not the row it sits in (`components::resident`).
use super::change_flow::{ask_plan, devices_alert, save, saving};
use crate::{
    components::widgets::popup::Select,
    error::FrontendError,
    graphql::resident::{
        change::{
            PortKindInput, PortVlanChange, ResidentChange, ResidentPlan, SaveState, SavedChange,
            VlanRef,
        },
        residents::{PortKind, ResidentNetwork, ResidentPort},
    },
};
use i18n::resident as t;
use patternfly_yew::prelude::{Button, ButtonVariant};
use std::fmt::{self, Display};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct PortVlanEditProps {
    /// the resident it is changed for (a resident's own, or the one of the support view)
    pub resident: i32,
    pub port: ResidentPort,
    /// the resident's networks, to choose from
    pub networks: Vec<ResidentNetwork>,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

/// One of the resident's networks, or none.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkChoice {
    vlan: Option<i32>,
    name: String,
}

impl Display for NetworkChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Where the change stands: from closed over the choice and its preview to saving.
pub enum PortVlanEdit {
    Closed,
    Choosing(Option<NetworkChoice>),
    Preview {
        choice: NetworkChoice,
        plan: Option<ResidentPlan>,
    },
    Saving {
        choice: NetworkChoice,
        saved: Option<SavedChange>,
    },
    Failed {
        choice: NetworkChoice,
        error: FrontendError,
    },
}

pub enum PortVlanEditMsg {
    Open,
    Close,
    Choose(NetworkChoice),
    Preview,
    Plan(Result<ResidentPlan, FrontendError>),
    Back,
    Save,
    Saved(Result<SavedChange, FrontendError>),
}

impl Component for PortVlanEdit {
    type Message = PortVlanEditMsg;
    type Properties = PortVlanEditProps;

    fn create(_ctx: &Context<Self>) -> Self {
        PortVlanEdit::Closed
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let state = std::mem::replace(self, PortVlanEdit::Closed);
        *self = match (state, msg) {
            (_, PortVlanEditMsg::Open) => PortVlanEdit::Choosing(current(ctx.props())),
            (_, PortVlanEditMsg::Close) => PortVlanEdit::Closed,
            (PortVlanEdit::Choosing(_), PortVlanEditMsg::Choose(choice)) => {
                PortVlanEdit::Choosing(Some(choice))
            }
            (PortVlanEdit::Choosing(Some(choice)), PortVlanEditMsg::Preview) => {
                let change = change(ctx.props(), &choice);
                ask_plan(
                    ctx.link(),
                    ctx.props().resident,
                    change,
                    PortVlanEditMsg::Plan,
                );
                PortVlanEdit::Preview { choice, plan: None }
            }
            (PortVlanEdit::Preview { choice, .. }, PortVlanEditMsg::Plan(plan)) => match plan {
                Ok(plan) => PortVlanEdit::Preview {
                    choice,
                    plan: Some(plan),
                },
                Err(error) => PortVlanEdit::Failed { choice, error },
            },
            (
                PortVlanEdit::Preview { choice, .. } | PortVlanEdit::Failed { choice, .. },
                PortVlanEditMsg::Back,
            ) => PortVlanEdit::Choosing(Some(choice)),
            (
                PortVlanEdit::Preview {
                    choice,
                    plan: Some(plan),
                },
                PortVlanEditMsg::Save,
            ) => {
                let change = change(ctx.props(), &choice);
                save(
                    ctx.link(),
                    ctx.props().resident,
                    change,
                    plan.fingerprint,
                    PortVlanEditMsg::Saved,
                );
                PortVlanEdit::Saving {
                    choice,
                    saved: None,
                }
            }
            (PortVlanEdit::Saving { choice, .. }, PortVlanEditMsg::Saved(saved)) => match saved {
                Ok(saved) => {
                    if saved.state == SaveState::Saved {
                        ctx.props().on_saved.emit(());
                    }
                    PortVlanEdit::Saving {
                        choice,
                        saved: Some(saved),
                    }
                }
                Err(error) => PortVlanEdit::Failed { choice, error },
            },
            (state, _) => state,
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        match self {
            PortVlanEdit::Closed => html! {
                <div>
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::CHANGE_NETWORK}
                        onclick={link.callback(|_| PortVlanEditMsg::Open)}
                    />
                </div>
            },
            PortVlanEdit::Choosing(choice) => {
                let unchanged = choice.is_none() || *choice == current(ctx.props());
                html! {
                    <div class="wlan-edit">
                        <Select<NetworkChoice>
                            placeholder={t::CHOOSE_NETWORK}
                            selected={choice.clone()}
                            entries={choices(ctx.props())}
                            onselect={link.callback(PortVlanEditMsg::Choose)}
                        />
                        <div class="wlan-edit-actions">
                            <Button
                                variant={ButtonVariant::Primary}
                                label={t::PREVIEW}
                                disabled={unchanged}
                                onclick={link.callback(|_| PortVlanEditMsg::Preview)}
                            />
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::DISCARD}
                                onclick={link.callback(|_| PortVlanEditMsg::Close)}
                            />
                        </div>
                    </div>
                }
            }
            PortVlanEdit::Preview { plan: None, .. } => html!(<p>{t::PLANNING}</p>),
            PortVlanEdit::Preview {
                plan: Some(plan), ..
            } => preview(ctx, plan),
            PortVlanEdit::Saving { saved, .. } => {
                saving(saved.as_ref(), link.callback(|_| PortVlanEditMsg::Close))
            }
            PortVlanEdit::Failed { error, .. } => html! {
                <div class="wlan-edit">
                    {error.to_html()}
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::BACK}
                        onclick={link.callback(|_| PortVlanEditMsg::Back)}
                    />
                </div>
            },
        }
    }
}

/// The resident's networks and none.
fn choices(props: &PortVlanEditProps) -> Vec<NetworkChoice> {
    props
        .networks
        .iter()
        .map(|network| NetworkChoice {
            vlan: Some(network.id),
            name: network.name.clone(),
        })
        .chain([NetworkChoice {
            vlan: None,
            name: t::NO_NETWORK.into(),
        }])
        .collect()
}

/// What the port has now, if it is one of the choices: not the operator's network.
fn current(props: &PortVlanEditProps) -> Option<NetworkChoice> {
    let port = &props.port;
    if port.foreign_network {
        return None;
    }
    let vlan = port.network.as_ref().map(|network| network.id);
    choices(props)
        .into_iter()
        .find(|choice| choice.vlan == vlan)
}

fn change(props: &PortVlanEditProps, choice: &NetworkChoice) -> ResidentChange {
    let kind = match props.port.kind {
        PortKind::Socket => PortKindInput::Socket,
        _ => PortKindInput::Interface,
    };
    ResidentChange::PortVlan(PortVlanChange {
        port: props.port.id,
        kind,
        vlan: choice.vlan,
    })
}

/// A VLAN of the plan as the resident knows it: one of its networks by name, else the
/// operator's; none is no network.
fn network_name(props: &PortVlanEditProps, vlan: Option<&VlanRef>) -> String {
    match vlan {
        None => t::NO_NETWORK.into(),
        Some(vlan) => props
            .networks
            .iter()
            .find(|network| network.vid == vlan.vid && network.name == vlan.name)
            .map(|network| network.name.clone())
            .unwrap_or_else(|| t::FOREIGN_NETWORK.into()),
    }
}

/// The plan in words, the devices that change, and saving it.
fn preview(ctx: &Context<PortVlanEdit>, plan: &ResidentPlan) -> Html {
    let link = ctx.link();
    let props = ctx.props();
    let port = props.port.label.as_deref().unwrap_or(&props.port.name);
    let changes = plan.ports.iter().map(|update| {
        t::port_network_change(
            port,
            &network_name(props, update.vlan_before.as_ref()),
            &network_name(props, update.vlan.as_ref()),
        )
    });
    let clients = if plan.ports.iter().any(|update| update.vlan.is_none()) {
        t::PORT_CLIENTS_CUT
    } else {
        t::PORT_CLIENTS_MOVE
    };
    html! {
        <div class="wlan-edit">
            <ul class="resident-facts">
                { for changes.map(|change| html!(<li>{change}</li>)) }
            </ul>
            {devices_alert(plan, Some(clients))}
            <div class="wlan-edit-actions">
                <Button
                    variant={ButtonVariant::Primary}
                    label={t::SAVE}
                    onclick={link.callback(|_| PortVlanEditMsg::Save)}
                />
                <Button
                    variant={ButtonVariant::Link}
                    label={t::BACK}
                    onclick={link.callback(|_| PortVlanEditMsg::Back)}
                />
            </div>
        </div>
    }
}
