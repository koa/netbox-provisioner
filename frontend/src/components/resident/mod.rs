//! Scope: What a resident sees of its network, in tabs: its sockets and devices, its networks;
//! its drafts, collected over both tabs until they are previewed and saved together. Not
//! loading it (`pages::residents`, `app`), nor the admins' choice of a resident.
use crate::{
    error::FrontendError,
    graphql::{
        request,
        resident::{
            changes::{ChangeState, ChangesQuery, ChangesVariables, UndoStep},
            live::{
                LiveState, LiveStateQuery, LiveStateVariables, PortDevicesQuery,
                PortDevicesVariables, PortHosts, SeenUplink, SeenUplinksQuery,
                SeenUplinksVariables,
            },
            residents::Resident,
        },
    },
};
use cynic::QueryBuilder;
use drafts::{Drafts, LinkDraft, PortKey, current_link};
use gloo::timers::callback::Interval;
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

mod changes;
mod drafts;
mod network_device;
mod network_devices;
mod networks;
mod port_name;
mod ports;
mod qr_dialog;
mod room_dialog;
mod save_dialog;
mod status;

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
    Changes,
}

/// How often the view reads the saved changes and what is up again, in ms.
const EVERY: u32 = 15_000;

/// A resident's sockets and devices, and its networks, with its drafts; where the routers see
/// its devices plugged in and the devices behind its ports, read once the view is shown and
/// again with each loaded resident; its saved changes and what is up, for the labels, read
/// again every 15 s.
pub struct ResidentView {
    view: View,
    /// the devices whose cable is in another port than their Anschluss; empty while read or
    /// where the routers didn't answer (the view offers nothing then)
    seen: Vec<SeenUplink>,
    /// the devices behind its ports; empty while read or where nothing answered
    behind: Vec<PortHosts>,
    drafts: Drafts,
    /// the device whose room is being chosen
    room_of: Option<i32>,
    /// the drafts' preview is open
    previewing: bool,
    /// a change taken back found everything as before it already
    as_before: bool,
    /// the saved changes, the newest first; none while read
    changes: Option<Vec<ChangeState>>,
    /// reading them failed the last time: why
    changes_failed: Option<Html>,
    /// what is up; none while read or where it couldn't be
    live: Option<LiveState>,
    _every: Interval,
}

pub enum ResidentViewMsg {
    Show(View),
    Seen(Vec<SeenUplink>),
    Behind(Vec<PortHosts>),
    /// read the changes and what is up again
    ReadState,
    Changes(Result<Vec<ChangeState>, FrontendError>),
    Live(Option<LiveState>),
    Port(PortEdit),
    Wlan(WlanEdit),
    Room(i32),
    CloseRoom,
    DiscardAll,
    Preview(bool),
    Saved,
    /// a saved change taken back: its parts become drafts, the preview opens
    Undo(Vec<UndoStep>),
}

impl Component for ResidentView {
    type Message = ResidentViewMsg;
    type Properties = ResidentViewProps;

