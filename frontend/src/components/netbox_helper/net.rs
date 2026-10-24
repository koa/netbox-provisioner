//! „Kunden-Netz“: a port or VLAN and a prefix of the site (`customerNetOptions`).
use super::{NetEntry, NetTarget, NetboxHelper, t};
use crate::{
    components::menu::popup::Select,
    error::FrontendError,
    graphql::{
        authenticated::{CustomerNetOptions, customer_net_options},
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Form, FormGroup, Spinner, SpinnerSize, TextInput};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

pub enum NetMsg {
    Options(Result<NetOptions, FrontendError>),
    Target(NetTarget),
    Parent(Option<u32>),
    ParentV6(Option<u32>),
    Length(String),
    Dhcp(bool),
}

/// What „Kunden-Netz“ is choosing.
#[derive(Debug, Clone, PartialEq)]
pub struct NetChoice {
    pub(super) target: Option<NetTarget>,
    pub(super) parent: Option<u32>,
    pub(super) length: String,
    pub(super) parent_v6: Option<u32>,
    pub(super) dhcp: bool,
}

impl Default for NetChoice {
    fn default() -> Self {
        Self {
            target: None,
            parent: None,
            length: String::new(),
            parent_v6: None,
            dhcp: true,
        }
    }
}

impl NetboxHelper {
    pub(super) fn update_net(&mut self, ctx: &Context<Self>, msg: NetMsg) -> bool {
        match msg {
            NetMsg::Options(options) => self.net_options = Some(options),
            NetMsg::Target(target) => {
                self.net.target = Some(target);
                self.choose(ctx);
            }
            NetMsg::Parent(parent) => {
                self.net.parent = parent;
                self.choose(ctx);
            }
            NetMsg::ParentV6(parent) => {
                self.net.parent_v6 = parent;
                self.choose(ctx);
            }
            NetMsg::Length(length) => {
                self.net.length = length;
                self.choose(ctx);
            }
            NetMsg::Dhcp(dhcp) => {
                self.net.dhcp = dhcp;
                self.choose(ctx);
            }
        }
        true
    }
    pub(super) fn net_form(&self, ctx: &Context<Self>) -> Html {
        let options = match &self.net_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => options,
        };
        let targets: Vec<NetEntry<NetTarget>> = options
            .interfaces
            .iter()
            .map(|interface| NetEntry {
                value: NetTarget::Interface(interface.id as u32),
                text: t::net_port(&interface.name, interface.label.as_deref()),
            })
            .chain(options.vlans.iter().map(|vlan| NetEntry {
                value: NetTarget::Vlan(vlan.id as u32),
                text: t::net_vlan(&vlan.name, vlan.vid, vlan.interface.as_deref()),
            }))
            .collect();
        // with „keins“ first: a network may come in one family only
        let entries = |parents: Vec<(i64, &String)>| -> Vec<NetEntry<Option<u32>>> {
            (!parents.is_empty())
                .then(|| NetEntry {
                    value: None,
                    text: t::NO_PARENT.into(),
                })
                .into_iter()
                .chain(parents.into_iter().map(|(id, prefix)| NetEntry {
                    value: Some(id as u32),
                    text: prefix.clone(),
                }))
                .collect()
        };
        let parents = entries(options.parents.iter().map(|p| (p.id, &p.prefix)).collect());
        let parents_v6 = entries(options.parents_v6.iter().map(|p| (p.id, &p.prefix)).collect());
        let target = targets.iter().find(|e| Some(e.value) == self.net.target).cloned();
        let selected = |entries: &[NetEntry<Option<u32>>], value: Option<u32>| {
            value.and_then(|value| entries.iter().find(|e| e.value == Some(value)).cloned())
        };
        let parent = selected(&parents, self.net.parent);
        let parent_v6 = selected(&parents_v6, self.net.parent_v6);
        let dhcp = self.net.dhcp;
        html! {
            <Form>
                if !self.fixed_port {
                    <FormGroup label={t::NET_TARGET}>
                        if targets.is_empty() {
                            <p>{t::NO_NET_TARGETS}</p>
                        } else {
                            <Select<NetEntry<NetTarget>>
                                placeholder={t::CHOOSE_NET_TARGET}
                                selected={target}
                                entries={targets}
                                onselect={ctx.link().callback(|entry: NetEntry<NetTarget>| NetMsg::Target(entry.value))}
                            />
                        }
                    </FormGroup>
                }
                <FormGroup label={t::PARENT}>
                    if parents.is_empty() {
                        <p>{t::NO_PARENTS}</p>
                    } else {
                        <Select<NetEntry<Option<u32>>>
                            placeholder={t::CHOOSE_PARENT}
                            selected={parent}
                            entries={parents}
                            onselect={ctx.link().callback(|entry: NetEntry<Option<u32>>| NetMsg::Parent(entry.value))}
                        />
                    }
                </FormGroup>
                <FormGroup label={t::PARENT_V6}>
                    if parents_v6.is_empty() {
                        <p>{t::NO_PARENTS_V6}</p>
                    } else {
                        <Select<NetEntry<Option<u32>>>
                            placeholder={t::CHOOSE_PARENT}
                            selected={parent_v6}
                            entries={parents_v6}
                            onselect={ctx.link().callback(|entry: NetEntry<Option<u32>>| NetMsg::ParentV6(entry.value))}
                        />
                    }
                </FormGroup>
                <FormGroup label={t::LENGTH}>
                    <TextInput
                        value={self.net.length.clone()}
                        placeholder={t::LENGTH_PLACEHOLDER}
                        onchange={ctx.link().callback(NetMsg::Length)}
                    />
                </FormGroup>
                <FormGroup>
                    <label class="net-dhcp">
                        <input
                            type="checkbox"
                            checked={dhcp}
                            onchange={ctx.link().callback(move |_| NetMsg::Dhcp(!dhcp))}
                        />
                        {" "}{t::DHCP_SERVER}
                    </label>
                </FormGroup>
            </Form>
        }
    }
}

pub(super) fn fetch_net_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<CustomerNetOptions, _>(
            scope.clone(),
            customer_net_options::Variables { id: id as i64 },
        )
        .await
        .map(|data| match data.topology.device_by_id {
            Some(device) => device.customer_net_options,
            None => NetOptions {
                interfaces: Vec::new(),
                vlans: Vec::new(),
                parents: Vec::new(),
                parents_v6: Vec::new(),
            },
        });
        scope.send_message(NetMsg::Options(options));
    });
}

pub(super) type NetOptions = customer_net_options::CustomerNetOptionsTopologyDeviceByIdCustomerNetOptions;
