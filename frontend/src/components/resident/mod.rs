//! Scope: What a resident sees of its network, in tabs: its sockets and devices, its networks;
//! its drafts, collected over both tabs until they are previewed and saved together. Not
//! loading it (`pages::residents`, `app`), nor the admins' choice of a resident.
use crate::graphql::{
    request_resident,
    resident::{
        live::{SeenUplink, SeenUplinksQuery, SeenUplinksVariables},
        residents::Resident,
    },
};
use cynic::QueryBuilder;
use drafts::{Drafts, LinkDraft, PortKey, current_link};
use i18n::resident as t;
use networks::WlanEdit;
use patternfly_yew::prelude::{
    Alert, AlertType, Badge, Button, ButtonVariant, Color, Label, Tab, Tabs,
};
use ports::{PortEdit, PortsTab};
use room_dialog::RoomDialog;
use save_dialog::SaveDialog;
use yew::{
    Callback, Component, Context, Html, MouseEvent, Properties, html, platform::spawn_local,
};

mod drafts;
mod network_devices;
mod networks;
mod ports;
mod room_dialog;
mod save_dialog;

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ResidentViewProps {
    pub resident: Resident,
    /// a change is saved: the page loads the resident again
    pub on_saved: Callback<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Ports,
    Networks,
}

/// A resident's sockets and devices, and its networks, with its drafts; where the routers see
/// its devices plugged in, read once the view is shown and again with each loaded resident.
pub struct ResidentView {
    view: View,
    /// the devices whose cable is in another port than their Anschluss; empty while read or
    /// where the routers didn't answer (the view offers nothing then)
    seen: Vec<SeenUplink>,
    drafts: Drafts,
    /// the device whose room is being chosen
    room_of: Option<i32>,
    /// the drafts' preview is open
    previewing: bool,
}

pub enum ResidentViewMsg {
    Show(View),
    Seen(Vec<SeenUplink>),
    Port(PortEdit),
    Wlan(WlanEdit),
    Room(i32),
    CloseRoom,
    DiscardAll,
    Preview(bool),
    Saved,
}

impl Component for ResidentView {
    type Message = ResidentViewMsg;
    type Properties = ResidentViewProps;

