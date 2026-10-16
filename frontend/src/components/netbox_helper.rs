//! The helpers that complete NetBox for the device (`Query.netboxPlan`), one chosen at a time:
//! what it would write, on a click the preview (the findings and the script applying would
//! bring after it), then writing it as a job, and taking the last one back. They write to
//! NetBox only, the device gets it by applying. „Verbindung“ asks for the two ports first
//! (`linkPorts`); a port cabled to a router brings the other end along.
use crate::{
    components::{adjust_target::SelectedTarget, job::JobView},
    error::FrontendError,
    graphql::{
        authenticated::{
            HelperJobs, LinkPorts, NetboxPlan, NetboxPlanPreview, UndoNetboxWrite,
            WriteNetboxPlan,
            helper_jobs::{self, JobKind, JobState},
            link_ports::{self, LinkPortFields},
            netbox_plan::{self, NetboxPlanNetboxPlanSteps as Step},
            netbox_plan_preview, undo_netbox_write, write_netbox_plan,
        },
        query_authenticated,
    },
    i18n::helper as t,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode, Form, FormGroup, SimpleSelect, Spinner, SpinnerSize, TextInput,
};
use std::fmt::{self, Display, Formatter};
use yew::{Callback, Component, Context, Html, Properties, html, html::Scope, platform::spawn_local};

#[derive(Clone, PartialEq, Properties)]
pub struct NetboxHelperProps {
    pub id: u32,
    /// how the device is reached for the preview of the script
    pub target: SelectedTarget,
    /// NetBox was written to (or that was taken back): the page shows what changed
    #[prop_or_default]
    pub on_written: Callback<()>,
}

/// What the preview shows: the device's finding kinds before and after the plan, and the
/// script (`None`: the device isn't reachable).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    before: Vec<String>,
    after: Vec<String>,
    script: Option<String>,
}

/// The helper chosen, with what it needs.
#[derive(Debug, Clone, PartialEq)]
pub enum HelperChoice {
    Loopback,
    Connection {
        interface: u32,
        peer: u32,
        mtu: Option<u16>,
    },
}

/// The helper's input in the variables of `$module` (each query has its own types).
macro_rules! helper_input {
    ($module:ident, $choice:expr, $id:expr) => {
        match $choice {
            HelperChoice::Loopback => $module::Helper::Loopback($id as i64),
            HelperChoice::Connection {
                interface,
                peer,
                mtu,
            } => $module::Helper::Connection($module::ConnectionInput {
                interface: interface as i64,
                peer: peer as i64,
                mtu: mtu.map(i64::from),
            }),
        }
    };
}

/// Which helper the card shows.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HelperKind {
    Loopback,
    Connection,
}

impl Display for HelperKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HelperKind::Loopback => t::LOOPBACK,
            HelperKind::Connection => t::CONNECTION,
        })
    }
}

/// A port or device in a select: its id and how it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    id: u32,
    text: String,
}

impl Display for Choice {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The ends of a link being chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkChoice {
    port: Option<u32>,
    peer_device: Option<u32>,
    peer: Option<u32>,
    mtu: String,
}

pub struct NetboxHelper {
    kind: HelperKind,
    link: LinkChoice,
    ports: Option<Result<link_ports::ResponseData, FrontendError>>,
    /// what the plan shown was computed for
    planned: Option<HelperChoice>,
    plan: Option<Result<netbox_plan::ResponseData, FrontendError>>,
    preview: Option<Result<Preview, FrontendError>>,
    preview_loading: bool,
    /// the job shown, writing or undoing
    job: Option<i64>,
    job_running: bool,
    /// the write job whose objects "Rückgängig" deletes or sets back
    written: Option<i64>,
    error: Option<FrontendError>,
}

pub enum NetboxHelperMsg {
    Kind(HelperKind),
    Ports(Result<link_ports::ResponseData, FrontendError>),
    Port(Choice),
    PeerDevice(Choice),
    Peer(Choice),
    Mtu(String),
    Plan(HelperChoice, Result<netbox_plan::ResponseData, FrontendError>),
    LoadPreview,
    Preview(Result<Preview, FrontendError>),
    Write,
    Undo,
    /// a write job (`true`) or an undo job started
    JobStarted(Result<i64, FrontendError>, bool),
    JobFinished,
    /// the last write job of the device the server still knows, done and not taken back
    LastWrite(i64),
}

impl Component for NetboxHelper {
    type Message = NetboxHelperMsg;
    type Properties = NetboxHelperProps;

