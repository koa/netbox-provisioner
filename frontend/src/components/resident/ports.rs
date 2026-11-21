//! Scope: The tab „Dosen und Geräte“: a frame per room with a row per port or socket of the
//! resident, its network and what hangs at its cable, and the resident's devices with where
//! they are linked, their Anschluss and room; each changed right there as a draft. Not
//! keeping the drafts (`components::resident`), nor saving them (`save_dialog`).
//!
//! A device stands in the room of the socket its cable comes from, where its card is at the
//! socket's row. In another room (a patch cable through the wall) the socket only points
//! there, and the device has a row of its own in its room; so has a device without link, or
//! one linked to a port not the resident's (its own row of that port then goes).
use super::{
    card_header,
    drafts::{Drafts, LinkDraft, PortKey, current_link, uplink_id},
};
use crate::{
    components::widgets::popup::Select,
    graphql::resident::{
        live::SeenUplink,
        residents::{
            FarEnd, Named, PortKind, Resident, ResidentDevice, ResidentLocation, ResidentPort,
        },
    },
};
use i18n::resident as t;
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Content, Form, FormGroup, FormHelperText, Level, Title,
};
use std::fmt::{self, Display};
use yew::{Callback, Html, html};

/// A draft changed in the tab, or the room dialog asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum PortEdit {
    /// the network of a port row
    Vlan {
        port: PortKey,
        vlan: Option<i32>,
    },
    ResetPort(PortKey),
    /// where a device is linked
    Link {
        device: i32,
        link: LinkDraft,
    },
    /// a device's link as saved
    ResetLink(i32),
    /// a device's link and room as saved
    ResetDevice(i32),
    /// the room dialog of a device
    Room(i32),
}

/// What the tab shows, and where its changes go.
pub struct PortsTab<'a> {
    pub resident: &'a Resident,
    pub drafts: &'a Drafts,
    /// where the routers see the resident's devices plugged in, where it isn't their Anschluss
    pub seen: &'a [SeenUplink],
    pub edit: &'a Callback<PortEdit>,
}

/// A room: a site and the path of locations in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RoomKey<'a> {
    site: Option<&'a str>,
    path: &'a [String],
}

/// A row of a room's frame.
enum Row<'a> {
    Port(&'a ResidentPort),
    /// one of the resident's linked devices in a row of its own, with where its cable comes from
    Device {
        device: &'a ResidentDevice,
        cable: Cable<'a>,
    },
}

/// Where the cable of a device in a row of its own comes from.
enum Cable<'a> {
    /// a socket or port of the resident in another room
    Patch(&'a ResidentPort),
    /// a port not the resident's (the operator's router)
    Operator(&'a FarEnd),
    None,
}

/// A room's frame with its rows.
struct Room<'a> {
    key: RoomKey<'a>,
    rows: Vec<Row<'a>>,
}

/// Where a device is linked, as the choice shows it.
#[derive(Debug, Clone, PartialEq)]
enum Place {
    Port {
        port: PortKey,
        name: String,
    },
    /// as now, a port not the resident's
    Current(String),
    Nowhere,
}

impl Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Port { name, .. } | Place::Current(name) => f.write_str(name),
            Place::Nowhere => f.write_str(t::NOT_CONNECTED),
        }
    }
}

/// A network, a device or a port to choose, by id and name.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    id: Option<i32>,
    name: String,
}

impl Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

