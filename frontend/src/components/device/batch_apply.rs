//! Scope: Applying a device together with others; nothing else.
//!
//! Applying the config to a device together with others (`applyBatch`): the other ends of its
//! wireless links are picked, more devices of the sites can be added, and each device of the
//! batch, this one first, shows its changes as the batch would plan them (with the others:
//! OSPF segments with every end in it move) before anything is applied.
use crate::{
    components::device_link::DeviceLink,
    components::diff_view::DiffView,
    error::{FrontendError, ServerProblem},
    graphql::authenticated::types::CfgDiff,
    graphql::{
        authenticated::batch_apply::{
            ApplyBatch, ApplyBatchVariables, BatchCandidate as Candidate, BatchCandidates,
            BatchDevice, DeviceVariables, PlanVariables,
        },
        request, request_response,
    },
    i18n::{apply_warning, batch as t, device},
};
use cynic::{MutationBuilder, QueryBuilder};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner,
    SpinnerSize,
};
use std::collections::{BTreeSet, HashMap};
use yew::{
    Callback, Component, Context, Html, Properties, html, html::Scope, platform::spawn_local,
};

/// A job of a started batch, as the device page shows it next to the others.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchJob {
    pub job: i32,
    pub device: i32,
    pub name: Box<str>,
    /// for the link to the device's page; `None` for the page's own device
    pub site: Option<i32>,
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
    Ready {
        diff: CfgDiff,
        warnings: Vec<String>,
        connected: Option<String>,
    },
    /// why it can't be applied
    Problem(String),
}

pub struct BatchApply {
    open: bool,
    candidates: Option<Result<Vec<Candidate>, String>>,
    picked: BTreeSet<i32>,
    /// the plans of the batch's devices, this one's too, for the pick of `round`
    planned: HashMap<i32, Planned>,
    /// counts the picks: a plan for an earlier one is dropped
    round: u32,
    starting: bool,
    error: Option<FrontendError>,
}

pub enum BatchMessage {
    Open,
    Candidates(Result<Vec<Candidate>, String>),
    Toggle(i32),
    Planned(u32, i32, Planned),
    Apply,
    Started(Result<Vec<(i32, i32, String)>, FrontendError>),
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
            round: 0,
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
                    self.picked
                        .extend(candidates.iter().filter(|c| c.partner).map(|c| c.device.id));
                }
                self.candidates = Some(candidates);
                self.plan_all(ctx);
            }
            BatchMessage::Toggle(id) => {
                if !self.picked.remove(&id) {
                    self.picked.insert(id);
                }
                self.plan_all(ctx);
            }
            BatchMessage::Planned(round, id, planned) => {
                if round == self.round {
                    self.planned.insert(id, planned);
                }
            }
            BatchMessage::Apply => {
                self.starting = true;
                self.error = None;
                let ids: Vec<i32> = std::iter::once(ctx.props().id as i32)
                    .chain(self.picked.iter().copied())
                    .collect();
                start(ctx.link().clone(), ids);
            }
            BatchMessage::Started(result) => {
                self.starting = false;
                match result {
                    Ok(jobs) => {
                        let own = ctx.props().id as i32;
                        let site = |device: i32| {
                            self.candidates
                                .as_ref()
                                .and_then(|c| c.as_ref().ok())
                                .and_then(|c| c.iter().find(|c| c.device.id == device))
                                .and_then(|c| c.device.site.as_ref())
                                .map(|site| site.id)
                        };
                        let mut jobs: Vec<BatchJob> = jobs
                            .into_iter()
                            .map(|(job, device, name)| BatchJob {
                                job,
                                device,
                                name: name.into(),
                                site: site(device),
                            })
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
            Some(Err(error)) => {
                html!(<Alert inline=true r#type={AlertType::Danger} title={error.clone()}/>)
            }
            Some(Ok(candidates)) if candidates.is_empty() => html!(<p>{t::NO_CANDIDATES}</p>),
            Some(Ok(candidates)) => html! {
                <ul class="batch-candidates">
                    { for candidates.iter().map(|c| self.candidate_view(ctx, c)) }
                </ul>
            },
        };
        let own = ctx.props().id as i32;
        let ready = !self.picked.is_empty()
            && self
                .picked
                .iter()
                .chain([&own])
                .all(|id| matches!(self.planned.get(id), Some(Planned::Ready { .. })));
        let count = self.picked.len() + 1;
        let this_device = (!self.picked.is_empty()).then(|| {
            html! {
                <div class="batch-this-device">
                    <strong>{t::THIS_DEVICE}</strong>
                    {self.planned_view(own)}
                </div>
            }
        });
        html! {
            <Card class="batch-apply">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{t::EXPLANATION}</p>
                    {this_device}
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
    /// Plans every device of the batch anew, this one too: what one plans depends on who is
    /// in it.
    fn plan_all(&mut self, ctx: &Context<Self>) {
        self.round += 1;
        self.planned.clear();
        if self.picked.is_empty() {
            return;
        }
        let together: Vec<i32> = std::iter::once(ctx.props().id as i32)
            .chain(self.picked.iter().copied())
            .collect();
        for &id in &together {
            self.planned.insert(id, Planned::Loading);
            fetch_planned(ctx.link().clone(), self.round, id, together.clone());
        }
    }

    /// What device `id` would get in the batch.
    fn planned_view(&self, id: i32) -> Html {
        match self.planned.get(&id) {
            None | Some(Planned::Loading) => html!(<Spinner size={SpinnerSize::Sm}/>),
            Some(Planned::Problem(problem)) => html!(
                <Alert inline=true r#type={AlertType::Danger} title={t::unreachable(problem)}/>
            ),
            Some(Planned::Ready {
                diff,
                warnings,
                connected,
            }) => html! {<>
                if let Some(connected) = connected {
                    <p class="batch-connected">{connected.clone()}</p>
                }
                { for warnings.iter().map(|w| html!(<Alert inline=true r#type={AlertType::Warning} title={w.clone()}/>)) }
                if diff.script.trim().is_empty() {
                    <p>{t::IN_LINE}</p>
                } else {
                    <DiffView diff={diff.clone()}/>
                }
            </>},
        }
    }

    fn candidate_view(&self, ctx: &Context<Self>, candidate: &Candidate) -> Html {
        let id = candidate.device.id;
        let picked = self.picked.contains(&id);
        let site = candidate.device.site.as_ref();
        let name = html! {
            <DeviceLink
                site={site.map(|site| site.id as u32)}
                id={id as u32}
                name={candidate.device.name.clone()}
            />
        };
        let planned = picked.then(|| self.planned_view(id));
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
        let variables = DeviceVariables { id: id as i32 };
        let result = request(scope.clone(), BatchCandidates::build(variables))
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

fn fetch_planned(scope: Scope<BatchApply>, round: u32, id: i32, together: Vec<i32>) {
    spawn_local(async move {
        let plan = BatchDevice::build(PlanVariables { id, together });
        let planned = match request_response(scope.clone(), plan).await {
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
                        None => Planned::Problem(
                            problem.unwrap_or_else(|| device::NOT_GENERATED.into()),
                        ),
                        Some(cfg) => Planned::Ready {
                            diff: cfg.diff,
                            warnings: cfg.warnings.iter().map(apply_warning::describe).collect(),
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
        scope.send_message(BatchMessage::Planned(round, id, planned));
    });
}

fn start(scope: Scope<BatchApply>, ids: Vec<i32>) {
    spawn_local(async move {
        let result = request(
            scope.clone(),
            ApplyBatch::build(ApplyBatchVariables { ids }),
        )
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