    fn create(ctx: &Context<Self>) -> Self {
        read_seen(ctx);
        ResidentView {
            view: View::Ports,
            seen: Vec::new(),
            drafts: Drafts::default(),
            room_of: None,
            previewing: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let resident = &ctx.props().resident;
        let drafts = &mut self.drafts;
        match msg {
            ResidentViewMsg::Show(view) => self.view = view,
            ResidentViewMsg::Seen(seen) => self.seen = seen,
            ResidentViewMsg::Port(PortEdit::Vlan { port, vlan }) => {
                let now = resident
                    .ports
                    .iter()
                    .find(|row| PortKey::of(row) == port)
                    .filter(|row| !row.foreign_network)
                    .map(|row| row.network.as_ref().map(|network| network.id));
                match now == Some(vlan) {
                    true => drafts.port_vlans.remove(&port),
                    false => drafts.port_vlans.insert(port, vlan),
                };
            }
            ResidentViewMsg::Port(PortEdit::ResetPort(port)) => {
                drafts.port_vlans.remove(&port);
            }
            ResidentViewMsg::Port(PortEdit::Link { device, link }) => {
                let now = resident
                    .devices
                    .iter()
                    .find(|own| own.id == device)
                    .and_then(|own| current_link(resident, own));
                match now.is_some_and(|now| link.is(&now)) {
                    true => drafts.links.remove(&device),
                    false => drafts.links.insert(device, link),
                };
                // a device linked to a port takes its network off: its draft would be undone
                if let LinkDraft::Connect { port, .. } = link {
                    drafts.port_vlans.remove(&port);
                }
            }
            ResidentViewMsg::Port(PortEdit::ResetLink(device)) => {
                drafts.links.remove(&device);
            }
            ResidentViewMsg::Port(PortEdit::ResetDevice(device)) => {
                drafts.links.remove(&device);
                drafts.rooms.remove(&device);
            }
            ResidentViewMsg::Port(PortEdit::Room(device)) => self.room_of = Some(device),
            ResidentViewMsg::Room(location) => {
                if let Some(device) = self.room_of.take() {
                    let now = resident
                        .devices
                        .iter()
                        .find(|own| own.id == device)
                        .and_then(|own| own.location);
                    match now == Some(location) {
                        true => drafts.rooms.remove(&device),
                        false => drafts.rooms.insert(device, location),
                    };
                }
            }
            ResidentViewMsg::CloseRoom => self.room_of = None,
            ResidentViewMsg::Wlan(WlanEdit { wlan, draft }) => {
                match draft.is_empty() {
                    true => drafts.wlans.remove(&wlan),
                    false => drafts.wlans.insert(wlan, draft),
                };
            }
            ResidentViewMsg::DiscardAll => *drafts = Drafts::default(),
            ResidentViewMsg::Preview(open) => self.previewing = open,
            // in NetBox now: the resident loaded again shows them
            ResidentViewMsg::Saved => {
                *drafts = Drafts::default();
                ctx.props().on_saved.emit(());
            }
        }
        true
    }

    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        // a new callback each time the page renders is no reason to ask the routers
        if old.resident != ctx.props().resident {
            read_seen(ctx);
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let props = ctx.props();
        let resident = &props.resident;
        let onselect = link.callback(ResidentViewMsg::Show);
        let ports = PortsTab {
            resident,
            drafts: &self.drafts,
            seen: &self.seen,
            edit: &link.callback(ResidentViewMsg::Port),
        };
        let networks = networks::networks(
            resident,
            &self.drafts,
            &link.callback(ResidentViewMsg::Wlan),
        );
        let bar = (!self.drafts.is_empty()).then(|| {
            html! {
                <div class="resident-drafts">
                    <Alert
                        inline=true
                        r#type={AlertType::Warning}
                        title={t::unsaved_changes(self.drafts.len())}
                    >
                        <p>{t::UNSAVED_HINT}</p>
                        <div class="resident-edit-actions">
                            <Button
                                variant={ButtonVariant::Primary}
                                label={t::PREVIEW_AND_SAVE}
                                onclick={link.callback(|_| ResidentViewMsg::Preview(true))}
                            />
                            <Button
                                variant={ButtonVariant::Link}
                                label={t::DISCARD_ALL}
                                onclick={link.callback(|_| ResidentViewMsg::DiscardAll)}
                            />
                        </div>
                    </Alert>
                </div>
            }
        });
        let room = self
            .room_of
            .and_then(|id| resident.devices.iter().find(|device| device.id == id))
            .map(|device| {
                let location = self
                    .drafts
                    .rooms
                    .get(&device.id)
                    .copied()
                    .or(device.location);
                html! {
                    <RoomDialog
                        device={device.name.clone()}
                        {location}
                        locations={resident.locations.clone()}
                        on_apply={link.callback(ResidentViewMsg::Room)}
                        on_close={link.callback(|()| ResidentViewMsg::CloseRoom)}
                    />
                }
            });
        let preview = self.previewing.then(|| {
            html! {
                <SaveDialog
                    resident={resident.clone()}
                    drafts={self.drafts.clone()}
                    on_close={link.callback(|()| ResidentViewMsg::Preview(false))}
                    on_saved={link.callback(|()| ResidentViewMsg::Saved)}
                />
            }
        });
        html! {
            <div class="resident-view">
                {bar}
                <Tabs<View> selected={self.view} {onselect}>
                    <Tab<View> index={View::Ports} title={self.tab_title(View::Ports)}>
                        {ports.view()}
                    </Tab<View>>
                    <Tab<View> index={View::Networks} title={self.tab_title(View::Networks)}>
                        {networks}
                    </Tab<View>>
                </Tabs<View>>
                {room}
                {preview}
            </div>
        }
    }
}

impl ResidentView {
    /// The title of the tab of `view`, with the number of its drafts where it has some.
    fn tab_title(&self, view: View) -> Html {
        let title = match view {
            View::Ports => t::TAB_PORTS,
            View::Networks => t::TAB_NETWORKS,
        };
        let badge = match self.drafts.len_in(view) {
            0 => None,
            count => Some(html! {
                <Badge read=false screen_reader_text={t::unsaved_changes(count)}>
                    {count}
                </Badge>
            }),
        };
        html! { <>{title}{" "}{badge}</> }
    }
}

/// Asks where the routers see the resident's devices plugged in.
fn read_seen(ctx: &Context<ResidentView>) {
    let scope = ctx.link().clone();
    let variables = SeenUplinksVariables {
        resident: ctx.props().resident.id,
    };
    spawn_local(async move {
        let seen = request_resident(scope.clone(), SeenUplinksQuery::build(variables))
            .await
            .map(|data| data.seen_uplinks.devices)
            .unwrap_or_default();
        scope.send_message(ResidentViewMsg::Seen(seen));
    });
}

/// A card's header: its title, and where it has a draft the label saying so and resetting it.
fn card_header(title: Html, changed: bool, reset: Callback<MouseEvent>) -> Html {
    let actions = changed.then(|| {
        html! {
            <div class="pf-v6-c-card__actions">
                <Button variant={ButtonVariant::InlineLink} label={t::RESET} onclick={reset}/>
                <Label label={t::UNSAVED} color={Color::Orange} compact=true/>
            </div>
        }
    });
    html! {
        <div class="pf-v6-c-card__header">
            {actions}
            <div class="pf-v6-c-card__header-main">
                <div class="pf-v6-c-card__title resident-card-title">{title}</div>
            </div>
        </div>
    }
}