impl PortsTab<'_> {
    pub fn view(&self) -> Html {
        let rooms = self.rooms();
        if rooms.is_empty() {
            return html!(<Content><p>{t::NO_PORTS}</p></Content>);
        }
        let mut shown_site = None;
        rooms
            .into_iter()
            .map(|room| {
                // the site once, above its first room
                let site = (shown_site != Some(room.key.site)).then(|| {
                    shown_site = Some(room.key.site);
                    room.key
                        .site
                        .map(|site| html!(<Title level={Level::H2}>{site}</Title>))
                });
                let title = (!room.key.path.is_empty()).then(|| {
                    let path = t::location_path(room.key.path);
                    html! {
                        <div class="pf-v6-c-card__title">
                            <h3 class="pf-v6-c-card__title-text">{path}</h3>
                        </div>
                    }
                });
                // keyed: a device moving to another room moves its rows, the rest keeps its state
                let key = format!("{:?}\0{}", room.key.site, room.key.path.join("\0"));
                html! {
                    <section class="resident-site" {key}>
                        {site.flatten()}
                        <div class="pf-v6-c-card resident-room">
                            {title}
                            <div class="pf-v6-c-card__body resident-rows">
                                { for room.rows.iter().map(|row| self.row(room.key, row)) }
                            </div>
                        </div>
                    </section>
                }
            })
            .collect()
    }

    /// The rooms with their rows: the ports as the backend sorts them (site, location, device,
    /// port), a device in a row of its own in front of its room's ports.
    fn rooms(&self) -> Vec<Room<'_>> {
        let resident = self.resident;
        let mut devices = Vec::new();
        // the rows of devices' own Anschluss: its cable is the device's card's
        let mut instead = Vec::new();
        for device in &resident.devices {
            let uplink = self.uplink_row(device);
            instead.extend(uplink.map(PortKey::of));
            let cable = match self.at(device.id) {
                Some(port) if self.stands_at(device, port) => continue,
                Some(port) => Cable::Patch(port),
                // one the provisioner doesn't link shows only where its cable comes from
                None if !device.routeros => continue,
                None => uplink
                    .and_then(|row| row.far_ends.first())
                    .map_or(Cable::None, Cable::Operator),
            };
            devices.push(Row::Device { device, cable });
        }
        let mut rooms: Vec<Room> = Vec::new();
        for port in &resident.ports {
            if instead.contains(&PortKey::of(port)) {
                continue;
            }
            let key = self.port_room(port);
            match rooms.last_mut() {
                Some(last) if last.key == key => last.rows.push(Row::Port(port)),
                _ => rooms.push(Room {
                    key,
                    rows: vec![Row::Port(port)],
                }),
            }
        }
        for row in devices {
            let Row::Device { device, .. } = &row else {
                continue;
            };
            let key = self.device_room(device);
            match rooms.iter_mut().find(|room| room.key == key) {
                Some(room) => {
                    let first_port = room
                        .rows
                        .iter()
                        .position(|row| matches!(row, Row::Port(_)))
                        .unwrap_or(room.rows.len());
                    room.rows.insert(first_port, row);
                }
                None => rooms.push(Room {
                    key,
                    rows: vec![row],
                }),
            }
        }
        rooms
    }

    fn port_room<'p>(&self, port: &'p ResidentPort) -> RoomKey<'p> {
        RoomKey {
            site: port.site.as_ref().map(|site| site.name.as_str()),
            path: &port.location,
        }
    }

    fn device_room(&self, device: &ResidentDevice) -> RoomKey<'_> {
        match self.location(device.location) {
            Some(location) => RoomKey {
                site: location.site.as_ref().map(|site| site.name.as_str()),
                path: &location.path,
            },
            None => RoomKey {
                site: None,
                path: &[],
            },
        }
    }

    /// Whether the device stands in the room of the port row its cable comes from (or in none).
    fn stands_at(&self, device: &ResidentDevice, port: &ResidentPort) -> bool {
        device.location.is_none() || self.device_room(device) == self.port_room(port)
    }

    fn location(&self, id: Option<i32>) -> Option<&ResidentLocation> {
        let id = id?;
        self.resident
            .locations
            .iter()
            .find(|location| location.id == id)
    }

    /// The resident's port row the cable of its device `device` comes from.
    fn at(&self, device: i32) -> Option<&ResidentPort> {
        self.resident.ports.iter().find(|port| {
            port.far_ends
                .iter()
                .any(|end| end.own_device && end.device.id == device)
        })
    }

    /// The row of the device's own Anschluss, where no row of the resident has its cable.
    fn uplink_row(&self, device: &ResidentDevice) -> Option<&ResidentPort> {
        let uplink = device.uplink.as_ref()?;
        self.resident
            .ports
            .iter()
            .find(|port| port.on_own_device && port.device.id == device.id && &port.name == uplink)
    }

    /// The resident's device at the far end `end`.
    fn own(&self, end: &FarEnd) -> Option<&ResidentDevice> {
        let own = end.own_device.then_some(&self.resident.devices)?;
        own.iter().find(|device| device.id == end.device.id)
    }

    fn row(&self, room: RoomKey, row: &Row) -> Html {
        let (key, left, cable, right) = match row {
            Row::Port(port) => (
                format!("{:?}-{}", port.kind, port.id),
                self.port_card(port),
                !port.far_ends.is_empty(),
                self.far_side(room, port),
            ),
            Row::Device { device, cable } => (
                format!("device-{}", device.id),
                cable_card(cable),
                !matches!(cable, Cable::None),
                self.device_card(device, cable_room(self, cable, device)),
            ),
        };
        let conn = match cable {
            true => html! {
                <div class="sock-conn">
                    <div class="sock-line"></div>
                    <div class="sock-text"><small>{t::CABLE}</small></div>
                </div>
            },
            false => html!(<div class="sock-conn"></div>),
        };
        html! {
            <div class="sock-row" {key}>
                <div class="sock-side">{left}</div>
                {conn}
                <div class="sock-side">{right}</div>
            </div>
        }
    }

    /// A port or socket: its name, its network to choose where nothing but the resident's
    /// devices hang at it, a device to link where nothing does, or its device taken off.
    fn port_card(&self, port: &ResidentPort) -> Html {
        let key = PortKey::of(port);
        let socket = port.kind == PortKind::Socket;
        let sub = match port.kind {
            PortKind::Socket if port.label.is_none() => t::SOCKET.to_string(),
            PortKind::Socket => t::socket_at(&port.device.name, &port.name),
            _ => sub(port),
        };
        // a socket's network is behind the wall, whatever is plugged in: shown with the socket
        let network = (socket && !port.far_ends.is_empty())
            .then(|| html!(<p><small>{self.network_name(port)}</small></p>));
        let linked = port
            .far_ends
            .iter()
            .filter_map(|end| self.own(end).filter(|device| device.routeros))
            .collect::<Vec<_>>();
        let vlan = (linked.is_empty() && port.far_ends.iter().all(|end| end.own_device))
            .then(|| self.vlan_choice(port));
        let connect = port.far_ends.is_empty().then(|| self.connect_choice(port));
        let remove = linked.iter().map(|device| {
            let edit = self.edit.clone();
            let device = device.id;
            html! {
                <div>
                    <Button
                        variant={ButtonVariant::DangerSecondary}
                        label={t::REMOVE_DEVICE}
                        onclick={Callback::from(move |_| edit.emit(PortEdit::Link {
                            device,
                            link: LinkDraft::Disconnect,
                        }))}
                    />
                </div>
            }
        });
        let changed = self.drafts.port_vlans.contains_key(&key);
        let reset = self.edit.reform(move |_| PortEdit::ResetPort(key));
        let heading = html!(<h4 class="pf-v6-c-card__title-text">{title(port)}</h4>);
        html! {
            <div class="pf-v6-c-card pf-m-compact">
                {card_header(heading, changed, reset)}
                <div class="pf-v6-c-card__body resident-edit">
                    <p><small>{sub}</small></p>
                    {network}
                    if vlan.is_some() || connect.is_some() {
                        <Form>{vlan}{connect}</Form>
                    }
                    { for remove }
                </div>
            </div>
        }
    }

    /// The port's network: one of the resident's, or none.
    fn vlan_choice(&self, port: &ResidentPort) -> Html {
        let key = PortKey::of(port);
        let entries = self
            .resident
            .networks
            .iter()
            .map(|network| Choice {
                id: Some(network.id),
                name: network.name.clone(),
            })
            .chain([Choice {
                id: None,
                name: t::NO_NETWORK.into(),
            }])
            .collect::<Vec<_>>();
        // the operator's network is none of the choices
        let now = match self.vlan_of(port) {
            Vlan::Some(vlan) => Some(Some(vlan)),
            Vlan::None => Some(None),
            Vlan::Foreign => None,
        };
        let selected = now.and_then(|now| entries.iter().find(|entry| entry.id == now).cloned());
        let onselect = self.edit.reform(move |choice: Choice| PortEdit::Vlan {
            port: key,
            vlan: choice.id,
        });
        html! {
            <FormGroup label={t::NETWORK}>
                <Select<Choice> placeholder={t::FOREIGN_NETWORK} {selected} {entries} {onselect}/>
            </FormGroup>
        }
    }

    /// A device to link to the free port, and those the drafts link there.
    fn connect_choice(&self, port: &ResidentPort) -> Html {
        let key = PortKey::of(port);
        let here = |device: &ResidentDevice| {
            matches!(
                self.drafts.links.get(&device.id),
                Some(LinkDraft::Connect { port, .. }) if *port == key
            )
        };
        let devices = self
            .resident
            .devices
            .iter()
            .filter(|device| device.routeros)
            .filter(|device| !(port.on_own_device && device.id == port.device.id));
        let entries = devices
            .clone()
            .filter(|device| !here(device))
            .map(|device| Choice {
                id: Some(device.id),
                name: device.name.clone(),
            })
            .collect::<Vec<_>>();
        let coming = devices
            .filter(|device| here(device))
            .map(|device| html!(<p><small>{t::comes_here(&device.name)}</small></p>));
        let onselect = self.edit.reform(move |choice: Choice| PortEdit::Link {
            device: choice.id.unwrap_or_default(),
            link: LinkDraft::Connect {
                port: key,
                device_port: None,
                seen: false,
            },
        });
        html! {
            <>
                if !entries.is_empty() {
                    <FormGroup label={t::OR_CONNECT_DEVICE}>
                        <Select<Choice> placeholder={t::CHOOSE_DEVICE} {entries} {onselect}/>
                    </FormGroup>
                }
                { for coming }
            </>
        }
    }

    /// What is at the port's cable, or its network where nothing is.
    fn far_side(&self, room: RoomKey, port: &ResidentPort) -> Html {
        if port.far_ends.is_empty() {
            let class = match self.vlan_of(port) {
                Vlan::None => "sock-net sock-none",
                Vlan::Some(_) | Vlan::Foreign => "sock-net",
            };
            return html!(<div class={class}>{self.network_name(port)}</div>);
        }
        port.far_ends
            .iter()
            .map(|end| match self.own(end) {
                Some(device) if self.stands_at(device, port) => self.device_card(device, room),
                Some(device) => {
                    let path = t::location_path(self.device_room(device).path);
                    html! {
                        <div class="pf-v6-c-card pf-m-compact">
                            <div class="pf-v6-c-card__title">
                                <h4 class="pf-v6-c-card__title-text">{&device.name}</h4>
                            </div>
                            <div class="pf-v6-c-card__body">{t::see_room(&path)}</div>
                        </div>
                    }
                }
                None => far_card(end),
            })
            .collect()
    }

    /// The port's network with its draft.
    fn vlan_of(&self, port: &ResidentPort) -> Vlan {
        match self.drafts.port_vlans.get(&PortKey::of(port)) {
            Some(Some(vlan)) => Vlan::Some(*vlan),
            Some(None) => Vlan::None,
            None if port.foreign_network => Vlan::Foreign,
            None => match &port.network {
                Some(network) => Vlan::Some(network.id),
                None => Vlan::None,
            },
        }
    }

    fn network_name(&self, port: &ResidentPort) -> String {
        match self.vlan_of(port) {
            Vlan::Some(vlan) => self
                .resident
                .networks
                .iter()
                .find(|network| network.id == vlan)
                .map(|network| network.name.clone())
                .unwrap_or_else(|| t::FOREIGN_NETWORK.into()),
            Vlan::Foreign => t::FOREIGN_NETWORK.into(),
            Vlan::None => t::NO_NETWORK.into(),
        }
    }

    /// One of the resident's devices: its room, where it is linked and out of which of its
    /// ports; `cable_room` is the room its cable comes from.
    fn device_card(&self, device: &ResidentDevice, cable_room: RoomKey) -> Html {
        let id = device.id;
        let draft = self.drafts.links.get(&id).copied();
        let current = current_link(self.resident, device);
        let room = self.drafts.rooms.get(&id).copied().or(device.location);
        let room_key = match self.location(room) {
            Some(location) => RoomKey {
                site: location.site.as_ref().map(|site| site.name.as_str()),
                path: &location.path,
            },
            None => cable_room,
        };
        let room_label = match room_key == cable_room {
            true => t::CHANGE_ROOM.to_string(),
            false => t::in_room(&t::location_path(room_key.path)),
        };
        let room_button = (!self.resident.locations.is_empty()).then(|| {
            html! {
                <Button
                    variant={ButtonVariant::InlineLink}
                    label={room_label}
                    onclick={self.edit.reform(move |_| PortEdit::Room(id))}
                />
            }
        });
        let title = html! {
            <>
                <h4 class="pf-v6-c-card__title-text">{&device.name}</h4>
                {room_button}
            </>
        };
        let changed = draft.is_some() || self.drafts.rooms.contains_key(&id);
        let reset = self.edit.reform(move |_| PortEdit::ResetDevice(id));
        html! {
            <div class="pf-v6-c-card pf-m-compact">
                {card_header(title, changed, reset)}
                <div class="pf-v6-c-card__body resident-edit">
                    {self.link_fields(device, draft, current)}
                </div>
            </div>
        }
    }

    /// Where a device the provisioner links is linked and out of which port, with the port a
    /// router sees its cable in; for any other device the port its cable is in.
    fn link_fields(
        &self,
        device: &ResidentDevice,
        draft: Option<LinkDraft>,
        current: Option<LinkDraft>,
    ) -> Html {
        if !device.routeros {
            let port = self
                .at(device.id)
                .into_iter()
                .flat_map(|port| &port.far_ends)
                .find(|end| end.device.id == device.id)
                .map(|end| t::far_port(&end.port));
            return html! {
                <p><small>{port}<br/>{t::YOUR_DEVICE}</small></p>
            };
        }
        let link = draft.or(current);
        let operator = match (current, self.uplink_row(device)) {
            (None, Some(row)) => row
                .far_ends
                .first()
                .map(|end| t::socket_name(&end.device.name, &end.port)),
            _ => None,
        };
        html! {
            <>
                <Form>
                    {self.place_choice(device, link, operator)}
                    {self.uplink_choice(device, link)}
                </Form>
                {self.seen_alert(device, draft.is_none().then_some(current).flatten())}
            </>
        }
    }

    /// Where the device is linked: a free port row of the resident, as now, or nowhere.
    fn place_choice(
        &self,
        device: &ResidentDevice,
        link: Option<LinkDraft>,
        operator: Option<String>,
    ) -> Html {
        let id = device.id;
        let ports = self
            .resident
            .ports
            .iter()
            .filter(|port| !(port.on_own_device && port.device.id == id))
            .filter(|port| {
                port.far_ends.is_empty() || port.far_ends.iter().any(|end| end.device.id == id)
            })
            .map(|port| Place::Port {
                port: PortKey::of(port),
                name: match port.kind {
                    PortKind::Socket => title(port),
                    _ => sub(port),
                },
            });
        let entries = operator
            .clone()
            .map(Place::Current)
            .into_iter()
            .chain(ports)
            .chain([Place::Nowhere])
            .collect::<Vec<_>>();
        let selected = match link {
            Some(LinkDraft::Connect { port, .. }) => entries
                .iter()
                .find(|entry| matches!(entry, Place::Port { port: at, .. } if *at == port))
                .cloned(),
            Some(LinkDraft::Disconnect) => Some(Place::Nowhere),
            None => operator.map(Place::Current),
        };
        let onselect = self.edit.reform(move |place: Place| match place {
            Place::Port { port, .. } => PortEdit::Link {
                device: id,
                link: LinkDraft::Connect {
                    port,
                    device_port: None,
                    seen: false,
                },
            },
            Place::Current(_) => PortEdit::ResetLink(id),
            Place::Nowhere => PortEdit::Link {
                device: id,
                link: LinkDraft::Disconnect,
            },
        });
        html! {
            <FormGroup label={t::CONNECTED_TO}>
                <Select<Place> {selected} {entries} {onselect}/>
            </FormGroup>
        }
    }

    /// The device's port its cable goes into, where it is linked to a port row of the
    /// resident; as text where it hangs at the operator's port.
    fn uplink_choice(&self, device: &ResidentDevice, link: Option<LinkDraft>) -> Html {
        let helper = FormHelperText::from(t::UPLINK_HINT);
        let (port, device_port) = match link {
            Some(LinkDraft::Connect {
                port, device_port, ..
            }) => (port, device_port.or(uplink_id(device))),
            Some(LinkDraft::Disconnect) => return html!(),
            None => {
                return match &device.uplink {
                    Some(uplink) => html! {
                        <FormGroup label={t::UPLINK} helper_text={helper}>
                            <p>{uplink}</p>
                        </FormGroup>
                    },
                    None => html!(),
                };
            }
        };
        if device.ports.is_empty() {
            return html!();
        }
        let id = device.id;
        let entries = device
            .ports
            .iter()
            .map(|port| Choice {
                id: Some(port.id),
                name: port.name.clone(),
            })
            .collect::<Vec<_>>();
        let selected = entries
            .iter()
            .find(|entry| entry.id.is_some() && entry.id == device_port)
            .cloned();
        let onselect = self.edit.reform(move |choice: Choice| PortEdit::Link {
            device: id,
            link: LinkDraft::Connect {
                port,
                device_port: choice.id,
                seen: false,
            },
        });
        html! {
            <FormGroup label={t::UPLINK} helper_text={helper}>
                <Select<Choice> placeholder={t::CHOOSE_PORT} {selected} {entries} {onselect}/>
            </FormGroup>
        }
    }

    /// The port a router sees the device's cable in, where it isn't its Anschluss, to take
    /// over; only for a device at a port row of the resident (`current`) without a draft.
    fn seen_alert(&self, device: &ResidentDevice, current: Option<LinkDraft>) -> Option<Html> {
        let Some(LinkDraft::Connect { port, .. }) = current else {
            return None;
        };
        let seen = self.seen.iter().find(|seen| seen.device == device.id)?;
        let uplink = device.uplink.as_deref().unwrap_or_default();
        let id = device.id;
        let seen_port: &Named = &seen.port;
        let take = seen_port.id;
        Some(html! {
            <Alert
                inline=true
                r#type={AlertType::Warning}
                title={t::uplink_misplugged(&seen_port.name, uplink)}
            >
                <p>{t::uplink_misplugged_hint(&seen_port.name, uplink)}</p>
                <Button
                    variant={ButtonVariant::InlineLink}
                    label={t::take_uplink(&seen_port.name)}
                    onclick={self.edit.reform(move |_| PortEdit::Link {
                        device: id,
                        link: LinkDraft::Connect {
                            port,
                            device_port: Some(take),
                            seen: true,
                        },
                    })}
                />
            </Alert>
        })
    }
}

