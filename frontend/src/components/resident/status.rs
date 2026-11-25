//! Scope: The one status label of each element of a resident's view (port, device, network)
//! and the summary on top: its rollout from the saved changes, else what is up from the live
//! state; not the label of a draft (`card_header`), nor reading them (`components::resident`).
//!
//! An element's rollout is that of the newest saved change touching it (`touched`, a device
//! also where it is one of the change's devices): „wird ausgerollt“ while a device is pending,
//! else „nicht ausgerollt“ where one failed, else „wartet“; all rolled out, the live state
//! decides. A text stands only where something isn't ok.
use super::ports::{sub, title};
use crate::graphql::resident::{
    changes::{ChangeDevice, ChangeState, HoldReason, RolloutState},
    live::{LiveState, PortLive},
    residents::{PortKind, ResidentDevice, ResidentNetwork, ResidentPort},
};
use i18n::{
    auto_rollout as rollout,
    resident::{self as t, StatusLabel},
};
use patternfly_yew::prelude::{Alert, AlertType, Color, Label};
use yew::{Html, html};

/// An element's label, with the text saying what isn't ok.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub label: StatusLabel,
    pub text: Option<String>,
}

impl Status {
    fn new(label: StatusLabel, text: impl Into<Option<String>>) -> Self {
        Status {
            label,
            text: text.into(),
        }
    }
    /// its label, as on a card's header
    pub fn label(&self) -> Html {
        let color = match self.label {
            StatusLabel::Ok => Color::Green,
            StatusLabel::Off => Color::Grey,
            StatusLabel::Rolling | StatusLabel::Waiting => Color::Blue,
            StatusLabel::Failed => Color::Red,
            StatusLabel::Down | StatusLabel::Limited => Color::Orange,
        };
        html!(<Label label={t::status_label(self.label)} {color} compact=true/>)
    }
    /// its text, at the end of a card's body
    pub fn text(&self) -> Option<Html> {
        let text = self.text.as_ref()?;
        let class = match self.label {
            StatusLabel::Failed => "pf-v6-c-helper-text__item pf-m-error",
            StatusLabel::Rolling => "pf-v6-c-helper-text__item pf-m-indeterminate",
            _ => "pf-v6-c-helper-text__item pf-m-warning",
        };
        Some(html! {
            <div class="pf-v6-c-helper-text">
                <div {class}><span class="pf-v6-c-helper-text__item-text">{text}</span></div>
            </div>
        })
    }
}

/// A line of the summary on top.
struct Issue {
    kind: AlertType,
    text: String,
}

/// What the labels are made of: the saved changes, the newest first, and what is up.
#[derive(Clone, Copy)]
pub struct State<'a> {
    pub changes: &'a [ChangeState],
    pub live: Option<&'a LiveState>,
}

/// How a device stands with a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stand {
    Done,
    Rolling,
    Waiting,
    Failed,
}

fn stand(device: &ChangeDevice) -> Stand {
    match device.state {
        RolloutState::RolledOut => Stand::Done,
        RolloutState::Pending => Stand::Rolling,
        RolloutState::Failed => Stand::Failed,
        RolloutState::Waiting
        | RolloutState::Deferred
        | RolloutState::Manual
        | RolloutState::Other(_) => Stand::Waiting,
    }
}

