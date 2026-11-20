//! Scope: Linking one of a resident's devices to a port or socket of it, and taking a device at
//! it off its link: the choice, the preview in words, saving; not the row it sits in
//! (`components::resident`).
use super::change_flow::{ask_plan, devices_alert, save, saving};
use crate::{
    components::widgets::popup::Select,
    error::FrontendError,
    graphql::resident::{
        change::{
            ConnectDeviceChange, InterfaceRef, PortKindInput, ResidentChange, ResidentPlan,
            SaveState, SavedChange,
        },
        residents::{PortKind, ResidentDevice, ResidentPort},
    },
};
use i18n::resident::{self as t, PortAt};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use std::fmt::{self, Display};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DeviceLinkEditProps {
    /// the resident it is changed for (a resident's own, or the one of the support view)
    pub resident: i32,
    pub port: ResidentPort,
    /// the resident's devices
    pub devices: Vec<ResidentDevice>,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

/// A device to link, as the choice shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceChoice {
    id: i32,
    name: String,
}

impl Display for DeviceChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// What is changed: a device linked here, or one at the port taken off its link.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkChange {
    Connect(DeviceChoice),
    Disconnect(DeviceChoice),
}

/// Where the change stands: from closed over the choice and its preview to saving.
pub enum DeviceLinkEdit {
    Closed,
    Choosing(Option<DeviceChoice>),
    Preview {
        change: LinkChange,
        plan: Option<ResidentPlan>,
    },
    Saving {
        change: LinkChange,
        saved: Option<SavedChange>,
    },
    Failed {
        change: LinkChange,
        error: FrontendError,
    },
}

pub enum DeviceLinkEditMsg {
    Open,
    Close,
    Choose(DeviceChoice),
    Preview(LinkChange),
    Plan(Result<ResidentPlan, FrontendError>),
    Back,
    Save,
    Saved(Result<SavedChange, FrontendError>),
}

impl Component for DeviceLinkEdit {
    type Message = DeviceLinkEditMsg;
    type Properties = DeviceLinkEditProps;

    fn create(_ctx: &Context<Self>) -> Self {
        DeviceLinkEdit::Closed
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let props = ctx.props();
        let state = std::mem::replace(self, DeviceLinkEdit::Closed);
        *self = match (state, msg) {
            (_, DeviceLinkEditMsg::Open) => DeviceLinkEdit::Choosing(None),
            (_, DeviceLinkEditMsg::Close) => DeviceLinkEdit::Closed,
            (DeviceLinkEdit::Choosing(_), DeviceLinkEditMsg::Choose(choice)) => {
                DeviceLinkEdit::Choosing(Some(choice))
            }
            (_, DeviceLinkEditMsg::Preview(change)) => {
                ask_plan(
                    ctx.link(),
                    props.resident,
                    resident_change(props, &change),
                    DeviceLinkEditMsg::Plan,
                );
                DeviceLinkEdit::Preview { change, plan: None }
            }
            (DeviceLinkEdit::Preview { change, .. }, DeviceLinkEditMsg::Plan(plan)) => match plan {
                Ok(plan) => DeviceLinkEdit::Preview {
                    change,
                    plan: Some(plan),
                },
                Err(error) => DeviceLinkEdit::Failed { change, error },
            },
            (
                DeviceLinkEdit::Preview { change, .. } | DeviceLinkEdit::Failed { change, .. },
                DeviceLinkEditMsg::Back,
            ) => match change {
                LinkChange::Connect(choice) => DeviceLinkEdit::Choosing(Some(choice)),
                LinkChange::Disconnect(_) => DeviceLinkEdit::Closed,
            },
            (
                DeviceLinkEdit::Preview {
                    change,
                    plan: Some(plan),
                },
                DeviceLinkEditMsg::Save,
            ) => {
                save(
                    ctx.link(),
                    props.resident,
                    resident_change(props, &change),
                    plan.fingerprint,
                    DeviceLinkEditMsg::Saved,
                );
                DeviceLinkEdit::Saving {
                    change,
                    saved: None,
                }
            }
            (DeviceLinkEdit::Saving { change, .. }, DeviceLinkEditMsg::Saved(saved)) => match saved
            {
                Ok(saved) => {
                    if saved.state == SaveState::Saved {
                        props.on_saved.emit(());
                    }
                    DeviceLinkEdit::Saving {
                        change,
                        saved: Some(saved),
                    }
                }
                Err(error) => DeviceLinkEdit::Failed { change, error },
            },
            (state, _) => state,
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let props = ctx.props();
        match self {
            DeviceLinkEdit::Closed => {
                let connect = (!choices(props).is_empty()).then(|| {
                    html! {
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={t::CONNECT_DEVICE}
                            onclick={link.callback(|_| DeviceLinkEditMsg::Open)}
                        />
                    }
                });
                let disconnect = linked_here(props).into_iter().map(|device| {
                    let label = t::disconnect_device(&device.name);
                    let change = LinkChange::Disconnect(device);
                    html! {
                        <Button
                            variant={ButtonVariant::Secondary}
                            {label}
                            onclick={link.callback(move |_| {
                                DeviceLinkEditMsg::Preview(change.clone())
                            })}
                        />
                    }
                });
                html!(<div class="wlan-edit-actions">{connect}{ for disconnect }</div>)
            }
            DeviceLinkEdit::Choosing(choice) => {
                let chosen = choice.clone();
                html! {
                    <div class="wlan-edit">
                        <Select<DeviceChoice>
                            placeholder={t::CHOOSE_DEVICE}
                            selected={choice.clone()}
                            entries={choices(props)}
                            onselect={link.callback(DeviceLinkEditMsg::Choose)}
                        />
                        <div class="wlan-edit-actions">
                            <Button
                                variant={ButtonVariant::Primary}
                                label={t::PREVIEW}
                                disabled={choice.is_none()}
                                onclick={link.batch_callback(move |_| {
                                    chosen.clone().map(|chosen| {
                                        DeviceLinkEditMsg::Preview(LinkChange::Connect(chosen))
                                    })
                                })}
                            />
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::DISCARD}
                                onclick={link.callback(|_| DeviceLinkEditMsg::Close)}
                            />
                        </div>
                    </div>
                }
            }
            DeviceLinkEdit::Preview { plan: None, .. } => html!(<p>{t::PLANNING}</p>),
            DeviceLinkEdit::Preview {
                change,
                plan: Some(plan),
            } => preview(ctx, change, plan),
            DeviceLinkEdit::Saving { saved, .. } => {
                saving(saved.as_ref(), link.callback(|_| DeviceLinkEditMsg::Close))
            }
            DeviceLinkEdit::Failed { error, .. } => html! {
                <div class="wlan-edit">
                    {error.to_html()}
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::BACK}
                        onclick={link.callback(|_| DeviceLinkEditMsg::Back)}
                    />
                </div>
            },
        }
    }
}

