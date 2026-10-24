//! The plan of the chosen helper (`netboxPlan`), its preview (`netboxPlanPreview`) and the last
//! write that can be taken back.
use super::{
    Flag, HelperChoice, HelperKind, NetTarget, NetboxHelper, NetboxHelperMsg, flags::tag_slug, t,
};
use crate::{
    components::adjust_target::SelectedTarget,
    graphql::{
        authenticated::{
            HelperJobs, NetboxPlan, NetboxPlanPreview,
            helper_jobs::{self, JobKind, JobState},
            netbox_plan::{self, NetboxPlanNetboxPlanSteps as Step},
            netbox_plan_preview,
        },
        query_authenticated,
    },
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, CodeBlock, CodeBlockCode, Spinner, SpinnerSize,
};
use yew::{Context, Html, html, html::Scope, platform::spawn_local};

/// What the preview shows: the device's finding kinds before and after the plan, and the
/// script (`None`: the device isn't reachable).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub(super) before: Vec<String>,
    pub(super) after: Vec<String>,
    pub(super) script: Option<String>,
}

impl NetboxHelper {
    pub(super) fn preview_view(&self) -> Html {
        if self.preview_loading {
            return html!(<Spinner size={SpinnerSize::Md}/>);
        }
        match &self.preview {
            None => Html::default(),
            Some(Err(error)) => error.to_html(),
            Some(Ok(preview)) => {
                let gone: Vec<&String> =
                    preview.before.iter().filter(|kind| !preview.after.contains(kind)).collect();
                html! {<div class="helper-preview">
                    if preview.after.is_empty() {
                        <p>{t::NO_FINDINGS_AFTER}</p>
                    } else {
                        <p>{t::FINDINGS_AFTER}{" "}{preview.after.join(", ")}</p>
                    }
                    if !gone.is_empty() {
                        <p>{t::findings_gone(&gone)}</p>
                    }
                    { match &preview.script {
                        None => html!(<p>{t::PREVIEW_UNREACHABLE}</p>),
                        Some(script) if script.trim().is_empty() => html!(<p>{t::PREVIEW_NOTHING}</p>),
                        Some(script) => html! {<>
                            <p>{t::PREVIEW_TITLE}</p>
                            <CodeBlock><CodeBlockCode>{script.clone()}</CodeBlockCode></CodeBlock>
                        </>},
                    }}
                </div>}
            }
        }
    }
}

pub(super) fn describe(step: &Step) -> String {
    match step {
        Step::CreateDevice(create) => t::create_device(
            &create.name,
            &create.device_type,
            &create.role,
            &create.site,
            create.wlan_group.as_deref(),
        ),
        Step::CreateInterface(create) => t::create_interface(
            &create.device.name,
            &create.name,
            &create.kind,
            &create.tags.iter().map(tag_slug).collect::<Vec<_>>(),
        ),
        Step::CreateAddress(create) => {
            t::create_address(&create.address, &create.device.name, &create.interface, &create.prefix)
        }
        Step::CreateCable(create) => t::create_cable(
            (&create.a.device.name, &create.a.name),
            (&create.b.device.name, &create.b.name),
        ),
        Step::CreatePrefix(create) => {
            t::create_prefix(&create.prefix, &create.pool, &create.description)
        }
        Step::UpdateInterface(update) => t::update_interface(
            (&update.interface.device.name, &update.interface.name),
            update
                .label
                .as_deref()
                .map(|label| (label, update.label_before.as_deref())),
            update.mtu.map(|mtu| (mtu, update.mtu_before)),
            &update.tags_added.iter().map(tag_slug).collect::<Vec<_>>(),
            &update.tags_removed.iter().map(tag_slug).collect::<Vec<_>>(),
            update.poe.as_ref().map(|poe| (plan_poe_mode(poe), update.poe_before.as_ref().map(plan_poe_mode))),
        ),
        Step::DeleteCable(delete) => t::delete_cable(
            (&delete.port.device.name, &delete.port.name),
            delete.far.as_ref().map(|far| (far.device.name.as_str(), far.name.as_str())),
        ),
        Step::DeleteAddress(delete) => t::delete_address(
            &delete.address,
            &delete.interface.device.name,
            &delete.interface.name,
        ),
        Step::DeletePrefix(delete) => t::delete_prefix(&delete.prefix),
        Step::UpdatePort(update) => t::update_port(
            (&update.interface.device.name, &update.interface.name),
            update.name.as_deref(),
            update
                .rf_role
                .as_ref()
                .map(|role| (rf_role(role), update.rf_role_before.as_ref().map(rf_role))),
            update.bridge.as_deref(),
            update.leaves_bridge.as_deref(),
        ),
        Step::CreateWirelessLink(create) => t::create_wireless_link(
            (&create.a.device.name, &create.a.name),
            (&create.b.device.name, &create.b.name),
            &create.ssid,
        ),
        Step::UpdatePortVlan(update) => {
            fn vlan(vlan: &netbox_plan::PlanVlan) -> (i64, &str) {
                (vlan.vid, vlan.name.as_str())
            }
            t::update_port_vlan(
                (&update.interface.device.name, &update.interface.name),
                update.vlan.as_ref().map(vlan),
                update.vlan_before.as_ref().map(vlan),
                &update.tagged_before.iter().map(vlan).collect::<Vec<_>>(),
            )
        }
        Step::DeleteInterface(delete) => {
            t::delete_interface((&delete.interface.device.name, &delete.interface.name))
        }
        Step::DeleteWirelessLink(delete) => t::delete_wireless_link(
            (&delete.a.device.name, &delete.a.name),
            (&delete.b.device.name, &delete.b.name),
        ),
        Step::UpdateWirelessLink(update) => t::update_wireless_link(
            (&update.a.device.name, &update.a.name),
            (&update.b.device.name, &update.b.name),
            update.ssid.as_deref().map(|ssid| (ssid, update.ssid_before.as_str())),
            update.psk_set,
        ),
        Step::SetPrimaryIp(set) => t::set_primary(
            match set.version {
                netbox_plan::IpVersion::V4 => "IPv4",
                netbox_plan::IpVersion::V6 => "IPv6",
                netbox_plan::IpVersion::Other(_) => "IP",
            },
            &set.device.name,
            &set.address,
            set.before.as_deref(),
        ),
    }
}

