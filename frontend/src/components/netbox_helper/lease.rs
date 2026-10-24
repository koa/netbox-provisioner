//! „Lease übernehmen“: the client of a static DHCP lease only the router has, with its name,
//! device type and role (`leaseTakeoverOptions`); its address and MAC come from the live check.
//! A device type named „Generic“ and the role `client` are preselected where NetBox has them.
//! The name is planned when left or confirmed with Enter, not on each key.
use super::{Choice, NetboxHelper, NetboxHelperMsg, t};
use crate::{
    components::menu::popup::Select,
    error::FrontendError,
    graphql::{
        authenticated::{LeaseTakeoverOptions, lease_takeover_options},
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Form, FormGroup, Spinner, SpinnerSize, TextInput};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

pub enum LeaseMsg {
    Options(Result<lease_takeover_options::ResponseData, FrontendError>),
    Name(String),
    /// the name left or confirmed: planned
    Commit,
    DeviceType(Choice),
    Role(Choice),
}

/// What „Lease übernehmen“ is choosing; the name as typed and as planned.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeaseChoice {
    pub(super) address: String,
    pub(super) mac: String,
    pub(super) name: String,
    pub(super) planned_name: String,
    pub(super) device_type: Option<u32>,
    pub(super) role: Option<u32>,
}

impl LeaseChoice {
    pub fn new(address: String, mac: String, name: Option<String>) -> Self {
        let name = name.unwrap_or_default();
        Self {
            address,
            mac,
            planned_name: name.clone(),
            name,
            ..Self::default()
        }
    }
}

impl NetboxHelper {
    pub(super) fn update_lease(&mut self, ctx: &Context<Self>, msg: LeaseMsg) -> bool {
        let choice = &mut self.lease;
        match msg {
            LeaseMsg::Options(options) => {
                if let Ok(options) = &options {
                    let options = &options.lease_takeover_options;
                    choice.device_type = choice.device_type.or_else(|| {
                        options
                            .device_types
                            .iter()
                            .find(|t| t.model.eq_ignore_ascii_case("generic"))
                            .map(|t| t.id as u32)
                    });
                    choice.role = choice.role.or_else(|| {
                        options.roles.iter().find(|r| r.slug == "client").map(|r| r.id as u32)
                    });
                }
                self.lease_options = Some(options);
                self.choose(ctx);
            }
            LeaseMsg::Name(name) => {
                choice.name = name;
                return false;
            }
            LeaseMsg::Commit => {
                if choice.planned_name == choice.name {
                    return false;
                }
                choice.planned_name = choice.name.clone();
                self.choose(ctx);
            }
            LeaseMsg::DeviceType(device_type) => {
                choice.device_type = Some(device_type.id);
                self.choose(ctx);
            }
            LeaseMsg::Role(role) => {
                choice.role = Some(role.id);
                self.choose(ctx);
            }
        }
        true
    }
    pub(super) fn lease_form(&self, ctx: &Context<Self>) -> Html {
        let choice = &self.lease;
        if choice.address.is_empty() {
            return html!(<p>{t::CHOOSE_LEASE_TAKEOVER}</p>);
        }
        let options = match &self.lease_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => &options.lease_takeover_options,
        };
        let types = options
            .device_types
            .iter()
            .map(|t| Choice {
                id: t.id as u32,
                text: format!("{} {}", t.manufacturer, t.model),
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
        let selected = |entries: &[Choice], id: Option<u32>| {
            entries.iter().find(|e| Some(e.id) == id).cloned()
        };
        let selected_type = selected(&types, choice.device_type);
        let selected_role = selected(&roles, choice.role);
        let commit = ctx.link().callback(|_: yew::FocusEvent| LeaseMsg::Commit);
        let enter = ctx.link().batch_callback(|event: yew::KeyboardEvent| {
            (event.key() == "Enter").then(|| {
                event.prevent_default();
                NetboxHelperMsg::from(LeaseMsg::Commit)
            })
        });
        html! {<>
            <p>{t::lease_seen(&choice.address, &choice.mac)}</p>
            <Form>
                <FormGroup label={t::LEASE_NAME}>
                    <TextInput
                        value={choice.name.clone()}
                        placeholder={t::LEASE_NAME_PLACEHOLDER}
                        onchange={ctx.link().callback(LeaseMsg::Name)}
                        onblur={commit}
                        onkeydown={enter}
                    />
                </FormGroup>
                <FormGroup label={t::NEW_DEVICE_TYPE}>
                    <Select<Choice>
                        placeholder={t::NEW_DEVICE_TYPE_PLACEHOLDER}
                        entries={types}
                        selected={selected_type}
                        onselect={ctx.link().callback(LeaseMsg::DeviceType)}
                    />
                </FormGroup>
                <FormGroup label={t::NEW_DEVICE_ROLE}>
                    <Select<Choice>
                        placeholder={t::LEASE_ROLE_PLACEHOLDER}
                        entries={roles}
                        selected={selected_role}
                        onselect={ctx.link().callback(LeaseMsg::Role)}
                    />
                </FormGroup>
            </Form>
        </>}
    }
}

pub(super) fn fetch_lease_options(scope: Scope<NetboxHelper>) {
    spawn_local(async move {
        let options = query_authenticated::<LeaseTakeoverOptions, _>(
            scope.clone(),
            lease_takeover_options::Variables {},
        )
        .await;
        scope.send_message(LeaseMsg::Options(options));
    });
}
