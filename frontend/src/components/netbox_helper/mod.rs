//! The helpers that complete NetBox for the device (`Query.netboxPlan`), one chosen at a time:
//! what it would write, on a click the preview (the findings and the script applying would
//! bring after it), then writing it as a job, and taking the last one back (`plan`). They write
//! to NetBox only, the device gets it by applying. The component here holds what each helper
//! chooses; a helper with a form has a module of its own with the form, its message and the
//! queries it chooses from: `link` („Verbindung“, „Verbindung abräumen“), `flags`, `net`
//! („Kunden-Netz“), `port_vlan`, `poe`, `radio` („Funkbrücke“). `choice` is what all share: the
//! preset a helper opens with, the helper shown and its input.
#[macro_use]
mod choice;
mod flags;
mod link;
mod net;
mod plan;
mod poe;
mod port_vlan;
mod radio;

pub use choice::{HelperKind, HelperPreset, LinkPreset, NetTarget, RadioPreset};

use choice::{Choice, HelperChoice, NetEntry};
use flags::{Flag, FlagFindings, FlagInterface, FlagsMsg, fetch_flag_interfaces};
use link::{LinkChoice, LinkMsg, fetch_ports};
use net::{NetChoice, NetMsg, NetOptions, fetch_net_options};
use plan::{Preview, fetch_plan, fetch_preview, last_write};
use poe::{Observed, PoeChoice, PoeMsg, PoeOptionsData, fetch_poe_options};
use port_vlan::{PortVlanChoice, PortVlanMsg, PortVlanOptionsData, fetch_port_vlan_options};
use radio::{Candidate, RadioChoice, RadioMsg, fetch_radio_options};
use crate::{
    components::{adjust_target::SelectedTarget, job::JobView},
    error::FrontendError,
    graphql::{
        authenticated::{
            UndoNetboxWrite, WriteNetboxPlan, link_ports, netbox_plan, undo_netbox_write,
            write_netbox_plan,
        },
        query_authenticated,
    },
    i18n::helper as t,
};
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Form, Switch,
};
use std::collections::BTreeMap;
use yew::{Callback, Component, Context, Html, Properties, html, platform::spawn_local};


#[derive(Clone, PartialEq, Properties)]
pub struct NetboxHelperProps {
    pub id: u32,
    /// how the device is reached for the preview of the script
    pub target: SelectedTarget,
    /// NetBox was written to (or that was taken back): the page shows what changed
    #[prop_or_default]
    pub on_written: Callback<()>,
    /// the helper to open and what it starts with (a cable from a finding, a row's action);
    /// without, „Loopback“
    #[prop_or_default]
    pub preset: Option<HelperPreset>,
    /// inside the modal of `components::helper_flow`: no card, no choice of the helper
    #[prop_or_default]
    pub embedded: bool,
    /// where it is: planning, the preview shown, writing, written
    #[prop_or_default]
    pub on_phase: Callback<Phase>,
}

/// How far the helper is, for the stepper of `components::helper_flow`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Phase {
    Plan,
    Preview,
    Writing,
    /// the write job is done
    Written,
    /// it failed or was cancelled; what is written can be taken back
    WriteFailed,
}