/// The newest helper job of the device, if it is a finished write: it can still be taken back
/// (until the server restarts, which forgets its jobs).
pub(super) fn last_write(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let Ok(data) = query_authenticated::<HelperJobs, _>(
            scope.clone(),
            helper_jobs::Variables { device_id: id as i64 },
        )
        .await
        else {
            return;
        };
        let last = data
            .jobs
            .into_iter()
            .find(|job| matches!(job.kind, JobKind::NETBOX_WRITE | JobKind::NETBOX_UNDO));
        if let Some(job) = last
            && matches!(job.kind, JobKind::NETBOX_WRITE)
            && matches!(job.state, JobState::DONE)
        {
            scope.send_message(NetboxHelperMsg::LastWrite(job.id));
        }
    });
}

pub(super) fn fetch_plan(scope: Scope<NetboxHelper>, id: u32, choice: HelperChoice) {
    spawn_local(async move {
        let plan = query_authenticated::<NetboxPlan, _>(
            scope.clone(),
            netbox_plan::Variables {
                helper: helper_input!(netbox_plan, choice.clone(), id),
            },
        )
        .await;
        scope.send_message(NetboxHelperMsg::Plan(choice, plan));
    });
}

pub(super) fn fetch_preview(scope: Scope<NetboxHelper>, id: u32, choice: HelperChoice, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| {
                netbox_plan_preview::AdhocCredentials { username, password }
            });
        let preview = query_authenticated::<NetboxPlanPreview, _>(
            scope.clone(),
            netbox_plan_preview::Variables {
                id: id as i64,
                helper: helper_input!(netbox_plan_preview, choice, id),
                target: target.address.map(|a| a.to_string()),
                credential_name,
                adhoc_credentials,
            },
        )
        .await
        .map(|data| {
            let kinds = |findings: Vec<String>| findings.into_iter().map(t::finding_kind).collect();
            Preview {
                before: kinds(
                    data.topology
                        .device_by_id
                        .map(|d| d.findings.into_iter().map(|f| format!("{:?}", f.kind)).collect())
                        .unwrap_or_default(),
                ),
                after: kinds(
                    data.netbox_plan
                        .preview
                        .findings
                        .into_iter()
                        .map(|f| format!("{:?}", f.kind))
                        .collect(),
                ),
                script: data
                    .netbox_plan
                    .preview_access
                    .and_then(|access| access.generate_cfg)
                    .map(|cfg| cfg.script),
            }
        });
        scope.send_message(NetboxHelperMsg::Preview(preview));
    });
}

fn rf_role(role: &netbox_plan::RadioRole) -> &str {
    match role {
        netbox_plan::RadioRole::AP => "AP",
        netbox_plan::RadioRole::STATION => "Station",
        netbox_plan::RadioRole::Other(other) => other,
    }
}

fn plan_poe_mode(mode: &netbox_plan::PoeMode) -> &str {
    match mode {
        netbox_plan::PoeMode::PSE => "PSE",
        netbox_plan::PoeMode::PD => "PD",
        netbox_plan::PoeMode::Other(other) => other,
    }
}

