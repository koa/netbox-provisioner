//! Scope: The room one of a resident's devices stands in: the choice of the rooms of its sites,
//! the preview in words, saving; not the row it sits in (`components::resident`).
use super::change_flow::{ask_plan, devices_alert, save, saving};
use crate::{
    components::widgets::popup::Select,
    error::FrontendError,
    graphql::resident::{
        change::{DeviceRoomChange, ResidentChange, ResidentPlan, SaveState, SavedChange},
        residents::{Named, ResidentLocation},
    },
};
use i18n::resident as t;
use patternfly_yew::prelude::{Button, ButtonVariant};
use std::fmt::{self, Display};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DeviceRoomEditProps {
    /// the resident it is changed for (a resident's own, or the one of the support view)
    pub resident: i32,
    pub device: Named,
    /// the room it stands in
    pub location: Option<i32>,
    /// the rooms of the resident's sites
    pub locations: Vec<ResidentLocation>,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

/// A room as the choice shows it: its path, behind its site where the resident has several.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomChoice {
    id: i32,
    name: String,
}

impl Display for RoomChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Where the change stands: from closed over the choice and its preview to saving.
pub enum DeviceRoomEdit {
    Closed,
    Choosing(Option<RoomChoice>),
    Preview {
        room: RoomChoice,
        plan: Option<ResidentPlan>,
    },
    Saving {
        room: RoomChoice,
        saved: Option<SavedChange>,
    },
    Failed {
        room: RoomChoice,
        error: FrontendError,
    },
}

pub enum DeviceRoomEditMsg {
    Open,
    Close,
    Choose(RoomChoice),
    Preview(RoomChoice),
    Plan(Result<ResidentPlan, FrontendError>),
    Back,
    Save,
    Saved(Result<SavedChange, FrontendError>),
}

impl Component for DeviceRoomEdit {
    type Message = DeviceRoomEditMsg;
    type Properties = DeviceRoomEditProps;

    fn create(_ctx: &Context<Self>) -> Self {
        DeviceRoomEdit::Closed
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let props = ctx.props();
        let state = std::mem::replace(self, DeviceRoomEdit::Closed);
        *self = match (state, msg) {
            (_, DeviceRoomEditMsg::Open) => DeviceRoomEdit::Choosing(None),
            (_, DeviceRoomEditMsg::Close) => DeviceRoomEdit::Closed,
            (DeviceRoomEdit::Choosing(_), DeviceRoomEditMsg::Choose(room)) => {
                DeviceRoomEdit::Choosing(Some(room))
            }
            (_, DeviceRoomEditMsg::Preview(room)) => {
                ask_plan(
                    ctx.link(),
                    props.resident,
                    change(props, &room),
                    DeviceRoomEditMsg::Plan,
                );
                DeviceRoomEdit::Preview { room, plan: None }
            }
            (DeviceRoomEdit::Preview { room, .. }, DeviceRoomEditMsg::Plan(plan)) => match plan {
                Ok(plan) => DeviceRoomEdit::Preview {
                    room,
                    plan: Some(plan),
                },
                Err(error) => DeviceRoomEdit::Failed { room, error },
            },
            (
                DeviceRoomEdit::Preview { room, .. } | DeviceRoomEdit::Failed { room, .. },
                DeviceRoomEditMsg::Back,
            ) => DeviceRoomEdit::Choosing(Some(room)),
            (
                DeviceRoomEdit::Preview {
                    room,
                    plan: Some(plan),
                },
                DeviceRoomEditMsg::Save,
            ) => {
                save(
                    ctx.link(),
                    props.resident,
                    change(props, &room),
                    plan.fingerprint,
                    DeviceRoomEditMsg::Saved,
                );
                DeviceRoomEdit::Saving { room, saved: None }
            }
            (DeviceRoomEdit::Saving { room, .. }, DeviceRoomEditMsg::Saved(saved)) => match saved {
                Ok(saved) => {
                    if saved.state == SaveState::Saved {
                        props.on_saved.emit(());
                    }
                    DeviceRoomEdit::Saving {
                        room,
                        saved: Some(saved),
                    }
                }
                Err(error) => DeviceRoomEdit::Failed { room, error },
            },
            (state, _) => state,
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let props = ctx.props();
        match self {
            DeviceRoomEdit::Closed => html! {
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::change_room(&props.device.name)}
                    onclick={link.callback(|_| DeviceRoomEditMsg::Open)}
                />
            },
            DeviceRoomEdit::Choosing(room) => {
                let chosen = room.clone();
                html! {
                    <div class="wlan-edit">
                        <Select<RoomChoice>
                            placeholder={t::CHOOSE_ROOM}
                            selected={room.clone()}
                            entries={choices(props)}
                            onselect={link.callback(DeviceRoomEditMsg::Choose)}
                        />
                        <div class="wlan-edit-actions">
                            <Button
                                variant={ButtonVariant::Primary}
                                label={t::PREVIEW}
                                disabled={room.is_none()}
                                onclick={link.batch_callback(move |_| {
                                    chosen.clone().map(DeviceRoomEditMsg::Preview)
                                })}
                            />
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::DISCARD}
                                onclick={link.callback(|_| DeviceRoomEditMsg::Close)}
                            />
                        </div>
                    </div>
                }
            }
            DeviceRoomEdit::Preview { plan: None, .. } => html!(<p>{t::PLANNING}</p>),
            DeviceRoomEdit::Preview {
                plan: Some(plan), ..
            } => preview(ctx, plan),
            DeviceRoomEdit::Saving { saved, .. } => {
                saving(saved.as_ref(), link.callback(|_| DeviceRoomEditMsg::Close))
            }
            DeviceRoomEdit::Failed { error, .. } => html! {
                <div class="wlan-edit">
                    {error.to_html()}
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::BACK}
                        onclick={link.callback(|_| DeviceRoomEditMsg::Back)}
                    />
                </div>
            },
        }
    }
}

