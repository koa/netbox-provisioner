//! „Flags“: the interfaces' tags as switches, with the device's findings as they would be
//! after them (`topology.findings(after:)`).
use super::{HelperChoice, NetTarget, NetboxHelper, t};
use crate::{
    components::findings::finding_list,
    error::FrontendError,
    graphql::{
        authenticated::{
            FlagInterfaces, Findings, flag_interfaces,
            findings::{self, FindingsTopologyFindings},
            netbox_plan,
        },
        query_authenticated,
    },
};
use patternfly_yew::prelude::{Spinner, SpinnerSize};
use yew::{Context, Html, classes, html, html::Scope, platform::spawn_local};

pub enum FlagsMsg {
    Interfaces(Result<Vec<FlagInterface>, FrontendError>),
    /// a switch to this
    Toggle(u32, Flag, bool),
    Findings(
        Vec<(u32, Flag, bool)>,
        Result<(String, Vec<FindingsTopologyFindings>), FrontendError>,
    ),
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

pub(super) const FLAGS: [Flag; 5] =
    [Flag::Ospf, Flag::Dhcp, Flag::DhcpClient, Flag::Nat, Flag::Internet];

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
            FlagsMsg::Toggle(interface, flag, set) => {
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
            FlagsMsg::Findings(changes, _) if changes != self.flag_changes() => {
                return false;
            }
            FlagsMsg::Findings(changes, findings) => {
                self.flag_findings = Some((changes, findings));
            }
        }
        true
    }
    pub(super) fn flag_changes(&self) -> Vec<(u32, Flag, bool)> {
        self.flags
            .iter()
            .map(|(&(interface, flag), &set)| (interface, flag, set))
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
        if self.flag_findings.as_ref().is_some_and(|(shown, _)| *shown == changes) {
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
        let rows = interfaces
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
                let onchange = ctx.link().callback(move |_| FlagsMsg::Toggle(id, flag, !set));
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
            Some((_, Err(error))) => error.to_html(),
            Some((changes, Ok((_, findings)))) if findings.is_empty() => {
                html!(<p>{t::no_flag_findings(!changes.is_empty())}</p>)
            }
            Some((changes, Ok((netbox, findings)))) => html! {<>
                <p>{t::flag_findings(!changes.is_empty())}</p>
                { finding_list(netbox, findings) }
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
        let interfaces = query_authenticated::<FlagInterfaces, _>(
            scope.clone(),
            flag_interfaces::Variables { id: id as i64 },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .map(|device| device.flag_interfaces)
                .unwrap_or_default()
        });
        scope.send_message(FlagsMsg::Interfaces(interfaces));
    });
}

/// The device's findings as they would be with `changes` (as they are without any).
pub(super) fn fetch_flag_findings(scope: Scope<NetboxHelper>, id: u32, changes: Vec<(u32, Flag, bool)>) {
    spawn_local(async move {
        let after = (!changes.is_empty())
            .then(|| helper_input!(findings, HelperChoice::Flags(changes.clone()), id));
        let result = query_authenticated::<Findings, _>(
            scope.clone(),
            findings::Variables {
                site: None,
                device: Some(id as i64),
                after,
            },
        )
        .await
        .map(|data| (data.netbox_url, data.topology.findings));
        scope.send_message(FlagsMsg::Findings(changes, result));
    });
}

pub(super) type FlagInterface = flag_interfaces::FlagInterfacesTopologyDeviceByIdFlagInterfaces;

/// The device's findings after these switches, with NetBox' address.
pub(super) type FlagFindings = (
    Vec<(u32, Flag, bool)>,
    Result<(String, Vec<FindingsTopologyFindings>), FrontendError>,
);

/// A tag as NetBox names it, from the `InterfaceTag` of a query (each has its own).
pub(super) trait TagSlug {
    fn slug(&self) -> &str;
}

macro_rules! tag_slug {
    ($($module:ident),*) => {$(
        impl TagSlug for $module::InterfaceTag {
            fn slug(&self) -> &str {
                match self {
                    $module::InterfaceTag::OSPF => "ospf",
                    $module::InterfaceTag::DHCP => "dhcp",
                    $module::InterfaceTag::DHCP_CLIENT => "dhcp-client",
                    $module::InterfaceTag::NAT => "nat",
                    $module::InterfaceTag::INTERNET => "internet",
                    $module::InterfaceTag::Other(other) => other,
                }
            }
        }
    )*};
}

tag_slug!(flag_interfaces, netbox_plan);

pub(super) fn tag_slug(tag: &impl TagSlug) -> &str {
    tag.slug()
}