    fn create(ctx: &Context<Self>) -> Self {
        fetch_plan(ctx.link().clone(), ctx.props().id, HelperChoice::Loopback);
        last_write(ctx.link().clone(), ctx.props().id);
        Self {
            kind: HelperKind::Loopback,
            link: LinkChoice::default(),
            ports: None,
            planned: Some(HelperChoice::Loopback),
            plan: None,
            preview: None,
            preview_loading: false,
            job: None,
            job_running: false,
            written: None,
            error: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NetboxHelperMsg::Kind(kind) => {
                self.kind = kind;
                if kind == HelperKind::Connection && self.ports.is_none() {
                    fetch_ports(ctx.link().clone(), ctx.props().id);
                }
                self.choose(ctx);
            }
            NetboxHelperMsg::Ports(ports) => self.ports = Some(ports),
            NetboxHelperMsg::Port(port) => {
                self.link.port = Some(port.id);
                // a cable to a router: its other end
                let far = self
                    .port(port.id)
                    .and_then(|p| p.connected.first())
                    .map(|far| (far.device.id as u32, far.id as u32));
                if let Some((device, far)) = far {
                    self.link.peer_device = Some(device);
                    self.link.peer = Some(far);
                }
                self.choose(ctx);
            }
            NetboxHelperMsg::PeerDevice(device) => {
                if self.link.peer_device != Some(device.id) {
                    self.link.peer = None;
                }
                self.link.peer_device = Some(device.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::Peer(port) => {
                self.link.peer = Some(port.id);
                self.choose(ctx);
            }
            NetboxHelperMsg::Mtu(mtu) => {
                self.link.mtu = mtu;
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
            NetboxHelperMsg::JobFinished => {
                self.job_running = false;
                self.preview = None;
                if let Some(choice) = self.planned.clone() {
                    fetch_plan(ctx.link().clone(), ctx.props().id, choice);
                }
                // the ports have the link now
                if self.ports.is_some() {
                    fetch_ports(ctx.link().clone(), ctx.props().id);
                }
                ctx.props().on_written.emit(());
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let job = self.job.map(|id| html! {
            <JobView key={id} id={id} on_finished={ctx.link().callback(|_| NetboxHelperMsg::JobFinished)}/>
        });
        let error = self.error.as_ref().map(|error| error.to_html());
        let plan = match (&self.planned, &self.plan) {
            (None, _) => html!(<p>{t::CHOOSE_PORTS}</p>),
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
        };
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
        };
        html! {
            <Card class="netbox-helper">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    {error}
                    {job}
                    {undo}
                    <div class="helper-kind">
                        <SimpleSelect<HelperKind>
                            selected={Some(self.kind)}
                            entries={vec![HelperKind::Loopback, HelperKind::Connection]}
                            onselect={ctx.link().callback(NetboxHelperMsg::Kind)}
                        />
                    </div>
                    <p>{hint}</p>
                    if self.kind == HelperKind::Connection {
                        { self.link_form(ctx) }
                    }
                    {plan}
                </CardBody>
            </Card>
        }
    }
}

impl NetboxHelper {
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
        }
    }
    fn choose(&mut self, ctx: &Context<Self>) {
        let choice = self.choice();
        if choice == self.planned {
            return;
        }
        self.planned = choice.clone();
        self.plan = None;
        self.preview = None;
        if let Some(choice) = choice {
            fetch_plan(ctx.link().clone(), ctx.props().id, choice);
        }
    }
    fn ports(&self) -> Option<&link_ports::ResponseData> {
        self.ports.as_ref().and_then(|ports| ports.as_ref().ok())
    }
    fn port(&self, id: u32) -> Option<&LinkPortFields> {
        self.ports()?
            .topology
            .device_by_id
            .as_ref()?
            .link_ports
            .iter()
            .find(|port| port.id as u32 == id)
    }
    fn link_form(&self, ctx: &Context<Self>) -> Html {
        let ports = match &self.ports {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(ports)) => ports,
        };
        let choices = |ports: &[LinkPortFields]| -> Vec<Choice> {
            ports
                .iter()
                .map(|port| Choice {
                    id: port.id as u32,
                    text: t::link_port(
                        &port.name,
                        port.label.as_deref(),
                        port.connected
                            .first()
                            .map(|far| (far.device.name.as_str(), far.name.as_str())),
                        port.cabled,
                        port.ipv4.as_deref(),
                        port.bridge.as_deref(),
                    ),
                })
                .collect()
        };
        let own = ports
            .topology
            .device_by_id
            .as_ref()
            .map(|device| choices(&device.link_ports))
            .unwrap_or_default();
        let devices: Vec<Choice> = ports
            .topology
            .all_devices
            .iter()
            .filter(|device| device.id as u32 != ctx.props().id)
            .map(|device| Choice {
                id: device.id as u32,
                text: t::link_device(&device.name, device.site.as_ref().map(|s| s.name.as_str())),
            })
            .collect();
        let peer_ports = self
            .link
            .peer_device
            .and_then(|id| ports.topology.all_devices.iter().find(|d| d.id as u32 == id))
            .map(|device| choices(&device.link_ports))
            .unwrap_or_default();
        let selected = |entries: &[Choice], id: Option<u32>| {
            id.and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
        };
        let mtu = self.link.mtu.clone();
        html! {
            <Form>
                <FormGroup label={t::PORT}>
                    <SimpleSelect<Choice>
                        placeholder={t::CHOOSE_PORT.to_string()}
                        selected={selected(&own, self.link.port)}
                        entries={own.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::Port)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_DEVICE}>
                    <SimpleSelect<Choice>
                        placeholder={t::CHOOSE_DEVICE.to_string()}
                        selected={selected(&devices, self.link.peer_device)}
                        entries={devices.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::PeerDevice)}
                    />
                </FormGroup>
                <FormGroup label={t::PEER_PORT}>
                    <SimpleSelect<Choice>
                        placeholder={t::CHOOSE_PEER_PORT.to_string()}
                        selected={selected(&peer_ports, self.link.peer)}
                        entries={peer_ports.clone()}
                        onselect={ctx.link().callback(NetboxHelperMsg::Peer)}
                    />
                </FormGroup>
                <FormGroup label={t::MTU}>
                    <TextInput
                        value={mtu}
                        placeholder={t::MTU_PLACEHOLDER}
                        onchange={ctx.link().callback(NetboxHelperMsg::Mtu)}
                    />
                </FormGroup>
            </Form>
        }
    }
    fn preview_view(&self) -> Html {
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

fn describe(step: &Step) -> String {
    match step {
        Step::CreateInterface(create) => t::create_interface(&create.name, &create.kind),
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
            update.ospf,
        ),
        Step::SetPrimaryIp(set) => t::set_primary(
            match set.version {
                netbox_plan::IpVersion::V4 => "IPv4",
                netbox_plan::IpVersion::V6 => "IPv6",
                netbox_plan::IpVersion::Other(_) => "IP",
            },
            &set.address,
            set.before.as_deref(),
        ),
    }
}

/// The newest helper job of the device, if it is a finished write: it can still be taken back
/// (until the server restarts, which forgets its jobs).
fn last_write(scope: Scope<NetboxHelper>, id: u32) {
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

fn fetch_plan(scope: Scope<NetboxHelper>, id: u32, choice: HelperChoice) {
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

fn fetch_ports(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let ports = query_authenticated::<LinkPorts, _>(
            scope.clone(),
            link_ports::Variables { id: id as i64 },
        )
        .await;
        scope.send_message(NetboxHelperMsg::Ports(ports));
    });
}

fn fetch_preview(scope: Scope<NetboxHelper>, id: u32, choice: HelperChoice, target: SelectedTarget) {
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
