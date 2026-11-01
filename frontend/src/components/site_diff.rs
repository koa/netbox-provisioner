//! How far each device of a site is from NetBox (`Mutation.diffSite`, `device::site_diff`): a
//! job of the site reads every device in the background, the card shows each as it comes, the
//! most changes first, with what needs a closer look (addresses removed, OSPF, bridge ports).
//! Opened again, it picks up the site's last job, running or not. What the provisioner learned
//! of a device since (`knownDiff`, subscribed: the device page read it after an apply) goes
//! before the job's reading; a device changed since and not read again says so.
use crate::{
    components::job::{POLL, RETRY},
    error::{FrontendError, ServerProblem},
    graphql::{
        authenticated::{
            DiffSite, SiteDiffEvents, SiteDiffJobs, SiteDiffKnown, diff_site,
            site_diff_events::{
                self, JobState, SiteDiffEventsJob as Job,
                SiteDiffEventsJobEventsEvent as Event,
            },
            site_diff_jobs::{self, JobKind},
            site_diff_known,
        },
        live::{Subscription, subscribe},
        query_authenticated,
    },
    i18n::site_diff as t,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize,
};
use std::{cell::Cell, collections::HashMap, rc::Rc};
use yew::{
    Component, Context, Html, Properties, html, html::Scope, platform::spawn_local,
    platform::time::sleep,
};
use yew_nested_router::components::Link;

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct SiteDiffProps {
    pub site: u32,
}

/// The changes of one menu, from the job or from what is known.
#[derive(Clone)]
struct Menu {
    menu: String,
    added: i64,
    changed: i64,
    removed: i64,
}

#[derive(Clone)]
enum Read {
    Pending,
    Done(Vec<Menu>),
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
    id: i64,
    name: String,
    read: Read,
}

pub struct SiteDiff {
    job: Option<i64>,
    /// when the job started (milliseconds since the epoch)
    job_started: Option<f64>,
    /// by device
    known: HashMap<i64, Known>,
    _known_subscription: Subscription,
    /// the devices of the job, in NetBox' order, as read so far
    rows: Vec<Row>,
    state: Option<JobState>,
    job_error: Option<ServerProblem>,
    error: Option<FrontendError>,
    lost: bool,
    /// whether the poll of the current job goes on
    alive: Rc<Cell<bool>>,
}

pub enum SiteDiffMsg {
    /// the site's last job, if it has one
    Last(Option<i64>),
    Start,
    Started(Result<i64, FrontendError>),
    Loaded(Job),
    Known(HashMap<i64, Known>),
    Lost,
    Error(FrontendError),
}