pub struct NetboxHelper {
    /// the device of the page
    device: u32,
    kind: HelperKind,
    link: LinkChoice,
    /// the port whose link „Verbindung abräumen“ takes out
    disconnect: Option<u32>,
    /// „Richtfunk abräumen“: the radio, and whether the wireless link goes too
    radio_disconnect: Option<u32>,
    radio_disconnect_link: bool,
    /// „Kunden-Anschluss abräumen“: the row's interface
    customer_disconnect: Option<u32>,
    /// „Aus Bridge lösen“: the port
    bridge_leave: Option<u32>,
    ports: Option<Result<link_ports::ResponseData, FrontendError>>,
    flag_interfaces: Option<Result<Vec<FlagInterface>, FrontendError>>,
    /// the tags switched away from what NetBox has
    flags: BTreeMap<(u32, Flag), bool>,
    /// „Flags“ of this interface only
    flags_of: Option<u32>,
    /// the preset gave the port the helper starts from (a port's row): the form doesn't offer
    /// it to choose
    fixed_port: bool,
    /// the device's findings after the switches, with NetBox' address: for these
    flag_findings: Option<FlagFindings>,
    net: NetChoice,
    net_options: Option<Result<NetOptions, FrontendError>>,
    port_vlan: PortVlanChoice,
    port_vlan_options: Option<Result<PortVlanOptionsData, FrontendError>>,
    /// „Kabel übernehmen“: the port and the one the device sees there
    cable_takeover: Option<(u32, u32)>,
    poe: PoeChoice,
    poe_options: Option<Result<PoeOptionsData, FrontendError>>,
    /// what the device reports feeding, once asked
    poe_observed: Option<Result<Vec<Observed>, FrontendError>>,
    poe_observing: bool,
    radio: RadioChoice,
    radio_options: Option<Result<Vec<Candidate>, FrontendError>>,
    /// what the plan shown was computed for
    planned: Option<HelperChoice>,
    plan: Option<Result<netbox_plan::ResponseData, FrontendError>>,
    /// a removal written: its choice is gone from NetBox, there is no plan for it any more
    removed: bool,
    preview: Option<Result<Preview, FrontendError>>,
    preview_loading: bool,
    /// the job shown, writing or undoing
    job: Option<i64>,
    job_running: bool,
    /// the write job whose objects "Rückgängig" deletes or sets back
    written: Option<i64>,
    /// how the write job of this session ended
    write_done: Option<bool>,
    /// the phase told last (`on_phase`)
    reported: Option<Phase>,
    error: Option<FrontendError>,
}

pub enum NetboxHelperMsg {
    Link(LinkMsg),
    Flags(FlagsMsg),
    Net(NetMsg),
    PortVlan(PortVlanMsg),
    Poe(PoeMsg),
    Radio(RadioMsg),
    RadioDisconnectLink(bool),
    Plan(HelperChoice, Result<netbox_plan::ResponseData, FrontendError>),
    LoadPreview,
    Preview(Result<Preview, FrontendError>),
    Write,
    Undo,
    /// a write job (`true`) or an undo job started
    JobStarted(Result<i64, FrontendError>, bool),
    /// `true`: done
    JobFinished(bool),
    /// the last write job of the device the server still knows, done and not taken back
    LastWrite(i64),
}

/// A helper's own message is one of the component: its forms' callbacks give it as is.
macro_rules! helper_msg {
    ($($variant:ident($msg:ty)),*) => {$(
        impl From<$msg> for NetboxHelperMsg {
            fn from(msg: $msg) -> Self {
                NetboxHelperMsg::$variant(msg)
            }
        }
    )*};
}

helper_msg!(
    Link(LinkMsg),
    Flags(FlagsMsg),
    Net(NetMsg),
    PortVlan(PortVlanMsg),
    Poe(PoeMsg),
    Radio(RadioMsg)
);

