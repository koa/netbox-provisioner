//! Scope: The form of „Flags“; nothing else.
//!
//! „Flags“: the interfaces' tags as switches, with the device's findings as they would be
//! after them (`topology.findings(after:)`).
use super::{HelperChoice, NetboxHelper, choice::helper, t};
use crate::{
    components::findings::finding_list,
    error::FrontendError,
    graphql::{
        authenticated::{
            findings::{Finding, Findings, FindingsVariables},
            helper::InterfaceTag,
            helper_options::{self, FlagInterfaces},
            types::DeviceVariables,
        },
        request,
    },
};
use cynic::{QueryBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{Spinner, SpinnerSize};
use yew::{Context, Html, classes, html, html::Scope, platform::spawn_local};

pub enum FlagsMsg {
    Interfaces(Result<Vec<FlagInterface>, FrontendError>),
    /// a switch to this
    Toggle(FlagChange),
    Findings(FlagFindings),
}

/// A switch of „Flags“: the interface's tag `flag` set or taken away.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FlagChange {
    pub(super) interface: u32,
    pub(super) flag: Flag,
    pub(super) set: bool,
}

/// A tag of „Flags“.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Flag {
    Ospf,
    Dhcp,
    DhcpClient,
    Nat,
    Internet,
}

pub(super) const FLAGS: [Flag; 5] = [
    Flag::Ospf,
    Flag::Dhcp,
    Flag::DhcpClient,
    Flag::Nat,
    Flag::Internet,
];

impl Flag {
    pub(super) fn slug(self) -> &'static str {
        match self {
            Flag::Ospf => "ospf",
            Flag::Dhcp => "dhcp",
            Flag::DhcpClient => "dhcp-client",
            Flag::Nat => "nat",
            Flag::Internet => "internet",
        }
    }
}

impl NetboxHelper {
    pub(super) fn update_flags(&mut self, ctx: &Context<Self>, msg: FlagsMsg) -> bool {
        match msg {
            FlagsMsg::Interfaces(interfaces) => self.flag_interfaces = Some(interfaces),
            FlagsMsg::Toggle(FlagChange {
                interface,
                flag,
                set,
            }) => {
                let has = self
                    .flag_interface(interface)
                    .is_some_and(|i| i.tags.iter().any(|tag| tag_slug(tag) == flag.slug()));
                if has == set {
                    self.flags.remove(&(interface, flag));
                } else {
                    self.flags.insert((interface, flag), set);
                }
                self.check_flags(ctx);
                self.choose(ctx);
            }
            FlagsMsg::Findings(findings) if findings.changes != self.flag_changes() => {
                return false;
            }
            FlagsMsg::Findings(findings) => self.flag_findings = Some(findings),
        }
        true
    }
    pub(super) fn flag_changes(&self) -> Vec<FlagChange> {
        self.flags
            .iter()
            .map(|(&(interface, flag), &set)| FlagChange {
                interface,
                flag,
                set,
            })
            .collect()
    }
    pub(super) fn flag_interface(&self, id: u32) -> Option<&FlagInterface> {
        self.flag_interfaces
            .as_ref()?
            .as_ref()
            .ok()?
            .iter()
            .find(|interface| interface.id as u32 == id)
    }
    /// Fetches the device's findings after the switches, unless they are there already.
    pub(super) fn check_flags(&mut self, ctx: &Context<Self>) {
        let changes = self.flag_changes();
        if self
            .flag_findings
            .as_ref()
            .is_some_and(|shown| shown.changes == changes)
        {
            return;
        }
        self.flag_findings = None;
        fetch_flag_findings(ctx.link().clone(), ctx.props().id, changes);
    }
    pub(super) fn flags_form(&self, ctx: &Context<Self>) -> Html {
        let interfaces = match &self.flag_interfaces {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(interfaces)) => interfaces,
        };
        let rows =
            interfaces
                .iter()
                .filter(|interface| self.flags_of.is_none_or(|id| interface.id as u32 == id))
                .map(|interface| {
                    let id = interface.id as u32;
                    let name = match &interface.label {
                        Some(label) => format!("{} ({label})", interface.name),
                        None => interface.name.clone(),
                    };
                    let mut about: Vec<String> = interface.addresses.clone();
                    about.extend(
                        interface
                            .connected
                            .iter()
                            .map(|far| t::cabled_to(&far.device.name, &far.name)),
                    );
                    if let Some(bridge) = &interface.bridge {
                        about.push(t::bridge_port(bridge));
                    }
                    let switches = FLAGS.iter().map(|&flag| {
                let has = interface.tags.iter().any(|tag| tag_slug(tag) == flag.slug());
                let set = self.flags.get(&(id, flag)).copied().unwrap_or(has);
                let class = (set != has).then_some("flag-changed");
                let onchange = ctx.link().callback(move |_| {
                    FlagsMsg::Toggle(FlagChange {
                        interface: id,
                        flag,
                        set: !set,
                    })
                });
                html! {
                    <td class={classes!("pf-v6-c-table__td", class)} data-label={flag.slug()}>
                        <input
                            type="checkbox"
                            checked={set}
                            aria-label={format!("{} {}", interface.name, flag.slug())}
                            disabled={self.job_running}
                            {onchange}
                        />
                    </td>
                }
            });
                    html! {
                        <tr class="pf-v6-c-table__tr" key={id}>
                            <td class="pf-v6-c-table__td" data-label={t::INTERFACE}>
                                <div>{name}</div>
                                <small>{about.join(", ")}</small>
                            </td>
                            { for switches }
                        </tr>
                    }
                });
        let findings = match &self.flag_findings {
            None => html!(<Spinner size={SpinnerSize::Md}/>),
            Some(FlagFindings {
                findings: Err(error),
                ..
            }) => error.to_html(),
            Some(FlagFindings {
                changes,
                findings: Ok(found),
            }) if found.findings.is_empty() => {
                html!(<p>{t::no_flag_findings(!changes.is_empty())}</p>)
            }
            Some(FlagFindings {
                changes,
                findings: Ok(found),
            }) => html! {<>
                <p>{t::flag_findings(!changes.is_empty())}</p>
                { finding_list(&found.netbox, &found.findings) }
            </>},
        };
        html! {<>
            <div class="flag-table">
                <table class="pf-v6-c-table pf-m-compact pf-m-grid-md" role="grid">
                    <thead class="pf-v6-c-table__thead">
                        <tr class="pf-v6-c-table__tr">
                            <th class="pf-v6-c-table__th">{t::INTERFACE}</th>
                            { for FLAGS.iter().map(|flag| html!(<th class="pf-v6-c-table__th">{flag.slug()}</th>)) }
                        </tr>
                    </thead>
                    <tbody class="pf-v6-c-table__tbody">{ for rows }</tbody>
                </table>
            </div>
            <div class="helper-findings">{findings}</div>
        </>}
    }
}

