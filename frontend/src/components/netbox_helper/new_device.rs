//! „Neues Gerät“: name, device type, role, its port and the WLAN group of an access point
//! (`newDeviceOptions`). The device type is preselected when one's model or part number is the
//! board the router sees at the port. The texts are planned when left or confirmed with Enter,
//! not on each key: a plan asks NetBox and a router. Once written, a link to the new device,
//! whose page has its initial setup.
use super::{Choice, NetEntry, NetboxHelper, NetboxHelperMsg, t};
use crate::{
    components::menu::popup::Select,
    error::FrontendError,
    graphql::{
        authenticated::{NewDeviceDevices, NewDeviceOptions, new_device_devices, new_device_options},
        query_authenticated,
    },
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{Form, FormGroup, Spinner, SpinnerSize, TextInput};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};
use yew_nested_router::components::Link;

pub enum NewDeviceMsg {
    Options(Result<new_device_options::ResponseData, FrontendError>),
    Name(String),
    FarPort(String),
    /// a text left or confirmed: planned
    Commit,
    DeviceType(Choice),
    Role(Choice),
    WlanGroup(Option<u32>),
    /// the device written: its site and id, for the link
    Written(Option<(u32, u32)>),
}

/// What the router sees at the port: a MikroTik NetBox doesn't know.
#[derive(Debug, Clone, PartialEq)]
pub struct SeenPreset {
    pub identity: String,
    pub board: String,
    pub interface_name: String,
}

/// What „Neues Gerät“ is choosing; the texts as typed and as planned.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewDeviceChoice {
    pub(super) port: Option<u32>,
    pub(super) seen: Option<SeenPreset>,
    pub(super) name: String,
    pub(super) far_port: String,
    pub(super) planned_name: String,
    pub(super) planned_far_port: String,
    pub(super) device_type: Option<u32>,
    pub(super) role: Option<u32>,
    pub(super) wlan_group: Option<u32>,
    /// the written device's site and id
    pub(super) written: Option<(u32, u32)>,
}

impl NewDeviceChoice {
    pub fn new(port: u32, seen: Option<SeenPreset>) -> Self {
        let far_port = seen
            .as_ref()
            .map(|seen| seen.interface_name.clone())
            .unwrap_or_else(|| "ether1".into());
        Self {
            port: Some(port),
            seen,
            far_port: far_port.clone(),
            planned_far_port: far_port,
            ..Self::default()
        }
    }
}