impl Component for NetboxHelper {
    type Message = NetboxHelperMsg;
    type Properties = NetboxHelperProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut link = LinkChoice::default();
        let mut disconnect = None;
        let mut net = NetChoice::default();
        let mut radio = RadioChoice::default();
        let mut radio_disconnect = None;
        let mut customer_disconnect = None;
        let mut bridge_leave = None;
        let mut port_vlan = PortVlanChoice::default();
        let mut cable_takeover = None;
        let mut poe = PoeChoice::default();
        let mut flags_of = None;
        let kind = match &ctx.props().preset {
            None => HelperKind::Loopback,
            Some(HelperPreset::Kind(kind)) => *kind,
            Some(HelperPreset::Link(preset)) => {
                link = LinkChoice {
                    port: Some(preset.port),
                    peer_device: Some(preset.peer_device),
                    peer: Some(preset.peer),
                    mtu: String::new(),
                };
                HelperKind::Connection
            }
            Some(HelperPreset::LinkFrom(port)) => {
                link.port = Some(*port);
                HelperKind::Connection
            }
            Some(HelperPreset::Disconnect(port)) => {
                disconnect = Some(*port);
                HelperKind::Disconnect
            }
            Some(HelperPreset::CustomerNet(target)) => {
                net.target = Some(*target);
                HelperKind::CustomerNet
            }
            Some(HelperPreset::RadioFrom(preset)) => {
                radio.router_device[0] = Some(preset.router_device);
                radio.router_port[0] = Some(preset.router_port);
                HelperKind::RadioBridge
            }
            Some(HelperPreset::RadioDisconnect(radio)) => {
                radio_disconnect = Some(*radio);
                HelperKind::RadioDisconnect
            }
            Some(HelperPreset::CustomerDisconnect(interface)) => {
                customer_disconnect = Some(*interface);
                HelperKind::CustomerDisconnect
            }
            Some(HelperPreset::PortVlan { port, vlan }) => {
                port_vlan = PortVlanChoice {
                    port: *port,
                    group: None,
                    vlan: *vlan,
                };
                HelperKind::PortVlan
            }
            Some(HelperPreset::CableTakeover { port, peer }) => {
                cable_takeover = Some((*port, *peer));
                HelperKind::CableTakeover
            }
            Some(HelperPreset::BridgeLeave(port)) => {
                bridge_leave = Some(*port);
                HelperKind::BridgeLeave
            }
            Some(HelperPreset::Poe(port)) => {
                poe.port = Some(*port);
                HelperKind::Poe
            }
            Some(HelperPreset::Flags(interface)) => {
                flags_of = Some(*interface);
                HelperKind::Flags
            }
            // the flow chooses the bridge device first: not here
            Some(HelperPreset::RadioChoose { .. }) => HelperKind::RadioBridge,
        };
        last_write(ctx.link().clone(), ctx.props().id);
        let mut helper = Self {
            device: ctx.props().id,
            kind,
            link,
            disconnect,
            radio_disconnect,
            radio_disconnect_link: false,
            customer_disconnect,
            bridge_leave,
            ports: None,
            flag_interfaces: None,
            flags: BTreeMap::new(),
            flags_of,
            fixed_port: ctx.props().preset.as_ref().is_some_and(HelperPreset::fixes_port),
            flag_findings: None,
            net,
            net_options: None,
            port_vlan,
            port_vlan_options: None,
            cable_takeover,
            poe,
            poe_options: None,
            poe_observed: None,
            poe_observing: false,
            radio,
            radio_options: None,
            planned: None,
            plan: None,
            removed: false,
            preview: None,
            preview_loading: false,
            job: None,
            job_running: false,
            written: None,
            write_done: None,
            reported: None,
            error: None,
        };
        helper.load(ctx);
        helper.choose(ctx);
        helper
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NetboxHelperMsg::RadioDisconnectLink(link) => {
                self.radio_disconnect_link = link;
                self.choose(ctx);
            }
            NetboxHelperMsg::Plan(choice, _) if self.planned.as_ref() != Some(&choice) => {
                // for a choice changed since
                return false;
            }
            NetboxHelperMsg::Plan(_, plan) => {
                // another plan, another preview
                let fingerprint = |plan: Option<&Result<netbox_plan::ResponseData, _>>| {
                    plan.and_then(|plan| plan.as_ref().ok())
                        .map(|plan| plan.netbox_plan.fingerprint.clone())
                };
                if fingerprint(self.plan.as_ref()) != fingerprint(Some(&plan)) {
                    self.preview = None;
                }
                self.plan = Some(plan);
            }
            NetboxHelperMsg::LoadPreview => {
                self.preview_loading = true;
                let Some(choice) = self.planned.clone() else {
                    return false;
                };
                fetch_preview(
                    ctx.link().clone(),
                    ctx.props().id,
                    choice,
                    ctx.props().target.clone(),
                );
            }
            NetboxHelperMsg::Preview(preview) => {
                self.preview_loading = false;
                self.preview = Some(preview);
            }
            NetboxHelperMsg::Write => {
                let (Some(Ok(plan)), Some(choice)) = (&self.plan, self.planned.clone()) else {
                    return false;
                };
                self.job_running = true;
                self.error = None;
                let scope = ctx.link().clone();
                let variables = write_netbox_plan::Variables {
                    helper: helper_input!(write_netbox_plan, choice, ctx.props().id),
                    fingerprint: plan.netbox_plan.fingerprint.clone(),
                };
                spawn_local(async move {
                    let result = query_authenticated::<WriteNetboxPlan, _>(scope.clone(), variables)
                        .await
                        .map(|data| data.write_netbox_plan.id);
                    scope.send_message(NetboxHelperMsg::JobStarted(result, true));
                });
            }
            NetboxHelperMsg::Undo => {
                let Some(job_id) = self.written else {
                    return false;
                };
                self.job_running = true;
                self.error = None;
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let result = query_authenticated::<UndoNetboxWrite, _>(
                        scope.clone(),
                        undo_netbox_write::Variables { job_id },
                    )
                    .await
                    .map(|data| data.undo_netbox_write.id);
                    scope.send_message(NetboxHelperMsg::JobStarted(result, false));
                });
            }
            NetboxHelperMsg::JobStarted(result, write) => match result {
                Ok(id) => {
                    self.job = Some(id);
                    self.written = write.then_some(id);
                    self.write_done = None;
                }
                Err(error) => {
                    self.job_running = false;
                    self.error = Some(error);
                }
            },
            NetboxHelperMsg::LastWrite(id) => {
                if self.job.is_none() {
                    self.written = Some(id);
                }
            }
            NetboxHelperMsg::JobFinished(done) => {
                self.job_running = false;
                if self.written.is_some() && self.written == self.job {
                    self.write_done = Some(done);
                }
                self.preview = None;
                // a written removal took what it was chosen for: planned again it finds nothing
                let removed = done
                    && self.written.is_some()
                    && self.written == self.job
                    && matches!(
                        self.kind,
                        HelperKind::Disconnect
                            | HelperKind::RadioDisconnect
                            | HelperKind::CustomerDisconnect
                            | HelperKind::BridgeLeave
                    );
                self.removed = removed;
                if removed {
                    self.plan = None;
                } else if let Some(choice) = self.planned.clone() {
                    fetch_plan(ctx.link().clone(), ctx.props().id, choice);
                }
                // the ports have the link now
                if self.ports.is_some() {
                    fetch_ports(ctx.link().clone(), ctx.props().id);
                }
                // the port or VLAN has its network now
                if self.net_options.is_some() {
                    self.net = NetChoice::default();
                    fetch_net_options(ctx.link().clone(), ctx.props().id);
                    self.choose(ctx);
                }
                // the port has its VLAN now
                if self.port_vlan_options.is_some() {
                    self.port_vlan = PortVlanChoice::default();
                    fetch_port_vlan_options(ctx.link().clone(), ctx.props().id);
                    self.choose(ctx);
                }
                // the bridges have their links and addresses now
                if self.radio_options.is_some() {
                    fetch_radio_options(ctx.link().clone(), ctx.props().id);
                }
                // the ports have their modes now
                if self.poe_options.is_some() {
                    fetch_poe_options(ctx.link().clone(), ctx.props().id);
                }
                // NetBox has the tags switched now (or again what it had)
                if self.flag_interfaces.is_some() {
                    self.flags.clear();
                    fetch_flag_interfaces(ctx.link().clone(), ctx.props().id);
                    self.check_flags(ctx);
                    self.choose(ctx);
                }
                ctx.props().on_written.emit(());
            }
            NetboxHelperMsg::Link(msg) => return self.update_link(ctx, msg),
            NetboxHelperMsg::Flags(msg) => return self.update_flags(ctx, msg),
            NetboxHelperMsg::Net(msg) => return self.update_net(ctx, msg),
            NetboxHelperMsg::PortVlan(msg) => return self.update_port_vlan(ctx, msg),
            NetboxHelperMsg::Poe(msg) => return self.update_poe(ctx, msg),
            NetboxHelperMsg::Radio(msg) => return self.update_radio(ctx, msg),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let job = self.job.map(|id| html! {
            <JobView key={id} id={id} on_finished={ctx.link().callback(NetboxHelperMsg::JobFinished)}/>
        });
        let error = self.error.as_ref().map(|error| error.to_html());
        let plan = self.plan_view(ctx);
        let undo = self.written.filter(|_| !self.job_running).map(|_| html! {
            <div class="helper-actions">
                <p>{t::WRITTEN}</p>
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::UNDO}
                    onclick={ctx.link().callback(|_| NetboxHelperMsg::Undo)}
                />
            </div>
        });
        let hint = match self.kind {
            HelperKind::Loopback => t::LOOPBACK_HINT,
            HelperKind::Connection => t::CONNECTION_HINT,
            HelperKind::Disconnect => t::DISCONNECT_HINT,
            HelperKind::Flags => t::FLAGS_HINT,
            HelperKind::CustomerNet => t::CUSTOMER_NET_HINT,
            HelperKind::Poe => t::POE_HINT,
            HelperKind::RadioBridge => t::RADIO_BRIDGE_HINT,
            HelperKind::RadioDisconnect => t::RADIO_DISCONNECT_HINT,
            HelperKind::CustomerDisconnect => t::CUSTOMER_DISCONNECT_HINT,
            HelperKind::BridgeLeave => t::BRIDGE_LEAVE_HINT,
            HelperKind::PortVlan => t::PORT_VLAN_HINT,
            HelperKind::CableTakeover => t::CABLE_TAKEOVER_HINT,
        };
        let content = html! {<>
                    {error}
                    {job}
                    {undo}
                    <p>{hint}</p>
                    if self.kind == HelperKind::Connection {
                        { self.link_form(ctx) }
                    }
                    if self.kind == HelperKind::Disconnect {
                        { self.disconnect_form(ctx) }
                    }
                    if self.kind == HelperKind::Flags {
                        { self.flags_form(ctx) }
                    }
                    if self.kind == HelperKind::CustomerNet {
                        { self.net_form(ctx) }
                    }
                    if self.kind == HelperKind::PortVlan {
                        { self.port_vlan_form(ctx) }
                    }
                    if self.kind == HelperKind::Poe {
                        { self.poe_form(ctx) }
                    }
                    if self.kind == HelperKind::RadioBridge {
                        { self.radio_form(ctx) }
                    }
                    if self.kind == HelperKind::RadioDisconnect {
                        <Form>
                            <Switch
                                label={t::RADIO_DISCONNECT_LINK}
                                checked={self.radio_disconnect_link}
                                onchange={ctx.link().callback(NetboxHelperMsg::RadioDisconnectLink)}
                            />
                        </Form>
                    }
                    {plan}
        </>};
        if ctx.props().embedded {
            return html!(<div class="netbox-helper">{content}</div>);
        }
        html! {
            <Card class="netbox-helper">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>{content}</CardBody>
            </Card>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        let phase = self.phase();
        if self.reported != Some(phase) {
            self.reported = Some(phase);
            ctx.props().on_phase.emit(phase);
        }
    }
}