impl Component for SiteDiff {
    type Message = SiteDiffMsg;
    type Properties = SiteDiffProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let site = ctx.props().site as i64;
        spawn_local(async move {
            let last = query_authenticated::<SiteDiffJobs, _>(scope.clone(), site_diff_jobs::Variables { site })
                .await
                .ok()
                .and_then(|data| data.jobs.into_iter().find(|job| job.kind == JobKind::SITE_DIFF))
                .map(|job| job.id);
            scope.send_message(SiteDiffMsg::Last(last));
        });
        let scope = ctx.link().clone();
        let known_subscription =
            subscribe::<SiteDiffKnown, _>(ctx.link(), site_diff_known::Variables { site }, move |response| {
                let devices = response
                    .ok()
                    .and_then(|response| response.data)
                    .and_then(|data| data.topology.site_by_id)
                    .map(|site| site.devices)
                    .unwrap_or_default();
                scope.send_message(SiteDiffMsg::Known(devices.into_iter().map(known).collect()));
            });
        Self {
            job: None,
            job_started: None,
            known: HashMap::new(),
            _known_subscription: known_subscription,
            rows: Vec::new(),
            state: None,
            job_error: None,
            error: None,
            lost: false,
            alive: Rc::new(Cell::new(false)),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            SiteDiffMsg::Last(None) => return false,
            SiteDiffMsg::Last(Some(id)) | SiteDiffMsg::Started(Ok(id)) => self.follow(ctx, id),
            SiteDiffMsg::Start => {
                self.state = Some(JobState::RUNNING);
                self.error = None;
                let scope = ctx.link().clone();
                let site = ctx.props().site as i64;
                spawn_local(async move {
                    let started = query_authenticated::<DiffSite, _>(scope.clone(), diff_site::Variables { site })
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
                                    name: device.name,
                                    read: Read::Pending,
                                })
                                .collect();
                        }
                        Event::DeviceDiffed(diffed) => {
                            if let Some(row) = self.rows.iter_mut().find(|row| row.id == diffed.device.id) {
                                row.read = match &diffed.error {
                                    Some(problem) => Read::Failed(ServerProblem::from(problem)),
                                    None => Read::Done(
                                        diffed
                                            .menus
                                            .into_iter()
                                            .map(|menu| Menu {
                                                menu: menu.menu,
                                                added: menu.added,
                                                changed: menu.changed,
                                                removed: menu.removed,
                                            })
                                            .collect(),
                                    ),
                                };
                            }
                        }
                        _ => {}
                    }
                }
                self.job_error = job.error.as_ref().map(ServerProblem::from);
                self.state = Some(job.state);
                self.job_started = Some(js_sys::Date::now() - job.age as f64 * 1000.0);
            }
            SiteDiffMsg::Known(known) => self.known = known,
            SiteDiffMsg::Lost => {
                self.lost = true;
                self.state = None;
            }
            SiteDiffMsg::Error(error) => self.error = Some(error),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let running = self.state == Some(JobState::RUNNING);
        let label = if self.job.is_some() { t::AGAIN } else { t::START };
        let onclick = ctx.link().callback(|_| SiteDiffMsg::Start);
        let reads = self
            .rows
            .iter()
            .map(|row| (row, self.newest(row, running)))
            .collect::<Vec<_>>();
        let read = reads.iter().filter(|(_, read)| !matches!(read, Read::Pending)).count();
        let in_line = reads
            .iter()
            .filter(|(_, read)| matches!(read, Read::Done(menus) if total(menus) == 0))
            .count();
        let progress = (!self.rows.is_empty()).then(|| {
            let text = if running {
                t::progress(read, self.rows.len())
            } else {
                t::in_line_count(in_line, self.rows.len())
            };
            html!(<p>{text}</p>)
        });
        let mut rows = reads;
        // the most changes first, then the unread, then the ones in line
        rows.sort_by_key(|(_, read)| match read {
            Read::Done(menus) if total(menus) > 0 => (0, u32::MAX - total(menus)),
            Read::Failed(_) | Read::Changed => (1, 0),
            Read::Pending => (2, 0),
            Read::Done(_) => (3, 0),
        });
        let site = ctx.props().site;
        html! {
            <Card class="findings">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{t::INTRO}</p>
                    <Button variant={ButtonVariant::Secondary} disabled={running} {onclick}>{label}</Button>
                    {self.error.as_ref().map(FrontendError::to_html)}
                    if self.lost { <p>{t::LOST}</p> }
                    if self.state == Some(JobState::CANCELLED) { <p>{t::CANCELLED}</p> }
                    {self.job_error.as_ref().map(|problem| html!(<p>{problem.text()}</p>))}
                    {progress}
                    <dl class="neighbor-results">
                        { for rows.into_iter().map(|(row, read)| html! {
                            <>
                                <dt><Link<AppRoute> to={AppRoute::device(site, row.id as u32)}>{row.name.clone()}</Link<AppRoute>></dt>
                                <dd>{read_view(&read)}</dd>
                            </>
                        }) }
                    </dl>
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
        if let Some(Known { read: Some(read), read_at: Some(at), .. }) = known
            && *at >= started
        {
            return read.clone();
        }
        if known.and_then(|known| known.changed_at).is_some_and(|at| at > started) {
            return Read::Changed;
        }
        row.read.clone()
    }
    /// Shows job `id` from its first event on, the poll of an earlier one stopped.
    fn follow(&mut self, ctx: &Context<Self>, id: i64) {
        self.alive.set(false);
        self.alive = Rc::new(Cell::new(true));
        self.job = Some(id);
        self.rows.clear();
        self.lost = false;
        self.job_error = None;
        poll(ctx.link().clone(), id, self.alive.clone());
    }
}