/// A port row's network with its draft.
enum Vlan {
    Some(i32),
    /// the operator's
    Foreign,
    None,
}

/// The room the cable of a device in a row of its own comes from: the socket's, else its own.
fn cable_room<'a>(tab: &'a PortsTab, cable: &Cable<'a>, device: &'a ResidentDevice) -> RoomKey<'a> {
    match cable {
        Cable::Patch(port) => tab.port_room(port),
        Cable::Operator(_) | Cable::None => tab.device_room(device),
    }
}

/// A port row's name: its label, a socket by its wall and place, or the port's name.
fn title(port: &ResidentPort) -> String {
    match &port.label {
        Some(label) => label.clone(),
        // a socket's device is the wall it is in, named after the room
        None if port.kind == PortKind::Socket => t::socket_name(&port.device.name, &port.name),
        None => port.name.clone(),
    }
}

/// A port by itself and its device: „Port ether3 an lab-np2“, „Port ether2 von lab-wap“.
fn sub(port: &ResidentPort) -> String {
    match port.on_own_device {
        true => t::own_port(&port.name, &port.device.name),
        false => t::port_of(&port.name, &port.device.name),
    }
}

/// The socket or port in another room a device's cable comes from: a patch cable from a socket.
fn patched_from(port: &ResidentPort) -> String {
    match port.kind {
        PortKind::Socket => t::patched_from(&title(port)),
        _ => t::cable_from(&sub(port)),
    }
}