/// The resident's devices the provisioner links, but the port's own and those at it already.
fn choices(props: &DeviceLinkEditProps) -> Vec<DeviceChoice> {
    let here = linked_here(props);
    props
        .devices
        .iter()
        .filter(|device| device.routeros)
        .filter(|device| !(props.port.on_own_device && device.id == props.port.device.id))
        .filter(|device| here.iter().all(|at| at.id != device.id))
        .map(|device| DeviceChoice {
            id: device.id,
            name: device.name.clone(),
        })
        .collect()
}

/// The resident's devices at the port's cable that the provisioner links.
fn linked_here(props: &DeviceLinkEditProps) -> Vec<DeviceChoice> {
    props
        .port
        .far_ends
        .iter()
        .filter(|end| end.own_device)
        .filter(|end| {
            props
                .devices
                .iter()
                .any(|device| device.id == end.device.id && device.routeros)
        })
        .map(|end| DeviceChoice {
            id: end.device.id,
            name: end.device.name.clone(),
        })
        .collect()
}

fn resident_change(props: &DeviceLinkEditProps, change: &LinkChange) -> ResidentChange {
    match change {
        LinkChange::Connect(device) => ResidentChange::ConnectDevice(ConnectDeviceChange {
            device: device.id,
            port: props.port.id,
            kind: match props.port.kind {
                PortKind::Socket => PortKindInput::Socket,
                _ => PortKindInput::Interface,
            },
            device_port: None,
        }),
        LinkChange::Disconnect(device) => ResidentChange::DisconnectDevice(device.id),
    }
}

fn port_at(port: &InterfaceRef) -> PortAt<'_> {
    PortAt {
        port: &port.name,
        device: &port.device.name,
    }
}

/// The plan in words: the cables, who is away meanwhile, the devices that change.
fn preview(ctx: &Context<DeviceLinkEdit>, change: &LinkChange, plan: &ResidentPlan) -> Html {
    let link = ctx.link();
    let removed = plan
        .cables_removed
        .iter()
        .map(|cable| t::cable_removed(port_at(&cable.port), cable.far.as_ref().map(port_at)));
    let added = plan
        .cables_added
        .iter()
        .map(|cable| t::cable_added(port_at(&cable.a), port_at(&cable.b)));
    let hint = match change {
        LinkChange::Connect(_) => t::LINK_HINT,
        LinkChange::Disconnect(_) => t::UNLINK_HINT,
    };
    // a device taken off its link is away for good, as the hint says
    let connect = matches!(change, LinkChange::Connect(_));
    let cut_off = (connect && !plan.cut_off.is_empty()).then(|| {
        let names = plan
            .cut_off
            .iter()
            .map(|device| device.name.as_str())
            .collect::<Vec<_>>();
        html!(<Alert inline=true r#type={AlertType::Warning} title={t::cut_off(&names)}/>)
    });
    html! {
        <div class="wlan-edit">
            <ul class="resident-facts">
                { for removed.chain(added).map(|line| html!(<li>{line}</li>)) }
            </ul>
            {cut_off}
            {devices_alert(plan, Some(hint))}
            <div class="wlan-edit-actions">
                <Button
                    variant={ButtonVariant::Primary}
                    label={t::SAVE}
                    onclick={link.callback(|_| DeviceLinkEditMsg::Save)}
                />
                <Button
                    variant={ButtonVariant::Link}
                    label={t::BACK}
                    onclick={link.callback(|_| DeviceLinkEditMsg::Back)}
                />
            </div>
        </div>
    }
}
