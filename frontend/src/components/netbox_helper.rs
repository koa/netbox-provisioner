//! A helper that completes NetBox for the device (`Query.netboxPlan`): what it would write,
//! on a click the preview (the findings and the script applying would bring after it), then
//! writing it as a job, and taking that back. It writes to NetBox only, the device gets it
//! by applying.
use crate::{
    components::{adjust_target::SelectedTarget, job::JobView},
    error::FrontendError,
    graphql::{
        authenticated::{
            HelperJobs, NetboxPlan, NetboxPlanPreview, UndoNetboxWrite, WriteNetboxPlan,
            helper_jobs::{self, JobKind, JobState},
            netbox_plan::{self, NetboxPlanNetboxPlanSteps as Step},
            netbox_plan_preview, undo_netbox_write, write_netbox_plan,
        },
        query_authenticated,
    },
    i18n::helper as t,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode, Spinner, SpinnerSize,
};
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

pub struct NetboxHelper {
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
    Plan(Result<netbox_plan::ResponseData, FrontendError>),
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

const HELPER: netbox_plan::Helper = netbox_plan::Helper::LOOPBACK;

impl Component for NetboxHelper {
    type Message = NetboxHelperMsg;
    type Properties = NetboxHelperProps;

    fn create(ctx: &Context<Self>) -> Self {
        fetch_plan(ctx.link().clone(), ctx.props().id);
        last_write(ctx.link().clone(), ctx.props().id);
        Self {
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
            NetboxHelperMsg::Plan(plan) => {
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
                fetch_preview(ctx.link().clone(), ctx.props().id, ctx.props().target.clone());
            }
            NetboxHelperMsg::Preview(preview) => {
                self.preview_loading = false;
                self.preview = Some(preview);
            }
            NetboxHelperMsg::Write => {
                let Some(Ok(plan)) = &self.plan else {
                    return false;
                };
                self.job_running = true;
                self.error = None;
                let scope = ctx.link().clone();
                let variables = write_netbox_plan::Variables {
                    id: ctx.props().id as i64,
                    helper: write_netbox_plan::Helper::LOOPBACK,
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
                fetch_plan(ctx.link().clone(), ctx.props().id);
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
        let plan = match &self.plan {
            None => html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => error.to_html(),
            // nothing to complete, and nothing written here to take back: no card
            Some(Ok(plan))
                if plan.netbox_plan.steps.is_empty() && self.job.is_none() && self.written.is_none() =>
            {
                return Html::default();
            }
            // what was written is shown with "Rückgängig"
            Some(Ok(plan)) if plan.netbox_plan.steps.is_empty() => Html::default(),
            Some(Ok(plan)) => {
                let allowed = plan.allow_netbox_write;
                html! {<>
                    <p>{t::LOOPBACK_HINT}</p>
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
        html! {
            <Card class="netbox-helper">
                <CardHeader><CardTitle>{format!("{}: {}", t::TITLE, t::LOOPBACK)}</CardTitle></CardHeader>
                <CardBody>
                    {error}
                    {job}
                    {undo}
                    {plan}
                </CardBody>
            </Card>
        }
    }
}

impl NetboxHelper {
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
            t::create_address(&create.address, &create.interface, &create.prefix)
        }
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

fn fetch_plan(scope: Scope<NetboxHelper>, id: u32) {
    spawn_local(async move {
        let plan = query_authenticated::<NetboxPlan, _>(
            scope.clone(),
            netbox_plan::Variables {
                id: id as i64,
                helper: HELPER,
            },
        )
        .await;
        scope.send_message(NetboxHelperMsg::Plan(plan));
    });
}

fn fetch_preview(scope: Scope<NetboxHelper>, id: u32, target: SelectedTarget) {
    spawn_local(async move {
        let (credential_name, adhoc_credentials) =
            target.credentials.variables(|username, password| {
                netbox_plan_preview::AdhocCredentials { username, password }
            });
        let preview = query_authenticated::<NetboxPlanPreview, _>(
            scope.clone(),
            netbox_plan_preview::Variables {
                id: id as i64,
                helper: netbox_plan_preview::Helper::LOOPBACK,
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