impl NetboxHelper {
    pub(super) fn plan_view(&self, ctx: &Context<Self>) -> Html {
        match (&self.planned, &self.plan) {
            (None, _) if self.kind == HelperKind::Disconnect => {
                html!(<p>{t::CHOOSE_DISCONNECT_PORT}</p>)
            }
            (None, _) if self.kind == HelperKind::Flags => html!(<p>{t::CHOOSE_FLAGS}</p>),
            // a port's row gave the port: only the rest is left to choose
            (None, _) if self.kind == HelperKind::CustomerNet && self.fixed_port => {
                html!(<p>{t::CHOOSE_NET_PREFIX}</p>)
            }
            (None, _) if self.kind == HelperKind::CustomerNet => html!(<p>{t::CHOOSE_NET}</p>),
            (None, _) if self.kind == HelperKind::Poe && self.fixed_port => {
                html!(<p>{t::CHOOSE_POE_PEER}</p>)
            }
            (None, _) if self.kind == HelperKind::Poe => html!(<p>{t::CHOOSE_POE}</p>),
            (None, _) if self.kind == HelperKind::RadioBridge => {
                html!(<p>{t::CHOOSE_RADIO_BRIDGE}</p>)
            }
            (None, _) if self.kind == HelperKind::RadioDisconnect => {
                html!(<p>{t::CHOOSE_RADIO_DISCONNECT}</p>)
            }
            (None, _) if self.kind == HelperKind::CustomerDisconnect => {
                html!(<p>{t::CHOOSE_CUSTOMER_DISCONNECT}</p>)
            }
            (None, _) if self.kind == HelperKind::PortVlan && self.fixed_port => {
                html!(<p>{t::CHOOSE_PORT_VLAN_GROUP_VLAN}</p>)
            }
            (None, _) if self.kind == HelperKind::PortVlan => html!(<p>{t::CHOOSE_PORT_VLAN}</p>),
            (None, _) if self.kind == HelperKind::BridgeLeave => {
                html!(<p>{t::CHOOSE_BRIDGE_LEAVE}</p>)
            }
            (None, _) if self.kind == HelperKind::AddressTakeover => {
                html!(<p>{t::CHOOSE_ADDRESS_TAKEOVER}</p>)
            }
            (None, _) if self.kind == HelperKind::NewDevice => {
                html!(<p>{t::CHOOSE_NEW_DEVICE_NAME}</p>)
            }
            (None, _) if self.fixed_port => html!(<p>{t::CHOOSE_PEER}</p>),
            (None, _) => html!(<p>{t::CHOOSE_PORTS}</p>),
            (_, None) if self.removed => html!(),
            (_, None) => html!(<Spinner size={SpinnerSize::Md}/>),
            (_, Some(Err(error))) => error.to_html(),
            (_, Some(Ok(plan))) if plan.netbox_plan.steps.is_empty() => html!(<p>{t::COMPLETE}</p>),
            (_, Some(Ok(plan))) => {
                let allowed = plan.allow_netbox_write;
                html! {<>
                    <ol class="helper-steps">
                        { for plan.netbox_plan.steps.iter().map(|step| html!(<li>{describe(step)}</li>)) }
                    </ol>
                    if let Some(router) = &plan.netbox_plan.checked_with {
                        <p>{t::checked_with(router)}</p>
                    }
                    if plan.netbox_plan.steps.iter().any(|step| matches!(step, Step::DeleteAddress(_))) {
                        <p>{t::ADDRESSES_GO}</p>
                    }
                    if !plan.netbox_plan.cut_off.is_empty() {
                        <Alert inline=true title={t::CUT_OFF} r#type={AlertType::Warning}>
                            <ul>
                            { for plan.netbox_plan.cut_off.iter().map(|device| html!(<li>{&device.name}</li>)) }
                            </ul>
                        </Alert>
                    }
                    if let Some(bridge) = &plan.netbox_plan.emptied_bridge {
                        <Alert inline=true title={t::emptied_bridge(&bridge.name)} r#type={AlertType::Warning}>
                            <p>{t::EMPTIED_BRIDGE_TEXT}</p>
                        </Alert>
                    }
                    if !plan.netbox_plan.apply_together.is_empty() {
                        <Alert inline=true title={t::APPLY_TOGETHER} r#type={AlertType::Warning}>
                            <ul>
                            { for plan.netbox_plan.apply_together.iter().map(|device| html!(<li>{&device.name}</li>)) }
                            </ul>
                        </Alert>
                    }
                    if !plan.netbox_plan.undocumented.is_empty() {
                        <Alert inline=true title={t::UNDOCUMENTED} r#type={AlertType::Warning}>
                            <ul>
                            { for plan.netbox_plan.undocumented.iter().map(|taken| html!(
                                <li>{t::undocumented(&taken.address, &taken.route, taken.gateway.as_deref(), &taken.router)}</li>
                            ))}
                            </ul>
                        </Alert>
                    }
                    { self.preview_view() }
                    if !allowed {
                        <Alert inline=true title={t::LOCKED} r#type={AlertType::Info}/>
                    }
                    <div class="helper-actions">
                        <Button
                            variant={ButtonVariant::Secondary}
                            label={t::PREVIEW}
                            disabled={self.preview_loading || self.job_running}
                            onclick={ctx.link().callback(|_| NetboxHelperMsg::LoadPreview)}
                        />
                        {" "}
                        <Button
                            variant={ButtonVariant::Primary}
                            label={t::WRITE}
                            disabled={!allowed || self.job_running}
                            onclick={ctx.link().callback(|_| NetboxHelperMsg::Write)}
                        />
                    </div>
                </>}
            }
        }
    }
}