fn total(menus: &[Menu]) -> u32 {
    menus
        .iter()
        .map(|menu| (menu.added + menu.changed + menu.removed) as u32)
        .sum()
}

/// What is known of a device, in milliseconds since the epoch from the ages.
fn known(device: site_diff_known::SiteDiffKnownTopologySiteByIdDevices) -> (i64, Known) {
    let now = js_sys::Date::now();
    let diff = device.known_diff;
    let read = match (diff.menus, &diff.error) {
        (_, Some(problem)) => Some(Read::Failed(ServerProblem::from(problem))),
        (Some(menus), None) => Some(Read::Done(
            menus
                .into_iter()
                .map(|menu| Menu {
                    menu: menu.menu,
                    added: menu.added,
                    changed: menu.changed,
                    removed: menu.removed,
                })
                .collect(),
        )),
        (None, None) => None,
    };
    let at = |age: Option<i64>| age.map(|age| now - age as f64 * 1000.0);
    (
        device.id,
        Known {
            read,
            read_at: at(diff.age),
            changed_at: at(diff.changed_age),
        },
    )
}

fn read_view(read: &Read) -> Html {
    match read {
        Read::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
        Read::Changed => html!({ t::CHANGED_SINCE }),
        Read::Failed(problem) => html!({ t::unreadable(&problem.text()) }),
        Read::Done(menus) if total(menus) == 0 => html!({ t::IN_LINE }),
        Read::Done(menus) => {
            let addresses = menus
                .iter()
                .filter(|menu| menu.menu == "ip/address" || menu.menu == "ipv6/address")
                .map(|menu| menu.removed)
                .sum::<i64>();
            let mut closer_look = Vec::new();
            if addresses > 0 {
                closer_look.push(t::removes_addresses(addresses));
            }
            let leases = menus
                .iter()
                .filter(|menu| menu.menu == "ip/dhcp-server/lease")
                .map(|menu| menu.removed)
                .sum::<i64>();
            if leases > 0 {
                closer_look.push(t::removes_leases(leases));
            }
            if menus.iter().any(|menu| menu.menu.starts_with("routing/ospf")) {
                closer_look.push(t::TOUCHES_OSPF.to_string());
            }
            if menus.iter().any(|menu| menu.menu == "interface/bridge/port") {
                closer_look.push(t::TOUCHES_BRIDGE_PORTS.to_string());
            }
            html! {
                <>
                    <strong>{t::changes(total(menus))}</strong>
                    { for closer_look.into_iter().map(|text| html!(<>{", "}<strong>{text}</strong></>)) }
                    <div>{ menus.iter().map(|menu| t::menu(&menu.menu, menu.added, menu.changed, menu.removed)).collect::<Vec<_>>().join(", ") }</div>
                </>
            }
        }
    }
}

/// Asks for the job's new events until it is over, or the server doesn't know it anymore;
/// while asking fails, it asks again, less often.
fn poll(scope: Scope<SiteDiff>, id: i64, alive: Rc<Cell<bool>>) {
    spawn_local(async move {
        let mut after = 0;
        let mut failed = 0;
        while alive.get() {
            match query_authenticated::<SiteDiffEvents, _>(
                scope.clone(),
                site_diff_events::Variables {
                    id,
                    after: Some(after),
                },
            )
            .await
            {
                Ok(site_diff_events::ResponseData { job: Some(job) }) => {
                    failed = 0;
                    if let Some(last) = job.events.last() {
                        after = last.index + 1;
                    }
                    let over = job.state != JobState::RUNNING;
                    if alive.get() {
                        scope.send_message(SiteDiffMsg::Loaded(job));
                    }
                    if over {
                        return;
                    }
                }
                Ok(site_diff_events::ResponseData { job: None }) => {
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
