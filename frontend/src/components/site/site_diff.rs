//! Scope: How far each device of a site is from NetBox; nothing else.
//!
//! How far each device of a site is from NetBox (`Mutation.diffSite`, `device::site_diff`): a
//! job of the site reads every device in the background, the card shows each as it comes, in
//! NetBox' order and one line high each (no row moves or grows while it reads): the summary of
//! its diff with what needs a closer look (addresses removed, OSPF, bridge ports), the rest of
//! the diff in a modal.
//! Opened again, it picks up the site's last job, running or not; with `fresh` (the preview
//! before clearing the automatic rollout's error) it starts a new one. What the provisioner learned
//! of a device since (`knownDiff`, subscribed: the device page read it after an apply) goes
//! before the job's reading; a device changed since and not read again says so.
use crate::{
    components::device_link::DeviceLink,
    components::diff_view::{DiffView, summary_line, total},
    error::{FrontendError, ServerProblem},
    graphql::{
        POLL, RETRY,
        authenticated::{
            site_diff::{
                DiffEvent as Event, DiffJob as Job, DiffSite, KnownDevice, SiteDiffEvents,
                SiteDiffEventsVariables, SiteDiffJobs, SiteDiffKnown, SiteVariables,
            },
            types::{CfgDiff, JobKind, JobState},
        },
        live::{Subscription, watch},
        request,
    },
    i18n::site_diff as t,
    reading_time::read_at,
};
use cynic::{MutationBuilder, QueryBuilder, SubscriptionBuilder};
use patternfly_yew::prelude::{
    Bullseye, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Modal, ModalVariant,
    Spinner, SpinnerSize,
};
use std::{cell::Cell, collections::HashMap, rc::Rc};
use yew::{
    Callback, Component, Context, Html, Properties, html, html::Scope, platform::spawn_local,
    platform::time::sleep,
};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct SiteDiffProps {
    pub site: u32,
    /// start a new check at once instead of showing the last one
    #[prop_or_default]
    pub fresh: bool,
    /// the check has read every device
    #[prop_or_default]
    pub on_finished: Callback<()>,
}

#[derive(Clone)]
enum Read {
    Pending,
    Done(CfgDiff),
    Failed(ServerProblem),
    /// changed since the check, not read again
    Changed,
}

/// What the provisioner knows of a device (`knownDiff`), times in milliseconds since the epoch.
pub struct Known {
    read: Option<Read>,
    read_at: Option<f64>,
    changed_at: Option<f64>,
}

struct Row {
    id: i32,
    name: Box<str>,
    read: Read,
}

pub struct SiteDiff {
    job: Option<i32>,
    /// when the job started (milliseconds since the epoch)
    job_started: Option<f64>,
    /// by device
    known: HashMap<i32, Known>,
    _known_subscription: Subscription,
    /// the devices of the job, in NetBox' order, as read so far
    rows: Box<[Row]>,
    state: Option<JobState>,
    job_error: Option<ServerProblem>,
    error: Option<FrontendError>,
    lost: bool,
    /// whether the poll of the current job goes on
    alive: Rc<Cell<bool>>,
    /// the device whose diff is open in the details
    details: Option<i32>,
}

pub enum SiteDiffMsg {
    /// the site's last job, if it has one
    Last(Option<i32>),
    Start,
    Started(Result<i32, FrontendError>),
    Loaded(Job),
    Known(HashMap<i32, Known>),
    Lost,
    Error(FrontendError),
    /// opens a device's details, or closes them
    Details(Option<i32>),
}