pub(super) fn fetch_flag_interfaces(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let query = FlagInterfaces::build(DeviceVariables { id: id as i32 });
        let interfaces = request(scope.clone(), query).await.map(|data| {
            data.topology
                .device_by_id
                .map(|device| device.flag_interfaces)
                .unwrap_or_default()
        });
        scope.send_message(FlagsMsg::Interfaces(interfaces));
    });
}

/// The device's findings as they would be with `changes` (as they are without any).
pub(super) fn fetch_flag_findings(scope: Scope<NetboxHelper>, id: u32, changes: Vec<FlagChange>) {
    spawn_local(async move {
        let after = (!changes.is_empty()).then(|| helper(HelperChoice::Flags(changes.clone()), id));
        let variables = FindingsVariables {
            site: None,
            device: Some(id as i32),
            after,
        };
        let result = request(scope.clone(), Findings::build(variables))
            .await
            .map(|data| NetboxFindings {
                netbox: data.topology.netbox_url,
                findings: data.topology.findings,
            });
        scope.send_message(FlagsMsg::Findings(FlagFindings {
            changes,
            findings: result,
        }));
    });
}

pub(super) type FlagInterface = helper_options::FlagInterface;

/// The device's findings after these switches.
pub struct FlagFindings {
    changes: Vec<FlagChange>,
    findings: Result<NetboxFindings, FrontendError>,
}

/// Findings, with NetBox' address for their links.
pub struct NetboxFindings {
    netbox: String,
    findings: Vec<Finding>,
}

/// A tag as NetBox names it.
pub(super) fn tag_slug(tag: &InterfaceTag) -> &str {
    match tag {
        InterfaceTag::Ospf => "ospf",
        InterfaceTag::Dhcp => "dhcp",
        InterfaceTag::DhcpClient => "dhcp-client",
        InterfaceTag::Nat => "nat",
        InterfaceTag::Internet => "internet",
        InterfaceTag::Other(other) => other,
    }
}
