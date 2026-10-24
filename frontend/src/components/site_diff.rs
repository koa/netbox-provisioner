//! How far each device of a site is from NetBox (`Mutation.diffSite`, `device::site_diff`): a
//! job of the site reads every device in the background, the card shows each as it comes, the
//! most changes first, with what needs a closer look (addresses removed, OSPF, bridge ports).
//! Opened again, it picks up the site's last job, running or not.
use crate::{
    components::job::{POLL, RETRY},
    error::{FrontendError, ServerProblem},
    graphql::{
        authenticated::{
            DiffSite, SiteDiffEvents, SiteDiffJobs, diff_site,
            site_diff_events::{
                self, JobState, SiteDiffEventsJob as Job,
                SiteDiffEventsJobEventsEvent as Event,
                SiteDiffEventsJobEventsEventOnDeviceDiffedMenus as Menu,
            },
            site_diff_jobs::{self, JobKind},
        },
        query_authenticated,
    },
    i18n::site_diff as t,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize,
};
use std::{cell::Cell, rc::Rc};
use yew::{
    Component, Context, Html, Properties, html, html::Scope, platform::spawn_local,
    platform::time::sleep,
};
use yew_nested_router::components::Link;

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct SiteDiffProps {
    pub site: u32,
}

enum Read {
    Pending,
    Done(Vec<Menu>),
    Failed(ServerProblem),
}

struct Row {
    id: i64,
    name: String,
    read: Read,
}

pub struct SiteDiff {
    job: Option<i64>,
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
        Self {
            job: None,
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
                                    None => Read::Done(diffed.menus),
                                };
                            }
                        }
                        _ => {}
                    }
                }
                self.job_error = job.error.as_ref().map(ServerProblem::from);
                self.state = Some(job.state);
            }
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
        let read = self.rows.iter().filter(|row| !matches!(row.read, Read::Pending)).count();
        let in_line = self
            .rows
            .iter()
            .filter(|row| matches!(&row.read, Read::Done(menus) if total(menus) == 0))
            .count();
        let progress = (!self.rows.is_empty()).then(|| {
            let text = if running {
                t::progress(read, self.rows.len())
            } else {
                t::in_line_count(in_line, self.rows.len())
            };
            html!(<p>{text}</p>)
        });
        let mut rows = self.rows.iter().collect::<Vec<_>>();
        // the most changes first, then the unread, then the ones in line
        rows.sort_by_key(|row| match &row.read {
            Read::Done(menus) if total(menus) > 0 => (0, u32::MAX - total(menus)),
            Read::Failed(_) => (1, 0),
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
                        { for rows.into_iter().map(|row| html! {
                            <>
                                <dt><Link<AppRoute> to={AppRoute::device(site, row.id as u32)}>{row.name.clone()}</Link<AppRoute>></dt>
                                <dd>{read_view(&row.read)}</dd>
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

fn read_view(read: &Read) -> Html {
    match read {
        Read::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
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