impl Component for SiteDiff {
    type Message = SiteDiffMsg;
    type Properties = SiteDiffProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let site = ctx.props().site as i32;
        if ctx.props().fresh {
            scope.send_message(SiteDiffMsg::Start);
        } else {
            spawn_local(async move {
                let last = request(scope.clone(), SiteDiffJobs::build(SiteVariables { site }))
                    .await
                    .ok()
                    .and_then(|data| {
                        data.jobs
                            .into_iter()
                            .find(|job| job.kind == JobKind::SiteDiff)
                    })
                    .map(|job| job.id);
                scope.send_message(SiteDiffMsg::Last(last));
            });
        }
        let scope = ctx.link().clone();
        let known_subscription = watch(
            ctx.link(),
            SiteDiffKnown::build(SiteVariables { site }),
            move |response| {
                let devices = response
                    .ok()
                    .and_then(|response| response.data)
                    .and_then(|data| data.topology.site_by_id)
                    .map(|site| site.devices)
                    .unwrap_or_default();
                scope.send_message(SiteDiffMsg::Known(devices.into_iter().map(known).collect()));
            },
        );
        Self {
            job: None,
            job_started: None,
            known: HashMap::new(),
            _known_subscription: known_subscription,
            rows: Vec::new().into(),
            state: None,
            job_error: None,
            error: None,
            lost: false,
            alive: Rc::new(Cell::new(false)),
            details: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            SiteDiffMsg::Last(None) => return false,
            SiteDiffMsg::Last(Some(id)) | SiteDiffMsg::Started(Ok(id)) => self.follow(ctx, id),
            SiteDiffMsg::Start => {
                self.state = Some(JobState::Running);
                self.error = None;
                let scope = ctx.link().clone();
                let site = ctx.props().site as i32;
                spawn_local(async move {
                    let started = request(scope.clone(), DiffSite::build(SiteVariables { site }))
                        .await
                        .map(|data| data.diff_site.id);
                    scope.send_message(SiteDiffMsg::Started(started));
                });
            }
            SiteDiffMsg::Started(Err(error)) => {
                self.state = None;
                self.error = Some(error);
            }
            SiteDiffMsg::Loaded(job) => {
                self.error = None;
                for logged in job.events {
                    match logged.event {
                        Event::SiteDiffStarted(started) => {
                            self.rows = started
                                .devices
                                .into_iter()
                                .map(|device| Row {
                                    id: device.id,
                                    name: device.name.into(),
                                    read: Read::Pending,
                                })
                                .collect();
                        }
                        Event::DeviceDiffed(diffed) => {
                            if let Some(row) =
                                self.rows.iter_mut().find(|row| row.id == diffed.device.id)
                            {
                                row.read = match (diffed.diff, &diffed.error) {
                                    (_, Some(problem)) => {
                                        Read::Failed(ServerProblem::from(problem))
                                    }
                                    (Some(diff), None) => Read::Done(diff),
                                    (None, None) => Read::Pending,
                                };
                            }
                        }
                        Event::Other => {}
                    }
                }
                self.job_error = job.error.as_ref().map(ServerProblem::from);
                if job.state == JobState::Done && self.state != Some(JobState::Done) {
                    ctx.props().on_finished.emit(());
                }
                self.state = Some(job.state);
                self.job_started = Some(read_at(job.age));
            }
            SiteDiffMsg::Known(known) => self.known = known,
            SiteDiffMsg::Lost => {
                self.lost = true;
                self.state = None;
            }
            SiteDiffMsg::Error(error) => self.error = Some(error),
            SiteDiffMsg::Details(device) => self.details = device,
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let running = self.state == Some(JobState::Running);
        let label = if self.job.is_some() {
            t::AGAIN
        } else {
            t::START
        };
        let onclick = ctx.link().callback(|_| SiteDiffMsg::Start);
        let reads = self
            .rows
            .iter()
            .map(|row| (row, self.newest(row, running)))
            .collect::<Vec<_>>();
        let read = reads
            .iter()
            .filter(|(_, read)| !matches!(read, Read::Pending))
            .count();
        let in_line = reads
            .iter()
            .filter(|(_, read)| matches!(read, Read::Done(diff) if total(&diff.tables) == 0))
            .count();
        let progress = (!self.rows.is_empty()).then(|| {
            let text = if running {
                t::progress(read, self.rows.len())
            } else {
                t::in_line_count(in_line, self.rows.len())
            };
            html!(<p>{text}</p>)
        });
        // in NetBox' order, so no row moves while the job reads
        let rows = reads;
        let site = ctx.props().site;
        let details = self.details.and_then(|id| {
            rows.iter().find_map(|(row, read)| match read {
                Read::Done(diff) if row.id == id => Some((row.name.clone(), diff.clone())),
                _ => None,
            })
        });
        let link = ctx.link();
        html! {
            <Card class="findings">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{t::INTRO}</p>
                    <Button variant={ButtonVariant::Secondary} disabled={running} {onclick}>{label}</Button>
                    {self.error.as_ref().map(FrontendError::to_html)}
                    if self.lost { <p>{t::LOST}</p> }
                    if self.state == Some(JobState::Cancelled) { <p>{t::CANCELLED}</p> }
                    {self.job_error.as_ref().map(|problem| html!(<p>{problem.text()}</p>))}
                    {progress}
                    <dl class="neighbor-results site-diff">
                        { for rows.into_iter().map(|(row, read)| {
                            let id = row.id;
                            let ondetails = link.callback(move |_| SiteDiffMsg::Details(Some(id)));
                            html! {<>
                                <dt><DeviceLink site={Some(site)} id={row.id as u32} name={row.name.to_string()}/></dt>
                                <dd>{read_view(&read, ondetails)}</dd>
                            </>}
                        }) }
                    </dl>
                    if let Some((name, diff)) = details {
                        {details_view(&name, diff, link.callback(|_| SiteDiffMsg::Details(None)))}
                    }
                </CardBody>
            </Card>
        }
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        self.alive.set(false);
    }
}

