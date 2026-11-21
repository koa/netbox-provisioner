//! Scope: The dialog choosing the room one of a resident's devices stands in, over the page;
//! not the draft it makes (`components::resident`).
use crate::{components::widgets::popup::Select, graphql::resident::residents::ResidentLocation};
use i18n::resident as t;
use patternfly_yew::prelude::{
    Bullseye, Button, ButtonVariant, Form, FormGroup, Modal, ModalVariant,
};
use std::fmt::{self, Display};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct RoomDialogProps {
    pub device: String,
    /// the room it stands in, with its draft
    pub location: Option<i32>,
    /// the rooms of the resident's sites
    pub locations: Vec<ResidentLocation>,
    pub on_apply: Callback<i32>,
    pub on_close: Callback<()>,
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

/// The room chosen so far.
pub struct RoomDialog {
    chosen: Option<RoomChoice>,
}

impl Component for RoomDialog {
    type Message = RoomChoice;
    type Properties = RoomDialogProps;

    fn create(ctx: &Context<Self>) -> Self {
        let location = ctx.props().location;
        RoomDialog {
            chosen: choices(ctx.props())
                .into_iter()
                .find(|choice| Some(choice.id) == location),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, chosen: Self::Message) -> bool {
        self.chosen = Some(chosen);
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let apply = {
            let on_apply = props.on_apply.clone();
            let chosen = self.chosen.as_ref().map(|chosen| chosen.id);
            Callback::from(move |_| {
                if let Some(chosen) = chosen {
                    on_apply.emit(chosen);
                }
            })
        };
        let footer = html! {
            <>
                <Button
                    variant={ButtonVariant::Primary}
                    label={t::APPLY}
                    disabled={self.chosen.as_ref().map(|chosen| chosen.id) == props.location}
                    onclick={apply}
                />
                <Button
                    variant={ButtonVariant::Link}
                    label={t::CANCEL}
                    onclick={props.on_close.reform(|_| ())}
                />
            </>
        };
        html! {
            <div class="pf-v6-c-backdrop resident-room">
                <Bullseye>
                    <Modal
                        title={t::room_title(&props.device)}
                        description={t::ROOM_HINT}
                        variant={ModalVariant::Small}
                        onclose={props.on_close.clone()}
                        {footer}
                    >
                        <Form>
                            <FormGroup label={t::ROOM}>
                                <Select<RoomChoice>
                                    selected={self.chosen.clone()}
                                    entries={choices(props)}
                                    onselect={ctx.link().callback(|chosen| chosen)}
                                />
                            </FormGroup>
                        </Form>
                    </Modal>
                </Bullseye>
            </div>
        }
    }
}

/// The rooms, behind their site where the resident has rooms in several.
fn choices(props: &RoomDialogProps) -> Vec<RoomChoice> {
    let first_site = props.locations.first().and_then(|room| room.site.as_ref());
    let sites = props
        .locations
        .iter()
        .any(|room| room.site.as_ref() != first_site);
    props
        .locations
        .iter()
        .map(|room| {
            let site = room.site.iter().filter(|_| sites);
            let path = site
                .map(|site| site.name.clone())
                .chain(room.path.iter().cloned())
                .collect::<Vec<_>>();
            RoomChoice {
                id: room.id,
                name: t::location_path(&path),
            }
        })
        .collect()
}