    fn create(ctx: &Context<Self>) -> Self {
        read_seen(ctx);
        read_state(ctx);
        let link = ctx.link().clone();
        ResidentView {
            _every: Interval::new(EVERY, move || link.send_message(ResidentViewMsg::ReadState)),
            changes: None,
            changes_failed: None,
            live: None,
            view: View::Ports,
            seen: Vec::new(),
            behind: Vec::new(),
            drafts: Drafts::default(),
            room_of: None,
            previewing: false,
            as_before: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let resident = &ctx.props().resident;
        self.as_before = false;
        let drafts = &mut self.drafts;
        match msg {
            ResidentViewMsg::Show(view) => self.view = view,
            ResidentViewMsg::Seen(seen) => self.seen = seen,
            ResidentViewMsg::Behind(behind) => self.behind = behind,
            ResidentViewMsg::ReadState => {
                read_state(ctx);
                return false;
            }
            ResidentViewMsg::Changes(Ok(changes)) => {
                self.changes = Some(changes);
                self.changes_failed = None;
            }
            ResidentViewMsg::Changes(Err(error)) => self.changes_failed = Some(error.to_html()),
            ResidentViewMsg::Live(live) => self.live = live,
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
            ResidentViewMsg::Port(PortEdit::ResetPort(port)) => drafts.reset_port(port),
            ResidentViewMsg::Port(PortEdit::Link { device, link }) => {
                let now = resident
                    .devices
                    .iter()
                    .find(|own| own.id == device)
                    .and_then(|own| current_link(resident, own));
                drafts.link(device, link, now);
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
                read_state(ctx);
            }
            ResidentViewMsg::Undo(steps) => {
                let before = self.drafts.clone();
                for step in steps {
                    self.undo_step(ctx, step);
                }
                self.previewing = !self.drafts.is_empty();
                self.as_before = self.drafts == before;
            }
        }
        true
    }

    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        // a new callback each time the page renders is no reason to ask the routers
        if old.resident != ctx.props().resident {
            read_seen(ctx);
            read_state(ctx);
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let props = ctx.props();
        let resident = &props.resident;
        let onselect = link.callback(ResidentViewMsg::Show);
        let state = status::State {
            changes: self.changes.as_deref().unwrap_or_default(),
            live: self.live.as_ref(),
        };
        let ports = PortsTab {
            resident,
            drafts: &self.drafts,
            seen: &self.seen,
            behind: &self.behind,
            state,
            edit: &link.callback(ResidentViewMsg::Port),
        };
        let networks = networks::networks(
            resident,
            &self.drafts,
            state,
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
        let as_before = self.as_before.then(|| {
            html! {
                <Alert inline=true r#type={AlertType::Info} title={t::UNDO_AS_BEFORE}/>
            }
        });
        html! {
            <div class="resident-view">
                {bar}
                {as_before}
                <div class="resident-summary">{state.summary(resident, &self.seen)}</div>
                <Tabs<View> selected={self.view} {onselect}>
                    <Tab<View> index={View::Ports} title={self.tab_title(View::Ports)}>
                        {ports.view()}
                    </Tab<View>>
                    <Tab<View> index={View::Networks} title={self.tab_title(View::Networks)}>
                        {networks}
                    </Tab<View>>
                    <Tab<View> index={View::Changes} title={self.tab_title(View::Changes)}>
                        <changes::ChangesTab resident={resident.id}
                            changes={self.changes.clone()}
                            failed={self.changes_failed.clone()}
                            on_read_again={link.callback(|()| ResidentViewMsg::ReadState)}
                            on_undo={link.callback(ResidentViewMsg::Undo)}/>
                    </Tab<View>>
                </Tabs<View>>
                {room}
                {preview}
            </div>
        }
    }
}

impl ResidentView {
    /// Makes a draft of a part that takes a change back, as an edit by hand would; one as it is
    /// now is none.
    fn undo_step(&mut self, ctx: &Context<Self>, step: UndoStep) {
        let msg = match step {
            UndoStep::WlanUndo(undo) => {
                let Some(now) = ctx
                    .props()
                    .resident
                    .networks
                    .iter()
                    .flat_map(|network| network.wlans.iter())
                    .find(|wlan| wlan.id == undo.wlan)
                else {
                    return;
                };
                let mut draft = self
                    .drafts
                    .wlans
                    .get(&undo.wlan)
                    .cloned()
                    .unwrap_or_default();
                if let Some(ssid) = undo.ssid {
                    draft.ssid = (ssid != now.ssid).then_some(ssid);
                }
                if let Some(psk) = undo.psk {
                    draft.psk = (Some(&psk) != now.psk.as_ref()).then_some(psk);
                }
                if let Some(enabled) = undo.enabled {
                    draft.enabled = (enabled != now.enabled).then_some(enabled);
                }
                ResidentViewMsg::Wlan(WlanEdit {
                    wlan: undo.wlan,
                    draft,
                })
            }
            UndoStep::PortVlanUndo(undo) => ResidentViewMsg::Port(PortEdit::Vlan {
                port: PortKey {
                    kind: undo.kind,
                    id: undo.port,
                },
                vlan: undo.vlan,
            }),
            UndoStep::ConnectDeviceUndo(undo) => ResidentViewMsg::Port(PortEdit::Link {
                device: undo.device,
                link: LinkDraft::Connect {
                    port: PortKey {
                        kind: undo.kind,
                        id: undo.port,
                    },
                    device_port: undo.device_port,
                    seen: false,
                },
            }),
            UndoStep::DisconnectDeviceUndo(undo) => ResidentViewMsg::Port(PortEdit::Link {
                device: undo.device,
                link: LinkDraft::Disconnect,
            }),
            UndoStep::DeviceRoomUndo(undo) => {
                self.room_of = Some(undo.device);
                ResidentViewMsg::Room(undo.location)
            }
            // a part of a newer server: the preview shows what the others give
            UndoStep::Unknown => return,
        };
        self.update(ctx, msg);
    }
    /// The title of the tab of `view`, with the number of its drafts where it has some.
    fn tab_title(&self, view: View) -> Html {
        let title = match view {
            View::Ports => t::TAB_PORTS,
            View::Networks => t::TAB_NETWORKS,
            View::Changes => t::TAB_CHANGES,
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

/// Asks where the routers see the resident's devices plugged in, and which devices are behind
/// its ports.
fn read_seen(ctx: &Context<ResidentView>) {
    let resident = ctx.props().resident.id;
    let scope = ctx.link().clone();
    spawn_local(async move {
        let variables = SeenUplinksVariables { resident };
        let seen = request(scope.clone(), SeenUplinksQuery::build(variables))
            .await
            .map(|data| data.seen_uplinks.devices)
            .unwrap_or_default();
        scope.send_message(ResidentViewMsg::Seen(seen));
    });
    let scope = ctx.link().clone();
    spawn_local(async move {
        let variables = PortDevicesVariables { resident };
        let behind = request(scope.clone(), PortDevicesQuery::build(variables))
            .await
            .map(|data| data.port_devices.ports)
            .unwrap_or_default();
        scope.send_message(ResidentViewMsg::Behind(behind));
    });
}

/// Reads the saved changes and what is up.
fn read_state(ctx: &Context<ResidentView>) {
    let resident = ctx.props().resident.id;
    let scope = ctx.link().clone();
    spawn_local(async move {
        let variables = ChangesVariables { resident };
        let changes = request(scope.clone(), ChangesQuery::build(variables))
            .await
            .map(|data| data.changes);
        scope.send_message(ResidentViewMsg::Changes(changes));
    });
    let scope = ctx.link().clone();
    spawn_local(async move {
        let variables = LiveStateVariables { resident };
        let live = request(scope.clone(), LiveStateQuery::build(variables))
            .await
            .map(|data| data.live_state)
            .ok();
        scope.send_message(ResidentViewMsg::Live(live));
    });
}

/// A card's header: its title, and its one label: where it has a draft the one saying so and
/// resetting it, else its `status`.
fn card_header(
    title: Html,
    changed: bool,
    reset: Callback<MouseEvent>,
    status: Option<&status::Status>,
) -> Html {
    let actions = match changed {
        true => Some(html! {
            <div class="pf-v6-c-card__actions">
                <Button variant={ButtonVariant::InlineLink} label={t::RESET} onclick={reset}/>
                <Label label={t::UNSAVED} color={Color::Orange} compact=true/>
            </div>
        }),
        false => {
            status.map(|status| html!(<div class="pf-v6-c-card__actions">{status.label()}</div>))
        }
    };
    html! {
        <div class="pf-v6-c-card__header">
            {actions}
            <div class="pf-v6-c-card__header-main">
                <div class="pf-v6-c-card__title resident-card-title">{title}</div>
            </div>
        </div>
    }
}
