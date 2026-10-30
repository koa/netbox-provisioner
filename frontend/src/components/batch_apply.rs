//! Applying the config to a device together with others (`applyBatch`): the other ends of its
//! wireless links are picked, more devices of the sites can be added, and each picked device
//! shows its planned changes before anything is applied.
use crate::{
    error::{FrontendError, ServerProblem},
    graphql::{
        authenticated::{
            ApplyBatch, BatchCandidates, BatchDevice, apply_batch, batch_candidates, batch_device,
        },
        query_authenticated, query_authenticated_response,
    },
    i18n::{batch as t, device},
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode, Spinner, SpinnerSize,
};
use std::collections::{BTreeSet, HashMap};
use yew::{Callback, Component, Context, Html, Properties, html, html::Scope, platform::spawn_local};
use yew_nested_router::components::Link;

type Candidate = batch_candidates::BatchCandidatesTopologyDeviceByIdBatchCandidates;
type Warning = batch_device::BatchDeviceTopologyDeviceByIdAccessGenerateCfgWarnings;

/// A job of a started batch, as the device page shows it next to the others.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchJob {
    pub job: i64,
    pub device: i64,
    pub name: String,
    /// for the link to the device's page; `None` for the page's own device
    pub site: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct BatchApplyProps {
    /// the device whose page this is
    pub id: u32,
    /// a job runs, or writing is locked
    pub disabled: bool,
    /// the jobs of all devices of the batch, this one first, once it started
    pub on_started: Callback<Vec<BatchJob>>,
}

/// What a picked device would get.
#[derive(Debug, Clone, PartialEq)]
pub enum Planned {
    Loading,
    /// the planned changes (empty: in line) and the warnings
    Ready { script: String, warnings: Vec<String>, connected: Option<String> },
    /// why it can't be applied
    Problem(String),
}

pub struct BatchApply {
    open: bool,
    candidates: Option<Result<Vec<Candidate>, String>>,
    picked: BTreeSet<i64>,
    planned: HashMap<i64, Planned>,
    starting: bool,
    error: Option<FrontendError>,
}

pub enum BatchMessage {
    Open,
    Candidates(Result<Vec<Candidate>, String>),
    Toggle(i64),
    Planned(i64, Planned),
    Apply,
    Started(Result<Vec<(i64, i64, String)>, FrontendError>),
}