impl NetboxHelper {
    /// Fetches what the chosen helper chooses from, unless it is there.
    fn load(&mut self, ctx: &Context<Self>) {
        let kind = self.kind;
        if kind.needs_ports() && self.ports.is_none() {
            fetch_ports(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::CustomerNet && self.net_options.is_none() {
            fetch_net_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::PortVlan && self.port_vlan_options.is_none() {
            fetch_port_vlan_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::Poe && self.poe_options.is_none() {
            fetch_poe_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::RadioBridge && self.radio_options.is_none() {
            fetch_radio_options(ctx.link().clone(), ctx.props().id);
        }
        if kind == HelperKind::Flags {
            if self.flag_interfaces.is_none() {
                fetch_flag_interfaces(ctx.link().clone(), ctx.props().id);
            }
            self.check_flags(ctx);
        }
    }
    fn phase(&self) -> Phase {
        let writing = self.written.is_some() && self.written == self.job;
        if writing && self.job_running {
            Phase::Writing
        } else if writing && self.write_done == Some(true) {
            Phase::Written
        } else if writing && self.write_done == Some(false) {
            Phase::WriteFailed
        } else if matches!(self.preview, Some(Ok(_))) {
            Phase::Preview
        } else {
            Phase::Plan
        }
    }
    /// The helper with its input as chosen now, `None` while ports are missing; a new one
    /// gets its plan.
    fn choice(&self) -> Option<HelperChoice> {
        match self.kind {
            HelperKind::Loopback => Some(HelperChoice::Loopback),
            HelperKind::Connection => Some(HelperChoice::Connection {
                interface: self.link.port?,
                peer: self.link.peer?,
                mtu: self.link.mtu.trim().parse().ok(),
            }),
            HelperKind::Disconnect => Some(HelperChoice::Disconnect(self.disconnect?)),
            HelperKind::BridgeLeave => Some(HelperChoice::BridgeLeave(self.bridge_leave?)),
            HelperKind::CustomerDisconnect => {
                Some(HelperChoice::CustomerDisconnect(self.customer_disconnect?))
            }
            HelperKind::PortVlan => Some(HelperChoice::PortVlan {
                port: self.port_vlan.port?,
                vlan: self.port_vlan.vlan?,
            }),
            HelperKind::CableTakeover => {
                let (port, peer) = self.cable_takeover?;
                Some(HelperChoice::CableTakeover { port, peer })
            }
            HelperKind::RadioDisconnect => Some(HelperChoice::RadioDisconnect {
                radio: self.radio_disconnect?,
                wireless_link: self.radio_disconnect_link,
            }),
            HelperKind::Flags => {
                let changes = self.flag_changes();
                (!changes.is_empty()).then_some(HelperChoice::Flags(changes))
            }
            HelperKind::CustomerNet => (self.net.parent.is_some() || self.net.parent_v6.is_some())
                .then_some(HelperChoice::CustomerNet {
                    target: self.net.target?,
                    parent: self.net.parent,
                    length: self.net.length.trim().trim_start_matches('/').parse().ok(),
                    parent_v6: self.net.parent_v6,
                    dhcp: self.net.dhcp,
                }),
            HelperKind::Poe => {
                let (own, peer) = (self.poe.port?, self.poe.peer?);
                let (pse, pd) = if self.poe.feeds { (own, peer) } else { (peer, own) };
                Some(HelperChoice::Poe { pse, pd })
            }
            HelperKind::RadioBridge => {
                let radio = &self.radio;
                let (own, peer) = (self.own_candidate()?.device.id as u32, radio.peer?);
                let address = |side: usize| {
                    Some(radio.address[side].trim().to_string()).filter(|a| !a.is_empty())
                };
                // this side first, the AP's first in the input
                let [mut a, mut b] = [0, 1].map(|side| (radio.router_port[side], address(side)));
                let (ap, station) = if radio.own_ap {
                    (own, peer)
                } else {
                    std::mem::swap(&mut a, &mut b);
                    (peer, own)
                };
                Some(HelperChoice::RadioBridge {
                    ap,
                    station,
                    ap_router_port: a.0,
                    station_router_port: b.0,
                    ap_address: a.1,
                    station_address: b.1,
                    ssid: Some(radio.ssid.trim().to_string()).filter(|s| !s.is_empty()),
                })
            }
        }
    }
    fn choose(&mut self, ctx: &Context<Self>) {
        let choice = self.choice();
        if choice == self.planned {
            return;
        }
        self.planned = choice.clone();
        self.plan = None;
        self.removed = false;
        self.preview = None;
        if let Some(choice) = choice {
            fetch_plan(ctx.link().clone(), ctx.props().id, choice);
        }
    }
}