/// The rooms but the one it stands in; the site in front where they are in several.
fn choices(props: &DeviceRoomEditProps) -> Vec<RoomChoice> {
    let several = props
        .locations
        .iter()
        .any(|location| location.site != props.locations[0].site);
    props
        .locations
        .iter()
        .filter(|location| Some(location.id) != props.location)
        .map(|location| {
            let path = t::location_path(&location.path);
            RoomChoice {
                id: location.id,
                name: match (&location.site, several) {
                    (Some(site), true) => format!("{}: {path}", site.name),
                    _ => path,
                },
            }
        })
        .collect()
}

fn change(props: &DeviceRoomEditProps, room: &RoomChoice) -> ResidentChange {
    ResidentChange::DeviceRoom(DeviceRoomChange {
        device: props.device.id,
        location: room.id,
    })
}

/// The plan in words: where the device stands then; NetBox only, as the alert says.
fn preview(ctx: &Context<DeviceRoomEdit>, plan: &ResidentPlan) -> Html {
    let link = ctx.link();
    let lines = plan.rooms.iter().map(|room| {
        let path = |location: &crate::graphql::resident::change::LocationRef| {
            t::location_path(&location.path)
        };
        let to = room.location.as_ref().map(path).unwrap_or_default();
        let before = room.location_before.as_ref().map(path);
        t::room_moved(&room.device.name, &to, before.as_deref())
    });
    html! {
        <div class="wlan-edit">
            <ul class="resident-facts">
                { for lines.map(|line| html!(<li>{line}</li>)) }
            </ul>
            {devices_alert(plan, None)}
            <div class="wlan-edit-actions">
                <Button
                    variant={ButtonVariant::Primary}
                    label={t::SAVE}
                    onclick={link.callback(|_| DeviceRoomEditMsg::Save)}
                />
                <Button
                    variant={ButtonVariant::Link}
                    label={t::BACK}
                    onclick={link.callback(|_| DeviceRoomEditMsg::Back)}
                />
            </div>
        </div>
    }
}