/// Where the cable of a device in a row of its own comes from, on the row's left.
fn cable_card(cable: &Cable) -> Html {
    match cable {
        Cable::Patch(port) => html! {
            <div class="pf-v6-c-card pf-m-compact">
                <div class="pf-v6-c-card__body">
                    <strong>{patched_from(port)}</strong>
                    <p><small>{t::PATCHED_HINT}</small></p>
                </div>
            </div>
        },
        Cable::Operator(end) => html! {
            <div class="pf-v6-c-card pf-m-compact">
                <div class="pf-v6-c-card__body">
                    <strong>{t::socket_name(&end.device.name, &end.port)}</strong>
                    <p><small>{t::OPERATOR_PORT}</small></p>
                </div>
            </div>
        },
        Cable::None => html! {
            <div class="pf-v6-c-card pf-m-compact pf-m-plain sock-none">
                <div class="pf-v6-c-card__body">
                    <strong>{t::NOT_CONNECTED}</strong>
                    <p><small>{t::NOT_CONNECTED_HINT}</small></p>
                </div>
            </div>
        },
    }
}

/// A device at the cable the provisioner doesn't link: another's, or one without RouterOS.
fn far_card(end: &FarEnd) -> Html {
    let own = end
        .own_device
        .then(|| html!(<><br/><small>{t::YOUR_DEVICE}</small></>));
    html! {
        <div class="pf-v6-c-card pf-m-compact">
            <div class="pf-v6-c-card__title">
                <h4 class="pf-v6-c-card__title-text">{&end.device.name}</h4>
            </div>
            <div class="pf-v6-c-card__body"><small>{t::far_port(&end.port)}</small>{own}</div>
        </div>
    }
}
