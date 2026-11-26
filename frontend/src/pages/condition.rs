//! Scope: The admins' page „Zustand“ (`/condition`); nothing else.
//!
//! What to check or maintain in the network, most urgent first, each finding linking its
//! device; below, the routers' health and resources, shown, not judged
//! (`networkCondition`, `docs/concepts/monitoring.md`). Read once on opening and on „Neu lesen“:
//! it reads every router, up to 30 s old where the network map read it just before.
use crate::{
    components::device_link::DeviceLink,
    error::{FrontendError, ServerProblem},
    graphql::{
        authenticated::{
            condition::{Condition, ConditionFinding, ConditionKind, NetworkCondition},
            network_map::{MapEdgeKind, MapNodeDetails},
            types::{DeviceRef, InterfaceRef},
        },
        request,
    },
};
use cynic::QueryBuilder;
use i18n::{condition as t, network_map as map};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Content, Level, Spinner, SpinnerSize, Title,
};
use yew::{Component, Context, Html, html, platform::spawn_local};

pub struct ConditionPage {
    condition: Option<Result<NetworkCondition, FrontendError>>,
}

pub enum ConditionMsg {
    Load,
    Loaded(Result<NetworkCondition, FrontendError>),
}

impl Component for ConditionPage {
    type Message = ConditionMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(ConditionMsg::Load);
        Self { condition: None }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ConditionMsg::Load => {
                self.condition = None;
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let data = request(scope.clone(), Condition::build(())).await;
                    scope.send_message(ConditionMsg::Loaded(
                        data.map(|data| data.topology.network_condition),
                    ));
                });
            }
            ConditionMsg::Loaded(condition) => self.condition = Some(condition),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let reload = html! {
            <Button
                variant={ButtonVariant::Secondary}
                disabled={self.condition.is_none()}
                onclick={ctx.link().callback(|_| ConditionMsg::Load)}
            >{t::RELOAD}</Button>
        };
        let body = match &self.condition {
            None => html!(<Spinner size={SpinnerSize::Lg} aria_label={t::LOADING}/>),
            Some(Err(error)) => error.to_html(),
            Some(Ok(condition)) => view_condition(condition),
        };
        html! {
            <div class="condition-page">
                <Title level={Level::H1}>{t::TITLE}</Title>
                <Content><p>{t::INTRO}</p></Content>
                <p>{reload}</p>
                {body}
            </div>
        }
    }
}

fn view_condition(condition: &NetworkCondition) -> Html {
    let lsdb_error = condition.lsdb_error.as_ref().map(|error| {
        let text = t::lsdb_error(&ServerProblem::from(error).text());
        html!(<Alert inline=true r#type={AlertType::Warning} title={text}/>)
    });
    let findings = match condition.findings.is_empty() {
        true => html!(<Alert inline=true r#type={AlertType::Success} title={t::NOTHING}/>),
        false => html! {
            <table class="pf-v6-c-table pf-m-compact pf-m-grid-md condition-findings">
                <tbody class="pf-v6-c-table__tbody">
                    { for condition.findings.iter().map(view_finding) }
                </tbody>
            </table>
        },
    };
    html! {
        <>
            {lsdb_error}
            {findings}
            <Title level={Level::H2}>{t::DEVICES}</Title>
            <table class="pf-v6-c-table pf-m-compact pf-m-grid-md condition-devices">
                <tbody class="pf-v6-c-table__tbody">
                    { for condition.devices.iter().map(view_device) }
                </tbody>
            </table>
        </>
    }
}

fn device_link(device: &DeviceRef) -> Html {
    html! {
        <DeviceLink
            site={device.site.map(|site| site as u32)}
            id={device.id as u32}
            name={device.name.clone()}
        />
    }
}

fn port(interface: &InterfaceRef) -> String {
    format!("{} {}", interface.device.name, interface.name)
}

/// How urgent a finding is, as PatternFly's status colour.
fn status(kind: ConditionKind) -> &'static str {
    match kind {
        ConditionKind::Unreachable | ConditionKind::LinkDown | ConditionKind::NoDefaultRoute => {
            "danger"
        }
        ConditionKind::SignalBad => "danger",
        ConditionKind::LinkIpv6Down | ConditionKind::UplinkLoss | ConditionKind::SignalWeak => {
            "warning"
        }
        ConditionKind::Rebooted | ConditionKind::PoeNoLoad | ConditionKind::Other => "info",
    }
}

fn view_finding(finding: &ConditionFinding) -> Html {
    let text = match (finding.kind, &finding.link, &finding.uplink) {
        (ConditionKind::Unreachable, ..) => t::UNREACHABLE.to_string(),
        (ConditionKind::LinkDown, Some(link), _) => t::link_down(
            link.kind == MapEdgeKind::Radio,
            &port(&link.a),
            &port(&link.b),
        ),
        (ConditionKind::LinkIpv6Down, Some(link), _) => {
            t::link_ipv6_down(&port(&link.a), &port(&link.b))
        }
        (ConditionKind::NoDefaultRoute, ..) => t::NO_DEFAULT_ROUTE.to_string(),
        (ConditionKind::UplinkLoss, _, Some(uplink)) => {
            t::uplink_loss(uplink.loss, &uplink.gateway, uplink.rtt_ms)
        }
        (ConditionKind::SignalBad | ConditionKind::SignalWeak, ..) => match &finding.client {
            Some(client) => t::weak_signal(
                finding.kind == ConditionKind::SignalBad,
                client.hostname.as_deref().unwrap_or(&client.mac_address),
                &client.ssid,
                match client.access_point.is_empty() {
                    true => &client.interface,
                    false => &client.access_point,
                },
                client.signal,
            ),
            None => t::UNKNOWN.to_string(),
        },
        (ConditionKind::Rebooted, ..) => {
            t::rebooted(finding.uptime.unwrap_or_default() / 1_000_000_000)
        }
        (ConditionKind::PoeNoLoad, ..) => t::poe_no_load(
            finding
                .port
                .as_ref()
                .map(|port| port.name.as_str())
                .unwrap_or_default(),
        ),
        _ => t::UNKNOWN.to_string(),
    };
    let style = format!(
        "border-left: 4px solid var(--pf-t--global--color--status--{}--default)",
        status(finding.kind)
    );
    html! {
        <tr class="pf-v6-c-table__tr">
            <td class="pf-v6-c-table__td" {style}>{device_link(&finding.device)}</td>
            <td class="pf-v6-c-table__td">{text}</td>
        </tr>
    }
}

fn view_device(node: &MapNodeDetails) -> Html {
    let resources = node.resources.as_ref().map(|r| {
        map::resources(
            r.uptime / 1_000_000_000,
            r.cpu_load,
            r.free_memory.0,
            r.total_memory.0,
            r.free_hdd_space.0,
            &r.version,
        )
    });
    let health = node
        .health
        .iter()
        .map(|h| map::health_row(&h.name, &h.value, h.unit.as_deref()))
        .collect::<Vec<_>>()
        .join(", ");
    html! {
        <tr class="pf-v6-c-table__tr">
            <td class="pf-v6-c-table__td">{device_link(&node.device)}</td>
            <td class="pf-v6-c-table__td">{resources}</td>
            <td class="pf-v6-c-table__td">{health}</td>
        </tr>
    }
}