/// The names of `devices` standing so.
fn names(devices: &[ChangeDevice], so: Stand) -> String {
    devices
        .iter()
        .filter(|device| stand(device) == so)
        .map(|device| device.device.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Waiting for an admin rather than for a device to come back.
fn for_admin(device: &ChangeDevice) -> bool {
    !device.reasons.is_empty() || device.state == RolloutState::Manual
}

/// The rollout of a change over all its devices; none where all have it.
fn change_rollout(change: &ChangeState) -> Option<Status> {
    let devices = &change.devices;
    let has = |so: Stand| devices.iter().any(|device| stand(device) == so);
    if has(Stand::Rolling) {
        let done = devices.iter().filter(|d| stand(d) == Stand::Done).count();
        return Some(Status::new(
            StatusLabel::Rolling,
            t::rolling_out(done, devices.len()),
        ));
    }
    if has(Stand::Failed) {
        return Some(Status::new(
            StatusLabel::Failed,
            t::not_rolled_out(&names(devices, Stand::Failed)),
        ));
    }
    if has(Stand::Waiting) {
        let waiting = |which: fn(&ChangeDevice) -> bool| {
            devices
                .iter()
                .filter(|device| stand(device) == Stand::Waiting && which(device))
                .collect::<Vec<_>>()
        };
        // an admin first: the others wait for the round it holds back
        let admin = waiting(for_admin);
        let unreached = waiting(|device| device.state == RolloutState::Deferred);
        let text = if !admin.is_empty() {
            t::waiting_for_admin(&join_names(&admin), &reasons(&admin))
        } else if !unreached.is_empty() {
            t::waiting_for(&join_names(&unreached))
        } else {
            t::WAITING_FOR_ROUND.to_string()
        };
        return Some(Status::new(StatusLabel::Waiting, text));
    }
    None
}

fn join_names(devices: &[&ChangeDevice]) -> String {
    devices
        .iter()
        .map(|device| device.device.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn reasons(devices: &[&ChangeDevice]) -> String {
    let mut reasons = devices
        .iter()
        .flat_map(|device| &device.reasons)
        .map(hold_reason)
        .collect::<Vec<_>>();
    reasons.dedup();
    reasons.join("; ")
}

/// Why a device holds a change back, as the resident reads it: a radio link as a complex
/// change for support (Z58), the others as the site page says them.
pub fn hold_reason(reason: &HoldReason) -> String {
    rollout::hold_reason(match reason {
        HoldReason::RadioLink => return t::RADIO_LINK_HOLD.to_string(),
        HoldReason::PoeCut => rollout::HoldReason::PoeCut,
        HoldReason::CapOff => rollout::HoldReason::CapOff,
        HoldReason::ApiClientsOutside => rollout::HoldReason::ApiClientsOutside,
        HoldReason::Rewire => rollout::HoldReason::Rewire,
        HoldReason::Outward => rollout::HoldReason::Outward,
        HoldReason::Other(other) => rollout::HoldReason::Other(other),
    })
}

/// A port in a text: a socket by its name, another port by itself and its device („Port ether3
/// an lab-np2“), as its label names the device at it.
fn place(port: &ResidentPort) -> String {
    match port.kind {
        PortKind::Socket => title(port),
        _ => sub(port),
    }
}

fn same_kind(a: &PortKind, b: &PortKind) -> bool {
    matches!(
        (a, b),
        (PortKind::Socket, PortKind::Socket) | (PortKind::Interface, PortKind::Interface)
    )
}

impl State<'_> {
    /// The newest change `touches` says touched the element.
    fn newest(&self, touches: impl Fn(&ChangeState) -> bool) -> Option<&ChangeState> {
        self.changes.iter().find(|change| touches(change))
    }

    /// A port or socket: its change's rollout, else whether its switch and its own device are
    /// up; a free one without network is off; none where neither is judged (a switch without
    /// OSPF of its own).
    pub fn port(&self, port: &ResidentPort, occupant: Option<&ResidentDevice>) -> Option<Status> {
        let rollout = self
            .newest(|change| {
                change
                    .touched
                    .ports
                    .iter()
                    .any(|touched| touched.id == port.id && same_kind(&touched.kind, &port.kind))
            })
            .and_then(change_rollout);
        if rollout.is_some() {
            return rollout;
        }
        let switch = self.port_live(port);
        if let Some(switch) = switch.filter(|switch| switch.reached == Some(false)) {
            return Some(Status::new(
                StatusLabel::Down,
                t::switch_unreachable(&switch.switch.name),
            ));
        }
        let link = switch.and_then(|switch| switch.link);
        if link == Some(false) {
            let socket = port.kind == PortKind::Socket;
            return Some(Status::new(
                StatusLabel::Down,
                t::no_link(socket).to_string(),
            ));
        }
        if let Some(device) = occupant.filter(|device| self.reached(device.id) == Some(false)) {
            return Some(Status::new(
                StatusLabel::Down,
                t::device_silent(&device.name, link == Some(true)),
            ));
        }
        if port.far_ends.is_empty() && port.network.is_none() && !port.foreign_network {
            return Some(Status::new(StatusLabel::Off, None));
        }
        // ok only where something behind it is judged: its switch, else its own device
        let judged =
            switch.is_some() || occupant.is_some_and(|device| self.reached(device.id).is_some());
        judged.then(|| Status::new(StatusLabel::Ok, None))
    }

    /// What the live state says of `port`: its switch reached, its link.
    fn port_live(&self, port: &ResidentPort) -> Option<&PortLive> {
        self.live
            .filter(|live| live.known)?
            .ports
            .iter()
            .find(|at| at.id == port.id && same_kind(&at.kind, &port.kind))
    }

    /// One of the resident's devices: its own rollout of the newest change it is in, else
    /// whether it is up; `upstream` is the device its cable leads to, `at` the resident's port
    /// it comes from.
    pub fn device(
        &self,
        device: &ResidentDevice,
        upstream: Option<&ResidentDevice>,
        at: Option<&ResidentPort>,
    ) -> Option<Status> {
        let change = self.newest(|change| {
            change.devices.iter().any(|d| d.device.id == device.id)
                || change.touched.devices.contains(&device.id)
        });
        let own = change.and_then(|change| {
            change
                .devices
                .iter()
                .find(|d| d.device.id == device.id)
                .map(|own| match stand(own) {
                    Stand::Done => None,
                    Stand::Rolling => Some(Status::new(
                        StatusLabel::Rolling,
                        t::DEVICE_ROLLING.to_string(),
                    )),
                    Stand::Failed => Some(Status::new(
                        StatusLabel::Failed,
                        t::DEVICE_FAILED.to_string(),
                    )),
                    Stand::Waiting if for_admin(own) => Some(Status::new(
                        StatusLabel::Waiting,
                        t::waiting_for_admin(&own.device.name, &reasons(&[own])),
                    )),
                    Stand::Waiting if own.state == RolloutState::Deferred => Some(Status::new(
                        StatusLabel::Waiting,
                        t::DEVICE_WAITING.to_string(),
                    )),
                    Stand::Waiting => Some(Status::new(
                        StatusLabel::Waiting,
                        t::WAITING_FOR_ROUND.to_string(),
                    )),
                })
        });
        let rollout = match own {
            Some(own) => own,
            None => change.and_then(change_rollout),
        };
        if rollout.is_some() {
            return rollout;
        }
        match self.reached(device.id)? {
            true => Some(Status::new(StatusLabel::Ok, None)),
            false => {
                let no_link = at
                    .filter(|port| self.port_live(port).and_then(|live| live.link) == Some(false));
                let text = match (no_link, upstream) {
                    (Some(port), _) => t::no_link_at(&place(port)),
                    (None, Some(up)) if self.reached(up.id) == Some(false) => {
                        t::unreachable_behind(&up.name)
                    }
                    _ => t::UNREACHABLE.to_string(),
                };
                Some(Status::new(StatusLabel::Down, text))
            }
        }
    }

    /// A network: the rollout of the newest change to one of its WLANs, else whether its
    /// WLANs' access points are up.
    pub fn network(&self, network: &ResidentNetwork) -> Option<Status> {
        let rollout = self
            .newest(|change| {
                network
                    .wlans
                    .iter()
                    .any(|wlan| change.touched.wlans.contains(&wlan.id))
            })
            .and_then(change_rollout);
        if rollout.is_some() {
            return rollout;
        }
        if !network.wlans.is_empty() && network.wlans.iter().all(|wlan| !wlan.enabled) {
            return Some(Status::new(StatusLabel::Off, None));
        }
        let live = self.live.filter(|live| live.known)?;
        for wlan in network.wlans.iter().filter(|wlan| wlan.enabled) {
            let Some(at) = live.wlans.iter().find(|at| at.wlan == wlan.id) else {
                continue;
            };
            if at.unreached.is_empty() {
                continue;
            }
            let of = at.access_points.max(0) as usize;
            let on = of.saturating_sub(at.unreached.len());
            let unreached = at
                .unreached
                .iter()
                .map(|ap| ap.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Some(match on {
                0 => Status::new(StatusLabel::Down, t::WLAN_DOWN.to_string()),
                _ => Status::new(StatusLabel::Limited, t::wlan_limited(on, of, &unreached)),
            });
        }
        Some(Status::new(StatusLabel::Ok, None))
    }

    /// Whether the routers reach the resident's device `id`; none where it isn't judged.
    fn reached(&self, id: i32) -> Option<bool> {
        self.live
            .filter(|live| live.known)?
            .devices
            .iter()
            .find(|device| device.device.id == id)
            .map(|device| device.reached)
    }

    /// The summary on top: its issues, the worst first; „Alles ok“ where none, once the live
    /// state is known.
    pub fn summary(&self, networks: &[ResidentNetwork], ports: &[ResidentPort]) -> Html {
        let mut issues = self.issues(networks, ports);
        if issues.is_empty() {
            return match self.live.filter(|live| live.known) {
                Some(_) => html! {
                    <Alert inline=true r#type={AlertType::Success} title={t::ALL_OK}>
                        <p>{t::ALL_OK_HINT}</p>
                    </Alert>
                },
                None => Html::default(),
            };
        }
        // the worst first
        let rank = |kind: &AlertType| match kind {
            AlertType::Danger => 0,
            AlertType::Warning => 1,
            _ => 2,
        };
        issues.sort_by_key(|issue| rank(&issue.kind));
        let kind = issues[0].kind;
        html! {
            <Alert inline=true r#type={kind} title={t::NOT_ALL_OK}>
                <ul>
                    { for issues.into_iter().map(|issue| html!(<li>{issue.text}</li>)) }
                </ul>
            </Alert>
        }
    }

    /// The lines of the summary: each change not through yet, each device not reached, each
    /// port without link where NetBox has a cable, each WLAN on part of its access points.
    fn issues(&self, networks: &[ResidentNetwork], ports: &[ResidentPort]) -> Vec<Issue> {
        let mut issues = Vec::new();
        for change in self.changes {
            let Some(status) = change_rollout(change) else {
                continue;
            };
            let head = t::change_head(&rollout::ago(change.age.into()), &change.by);
            issues.push(Issue {
                kind: match status.label {
                    StatusLabel::Failed => AlertType::Danger,
                    StatusLabel::Rolling => AlertType::Info,
                    _ => AlertType::Warning,
                },
                text: t::summary_change(&head, status.text.as_deref().unwrap_or_default()),
            });
        }
        let live = self.live.filter(|live| live.known);
        for device in live
            .iter()
            .flat_map(|live| &live.devices)
            .filter(|device| !device.reached)
        {
            issues.push(Issue {
                kind: AlertType::Warning,
                text: t::summary_unreachable(&device.device.name),
            });
        }
        for port in ports
            .iter()
            .filter(|port| self.port_live(port).and_then(|live| live.link) == Some(false))
        {
            issues.push(Issue {
                kind: AlertType::Warning,
                text: t::summary_no_link(&place(port)),
            });
        }
        for wlan in networks.iter().flat_map(|network| &network.wlans) {
            let Some(at) = live
                .iter()
                .flat_map(|live| &live.wlans)
                .find(|at| at.wlan == wlan.id && wlan.enabled && !at.unreached.is_empty())
            else {
                continue;
            };
            let of = at.access_points.max(0) as usize;
            issues.push(Issue {
                kind: AlertType::Warning,
                text: t::summary_wlan(&wlan.ssid, of.saturating_sub(at.unreached.len()), of),
            });
        }
        issues
    }
}

#[cfg(test)]
mod test;