impl Component for BatchApply {
    type Message = BatchMessage;
    type Properties = BatchApplyProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            open: false,
            candidates: None,
            picked: BTreeSet::new(),
            planned: HashMap::new(),
            starting: false,
            error: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            BatchMessage::Open => {
                self.open = true;
                fetch_candidates(ctx.link().clone(), ctx.props().id);
            }
            BatchMessage::Candidates(candidates) => {
                if let Ok(candidates) = &candidates {
                    for candidate in candidates.iter().filter(|c| c.partner) {
                        self.pick(ctx, candidate.device.id);
                    }
                }
                self.candidates = Some(candidates);
            }
            BatchMessage::Toggle(id) => {
                if !self.picked.remove(&id) {
                    self.pick(ctx, id);
                }
            }
            BatchMessage::Planned(id, planned) => {
                self.planned.insert(id, planned);
            }
            BatchMessage::Apply => {
                self.starting = true;
                self.error = None;
                let ids: Vec<i64> = std::iter::once(ctx.props().id as i64)
                    .chain(self.picked.iter().copied())
                    .collect();
                start(ctx.link().clone(), ids);
            }
            BatchMessage::Started(result) => {
                self.starting = false;
                match result {
                    Ok(jobs) => {
                        let own = ctx.props().id as i64;
                        let site = |device: i64| {
                            self.candidates
                                .as_ref()
                                .and_then(|c| c.as_ref().ok())
                                .and_then(|c| c.iter().find(|c| c.device.id == device))
                                .and_then(|c| c.device.site.as_ref())
                                .map(|site| site.id)
                        };
                        let mut jobs: Vec<BatchJob> = jobs
                            .into_iter()
                            .map(|(job, device, name)| BatchJob { job, device, name, site: site(device) })
                            .collect();
                        jobs.sort_by_key(|job| job.device != own);
                        // the jobs show on the page, next to each other
                        self.open = false;
                        ctx.props().on_started.emit(jobs);
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let disabled = ctx.props().disabled || self.starting;
        if !self.open {
            return html! {
                <div class="batch-apply-open">
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::OPEN}
                        disabled={disabled}
                        onclick={ctx.link().callback(|_| BatchMessage::Open)}
                    />
                </div>
            };
        }
        let candidates = match &self.candidates {
            None => html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => html!(<Alert inline=true r#type={AlertType::Danger} title={error.clone()}/>),
            Some(Ok(candidates)) if candidates.is_empty() => html!(<p>{t::NO_CANDIDATES}</p>),
            Some(Ok(candidates)) => html! {
                <ul class="batch-candidates">
                    { for candidates.iter().map(|c| self.candidate_view(ctx, c)) }
                </ul>
            },
        };
        let ready = !self.picked.is_empty()
            && self
                .picked
                .iter()
                .all(|id| matches!(self.planned.get(id), Some(Planned::Ready { .. })));
        let count = self.picked.len() + 1;
        html! {
            <Card class="batch-apply">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{t::EXPLANATION}</p>
                    {candidates}
                    if let Some(error) = &self.error {
                        {error.to_html()}
                    }
                    <Button
                        variant={ButtonVariant::Primary}
                        label={t::apply(count)}
                        disabled={disabled || !ready}
                        onclick={ctx.link().callback(|_| BatchMessage::Apply)}
                    />
                    if !self.picked.is_empty() && !ready {
                        <p class="batch-not-ready">{t::NOT_READY}</p>
                    }
                </CardBody>
            </Card>
        }
    }
}

impl BatchApply {
    fn pick(&mut self, ctx: &Context<Self>, id: i64) {
        self.picked.insert(id);
        if !matches!(self.planned.get(&id), Some(Planned::Ready { .. } | Planned::Loading)) {
            self.planned.insert(id, Planned::Loading);
            fetch_planned(ctx.link().clone(), id);
        }
    }

    fn candidate_view(&self, ctx: &Context<Self>, candidate: &Candidate) -> Html {
        let id = candidate.device.id;
        let picked = self.picked.contains(&id);
        let site = candidate.device.site.as_ref();
        let name = html! {
            if let Some(site) = site {
                <Link<AppRoute> to={AppRoute::device(site.id as u32, id as u32)}>{&candidate.device.name}</Link<AppRoute>>
            } else {
                {&candidate.device.name}
            }
        };
        let planned = picked.then(|| match self.planned.get(&id) {
            None | Some(Planned::Loading) => html!(<Spinner size={SpinnerSize::Sm}/>),
            Some(Planned::Problem(problem)) => html!(
                <Alert inline=true r#type={AlertType::Danger} title={t::unreachable(problem)}/>
            ),
            Some(Planned::Ready { script, warnings, connected }) => html! {<>
                if let Some(connected) = connected {
                    <p class="batch-connected">{connected.clone()}</p>
                }
                { for warnings.iter().map(|w| html!(<Alert inline=true r#type={AlertType::Warning} title={w.clone()}/>)) }
                if script.trim().is_empty() {
                    <p>{t::IN_LINE}</p>
                } else {
                    <CodeBlock><CodeBlockCode>{script.clone()}</CodeBlockCode></CodeBlock>
                }
            </>},
        });
        html! {
            <li>
                <label>
                    <input
                        type="checkbox"
                        checked={picked}
                        onchange={ctx.link().callback(move |_| BatchMessage::Toggle(id))}
                    />
                    {" "}{name}
                    if let Some(site) = site {
                        {" "}<span class="batch-site">{format!("({})", site.name)}</span>
                    }
                    if candidate.partner {
                        {" "}<span class="batch-partner">{t::PARTNER}</span>
                    }
                </label>
                {planned}
            </li>
        }
    }
}

fn fetch_candidates(scope: Scope<BatchApply>, id: u32) {
    spawn_local(async move {
        let result = query_authenticated::<BatchCandidates, _>(
            scope.clone(),
            batch_candidates::Variables { id: id as i64 },
        )
        .await
        .map_err(|e| e.message())
        .map(|data| {
            data.topology
                .device_by_id
                .map(|d| d.batch_candidates)
                .unwrap_or_default()
        });
        scope.send_message(BatchMessage::Candidates(result));
    });
}

fn fetch_planned(scope: Scope<BatchApply>, id: i64) {
    spawn_local(async move {
        let planned = match query_authenticated_response::<BatchDevice, _>(
            scope.clone(),
            batch_device::Variables { id },
        )
        .await
        {
            Err(error) => Planned::Problem(error.message()),
            Ok(response) => {
                let problem = response
                    .errors
                    .as_ref()
                    .and_then(|errors| errors.first())
                    .map(|e| ServerProblem::of(e).text());
                let access = response
                    .data
                    .and_then(|data| data.topology.device_by_id)
                    .and_then(|device| device.access);
                match access {
                    None => Planned::Problem(problem.unwrap_or_else(|| t::NO_ACCESS.into())),
                    Some(access) => match access.generate_cfg {
                        None => Planned::Problem(problem.unwrap_or_else(|| device::NOT_GENERATED.into())),
                        Some(cfg) => Planned::Ready {
                            script: cfg.script,
                            warnings: cfg.warnings.iter().map(describe_warning).collect(),
                            connected: Some(device::connected(
                                &access.connected.address,
                                access.connected.neighbor.as_deref(),
                                &access.connected.identity,
                            )),
                        },
                    },
                }
            }
        };
        scope.send_message(BatchMessage::Planned(id, planned));
    });
}

fn start(scope: Scope<BatchApply>, ids: Vec<i64>) {
    spawn_local(async move {
        let result = query_authenticated::<ApplyBatch, _>(scope.clone(), apply_batch::Variables { ids })
            .await
            .map(|data| {
                data.apply_batch
                    .into_iter()
                    // a batch's jobs are each a device's
                    .filter_map(|job| Some((job.id, job.device_id?, job.device_name?)))
                    .collect()
            });
        scope.send_message(BatchMessage::Started(result));
    });
}

/// A warning of `generateCfg`, as on the device page.
fn describe_warning(warning: &Warning) -> String {
    use device::warning as w;
    match warning {
        Warning::PoeCut(cut) => w::poe_cut(&cut.port, &cut.default_name),
        Warning::CapOffOutsideWlanGroup(cap) => w::cap_off_outside_wlan_group(&cap.package),
        Warning::WatchTargetSilent(silent) => {
            w::watch_target_silent(&silent.address, silent.port.as_deref(), &silent.device)
        }
        Warning::ApiClientsOutside(outside) => w::api_clients_outside(&outside.clients),
    }
}