impl SiteDiff {
    /// What to show of `row`: what is known if read after the job started, else the job's
    /// reading, unless the device changed since.
    fn newest(&self, row: &Row, running: bool) -> Read {
        if running && matches!(row.read, Read::Pending) {
            return Read::Pending;
        }
        let known = self.known.get(&row.id);
        let started = self.job_started.unwrap_or(f64::MAX);
        if let Some(Known {
            read: Some(read),
            read_at: Some(at),
            ..
        }) = known
            && *at >= started
        {
            return read.clone();
        }
        if known
            .and_then(|known| known.changed_at)
            .is_some_and(|at| at > started)
        {
            return Read::Changed;
        }
        row.read.clone()
    }
    /// Shows job `id` from its first event on, the poll of an earlier one stopped.
    fn follow(&mut self, ctx: &Context<Self>, id: i32) {
        self.alive.set(false);
        self.alive = Rc::new(Cell::new(true));
        self.job = Some(id);
        self.rows = Box::default();
        self.lost = false;
        self.job_error = None;
        poll(ctx.link().clone(), id, self.alive.clone());
    }
}

/// What is known of a device, in milliseconds since the epoch from the ages.
fn known(device: KnownDevice) -> (i32, Known) {
    let diff = device.known_diff;
    let read = match (diff.diff, &diff.error) {
        (_, Some(problem)) => Some(Read::Failed(ServerProblem::from(problem))),
        (Some(planned), None) => Some(Read::Done(planned)),
        (None, None) => None,
    };
    (
        device.id,
        Known {
            read,
            read_at: diff.age.map(read_at),
            changed_at: diff.changed_age.map(read_at),
        },
    )
}

/// One device's fields, script and script back, over the page.
fn details_view(name: &str, diff: CfgDiff, onclose: Callback<()>) -> Html {
    let footer = html! {
        <Button variant={ButtonVariant::Link} label={t::CLOSE} onclick={onclose.reform(|_| ())}/>
    };
    html! {
        <div class="pf-v6-c-backdrop diff-details">
            <Bullseye>
                <Modal
                    title={t::details_title(name)}
                    variant={ModalVariant::Large}
                    {onclose}
                    {footer}
                >
                    <DiffView {diff} summary=false/>
                </Modal>
            </Bullseye>
        </div>
    }
}

/// A row's text in one line, the whole in the tooltip.
fn line(text: String) -> Html {
    html!(<span class="diff-line" title={text.clone()}>{text}</span>)
}

fn read_view(read: &Read, ondetails: Callback<yew::MouseEvent>) -> Html {
    match read {
        Read::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
        Read::Changed => line(t::CHANGED_SINCE.into()),
        Read::Failed(problem) => line(t::unreadable(&problem.text())),
        Read::Done(diff) if total(&diff.tables) == 0 => line(t::IN_LINE.into()),
        Read::Done(diff) => html! {<>
            {summary_line(&diff.tables)}
            <Button variant={ButtonVariant::InlineLink} onclick={ondetails}>
                {t::DETAILS}
            </Button>
        </>},
    }
}

/// Asks for the job's new events until it is over, or the server doesn't know it anymore;
/// while asking fails, it asks again, less often.
fn poll(scope: Scope<SiteDiff>, id: i32, alive: Rc<Cell<bool>>) {
    spawn_local(async move {
        let mut after = 0;
        let mut failed = 0;
        while alive.get() {
            let variables = SiteDiffEventsVariables {
                id,
                after: Some(after),
            };
            match request(scope.clone(), SiteDiffEvents::build(variables)).await {
                Ok(SiteDiffEvents { job: Some(job) }) => {
                    failed = 0;
                    if let Some(last) = job.events.last() {
                        after = last.index + 1;
                    }
                    let over = job.state != JobState::Running;
                    if alive.get() {
                        scope.send_message(SiteDiffMsg::Loaded(job));
                    }
                    if over {
                        return;
                    }
                }
                Ok(SiteDiffEvents { job: None }) => {
                    scope.send_message(SiteDiffMsg::Lost);
                    return;
                }
                Err(error) => {
                    scope.send_message(SiteDiffMsg::Error(error));
                    sleep(RETRY[failed.min(RETRY.len() - 1)]).await;
                    failed += 1;
                    continue;
                }
            }
            sleep(POLL).await;
        }
    });
}
