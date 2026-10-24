//! „VLAN am Port“: a port, then the VLAN group, then its VLAN (`portVlanOptions`).
use super::{Choice, NetboxHelper, t};
use crate::{
    components::menu::popup::Select,
    error::FrontendError,
    graphql::{
        authenticated::{PortVlanOptions, port_vlan_options},
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Form, FormGroup, Spinner, SpinnerSize};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

pub enum PortVlanMsg {
    Options(Result<PortVlanOptionsData, FrontendError>),
    Port(u32),
    Group(u32),
    Vlan(u32),
}

/// What „VLAN am Port“ is choosing: the group first, then a VLAN of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PortVlanChoice {
    pub(super) port: Option<u32>,
    pub(super) group: Option<u32>,
    pub(super) vlan: Option<u32>,
}

impl NetboxHelper {
    pub(super) fn update_port_vlan(&mut self, ctx: &Context<Self>, msg: PortVlanMsg) -> bool {
        match msg {
            PortVlanMsg::Options(options) => {
                // the group of the VLAN given, else the one the backend starts with
                if let Ok(options) = &options
                    && self.port_vlan.group.is_none()
                {
                    self.port_vlan.group = self
                        .port_vlan
                        .vlan
                        .and_then(|vlan| {
                            options.groups.iter().find(|g| g.vlans.iter().any(|v| v.id as u32 == vlan))
                        })
                        .map(|g| g.id as u32)
                        .or(options.group.map(|g| g as u32));
                }
                self.port_vlan_options = Some(options);
            }
            PortVlanMsg::Group(group) => {
                if self.port_vlan.group != Some(group) {
                    self.port_vlan.group = Some(group);
                    self.port_vlan.vlan = None;
                    self.choose(ctx);
                }
            }
            PortVlanMsg::Port(port) => {
                self.port_vlan.port = Some(port);
                self.choose(ctx);
            }
            PortVlanMsg::Vlan(vlan) => {
                self.port_vlan.vlan = Some(vlan);
                self.choose(ctx);
            }
        }
        true
    }
    pub(super) fn port_vlan_form(&self, ctx: &Context<Self>) -> Html {
        let options = match &self.port_vlan_options {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(options)) => options,
        };
        let ports: Vec<Choice> = options
            .ports
            .iter()
            .map(|port| Choice {
                id: port.id as u32,
                text: t::port_vlan_port(
                    &port.name,
                    port.label.as_deref(),
                    port.vlan.as_ref().map(|vlan| (vlan.vid, vlan.name.as_str())),
                ),
            })
            .collect();
        let groups: Vec<Choice> = options
            .groups
            .iter()
            .map(|group| Choice {
                id: group.id as u32,
                text: t::port_vlan_group(&group.name, group.vxlan),
            })
            .collect();
        let vlans: Vec<Choice> = options
            .groups
            .iter()
            .filter(|group| Some(group.id as u32) == self.port_vlan.group)
            .flat_map(|group| &group.vlans)
            .map(|vlan| Choice {
                id: vlan.id as u32,
                text: t::port_vlan_vlan(vlan.vid, &vlan.name),
            })
            .collect();
        let port = ports.iter().find(|c| Some(c.id) == self.port_vlan.port).cloned();
        let group = groups.iter().find(|c| Some(c.id) == self.port_vlan.group).cloned();
        let vlan = vlans.iter().find(|c| Some(c.id) == self.port_vlan.vlan).cloned();
        html! {
            <Form>
                if !self.fixed_port {
                <FormGroup label={t::PORT_VLAN_PORT}>
                    if ports.is_empty() {
                        <p>{t::NO_PORT_VLAN_PORTS}</p>
                    } else {
                        <Select<Choice>
                            placeholder={t::CHOOSE_PORT_VLAN_PORT}
                            selected={port}
                            entries={ports}
                            onselect={ctx.link().callback(|choice: Choice| PortVlanMsg::Port(choice.id))}
                        />
                    }
                </FormGroup>
                }
                if groups.is_empty() {
                    <FormGroup label={t::PORT_VLAN_VLAN}>
                        <p>{t::NO_PORT_VLAN_VLANS}</p>
                    </FormGroup>
                } else {
                    <FormGroup label={t::PORT_VLAN_GROUP}>
                        <Select<Choice>
                            placeholder={t::CHOOSE_PORT_VLAN_GROUP}
                            selected={group}
                            entries={groups}
                            onselect={ctx.link().callback(|choice: Choice| PortVlanMsg::Group(choice.id))}
                        />
                    </FormGroup>
                    if self.port_vlan.group.is_some() {
                        <FormGroup label={t::PORT_VLAN_VLAN}>
                            <Select<Choice>
                                placeholder={t::CHOOSE_PORT_VLAN_VLAN}
                                selected={vlan}
                                entries={vlans}
                                onselect={ctx.link().callback(|choice: Choice| PortVlanMsg::Vlan(choice.id))}
                            />
                        </FormGroup>
                    }
                }
            </Form>
        }
    }
}

pub(super) fn fetch_port_vlan_options(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let options = query_authenticated::<PortVlanOptions, _>(
            scope.clone(),
            port_vlan_options::Variables { id: id as i64 },
        )
        .await
        .map(|data| match data.topology.device_by_id {
            Some(device) => device.port_vlan_options,
            None => PortVlanOptionsData {
                ports: Vec::new(),
                groups: Vec::new(),
                group: None,
            },
        });
        scope.send_message(PortVlanMsg::Options(options));
    });
}

pub(super) type PortVlanOptionsData = port_vlan_options::PortVlanOptionsTopologyDeviceByIdPortVlanOptions;