impl NetboxHelper {
    pub(super) fn update_new_device(&mut self, ctx: &Context<Self>, msg: NewDeviceMsg) -> bool {
        let choice = &mut self.new_device;
        match msg {
            NewDeviceMsg::Options(options) => {
                if let Ok(options) = &options {
                    let options = &options.new_device_options;
                    // the board seen, as model or part number
                    let board = choice.seen.as_ref().map(|seen| seen.board.to_lowercase());
                    choice.device_type = choice.device_type.or_else(|| {
                        options
                            .device_types
                            .iter()
                            .find(|t| {
                                board.as_ref().is_some_and(|board| {
                                    t.model.to_lowercase() == *board
                                        || t.part_number.as_ref().is_some_and(|p| p.to_lowercase() == *board)
                                })
                            })
                            .map(|t| t.id as u32)
                    });
                    choice.role = choice.role.or_else(|| {
                        options
                            .roles
                            .iter()
                            .find(|r| r.slug == "router")
                            .map(|r| r.id as u32)
                    });
                }
                self.new_device_options = Some(options);
                self.choose(ctx);
            }
            NewDeviceMsg::Name(name) => {
                choice.name = name;
                return false;
            }
            NewDeviceMsg::FarPort(port) => {
                choice.far_port = port;
                return false;
            }
            NewDeviceMsg::Commit => {
                if choice.planned_name == choice.name && choice.planned_far_port == choice.far_port {
                    return false;
                }
                choice.planned_name = choice.name.clone();
                choice.planned_far_port = choice.far_port.clone();
                self.choose(ctx);
            }
            NewDeviceMsg::DeviceType(device_type) => {
                choice.device_type = Some(device_type.id);
                self.choose(ctx);
            }
            NewDeviceMsg::Role(role) => {
                choice.role = Some(role.id);
                self.choose(ctx);
            }
            NewDeviceMsg::WlanGroup(group) => {
                choice.wlan_group = group;
                self.choose(ctx);
            }
            NewDeviceMsg::Written(device) => choice.written = device,
        }
        true
    }
    pub(super) fn new_device_form(&self, ctx: &Context<Self>) -> Html {
        let choice = &self.new_device;
        if let Some((site, id)) = choice.written {
            return html! {
                <p><Link<AppRoute> to={AppRoute::device(site, id)}>{t::NEW_DEVICE_OPEN}</Link<AppRoute>></p>
            };
        }
        if choice.port.is_none() {
            return html!(<p>{t::CHOOSE_NEW_DEVICE}</p>);
        }
        let options = match &self.new_device_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => &options.new_device_options,
        };
        if options.device_types.is_empty() {
            return html!(<p>{t::NEW_DEVICE_NO_TYPES}</p>);
        }
        let types = options
            .device_types
            .iter()
            .map(|t| Choice {
                id: t.id as u32,
                text: match &t.part_number {
                    Some(part) if *part != t.model => format!("{} ({part})", t.model),
                    _ => t.model.clone(),
                },
            })
            .collect::<Vec<_>>();
        let roles = options
            .roles
            .iter()
            .map(|r| Choice {
                id: r.id as u32,
                text: r.name.clone(),
            })
            .collect::<Vec<_>>();
        let groups = std::iter::once(NetEntry {
            value: None,
            text: t::NEW_DEVICE_NO_WLAN_GROUP.to_string(),
        })
        .chain(options.wlan_groups.iter().map(|g| NetEntry {
            value: Some(g.id as u32),
            text: t::wlan_group(&g.name, &g.controller),
        }))
        .collect::<Vec<_>>();
        let selected = |entries: &[Choice], id: Option<u32>| {
            entries.iter().find(|e| Some(e.id) == id).cloned()
        };
        let selected_type = selected(&types, choice.device_type);
        let selected_role = selected(&roles, choice.role);
        let selected_group = groups.iter().find(|g| g.value == choice.wlan_group).cloned();
        let seen = choice.seen.as_ref().map(|seen| {
            let matched = choice.device_type.is_some()
                && options.device_types.iter().any(|t| {
                    Some(t.id as u32) == choice.device_type
                        && (t.model.eq_ignore_ascii_case(&seen.board)
                            || t.part_number.as_ref().is_some_and(|p| p.eq_ignore_ascii_case(&seen.board)))
                });
            html!(<p>{t::new_device_seen(&seen.identity, &seen.board, &seen.interface_name, matched)}</p>)
        });
        let commit = ctx.link().callback(|_: yew::FocusEvent| NewDeviceMsg::Commit);
        let enter = ctx.link().batch_callback(|event: yew::KeyboardEvent| {
            (event.key() == "Enter").then(|| {
                event.prevent_default();
                NetboxHelperMsg::from(NewDeviceMsg::Commit)
            })
        });
        html! {<>
            {seen}
            <Form>
                <FormGroup label={t::NEW_DEVICE_NAME}>
                    <TextInput
                        value={choice.name.clone()}
                        placeholder={t::NEW_DEVICE_NAME_PLACEHOLDER}
                        onchange={ctx.link().callback(NewDeviceMsg::Name)}
                        onblur={commit.clone()}
                        onkeydown={enter.clone()}
                    />
                </FormGroup>
                <FormGroup label={t::NEW_DEVICE_TYPE}>
                    <Select<Choice>
                        placeholder={t::NEW_DEVICE_TYPE_PLACEHOLDER}
                        entries={types}
                        selected={selected_type}
                        onselect={ctx.link().callback(NewDeviceMsg::DeviceType)}
                    />
                </FormGroup>
                <FormGroup label={t::NEW_DEVICE_ROLE}>
                    <Select<Choice>
                        entries={roles}
                        selected={selected_role}
                        onselect={ctx.link().callback(NewDeviceMsg::Role)}
                    />
                </FormGroup>
                <FormGroup label={t::NEW_DEVICE_PORT}>
                    <TextInput
                        value={choice.far_port.clone()}
                        onchange={ctx.link().callback(NewDeviceMsg::FarPort)}
                        onblur={commit}
                        onkeydown={enter}
                    />
                </FormGroup>
                <FormGroup label={t::NEW_DEVICE_WLAN_GROUP}>
                    <Select<NetEntry<Option<u32>>>
                        entries={groups}
                        selected={selected_group}
                        onselect={ctx.link().callback(|group: NetEntry<Option<u32>>| NewDeviceMsg::WlanGroup(group.value))}
                    />
                </FormGroup>
            </Form>
        </>}
    }
}

pub(super) fn fetch_new_device_options(scope: Scope<NetboxHelper>) {
    spawn_local(async move {
        let options = query_authenticated::<NewDeviceOptions, _>(
            scope.clone(),
            new_device_options::Variables {},
        )
        .await;
        scope.send_message(NewDeviceMsg::Options(options));
    });
}

/// The device of this name, once the topology has it after the write.
pub(super) fn fetch_written_device(scope: Scope<NetboxHelper>, name: String) {
    spawn_local(async move {
        let device = query_authenticated::<NewDeviceDevices, _>(
            scope.clone(),
            new_device_devices::Variables {},
        )
        .await
        .ok()
        .and_then(|data| {
            data.topology
                .all_devices
                .into_iter()
                .find(|d| d.name.eq_ignore_ascii_case(name.trim()))
                .and_then(|d| Some((d.site?.id as u32, d.id as u32)))
        });
        scope.send_message(NewDeviceMsg::Written(device));
    });
}
