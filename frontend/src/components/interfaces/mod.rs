//! The device's interfaces by task (`DeviceAccess.interfaceView`,
//! `docs/concepts/interface-view.md`): a section per task, each with a table whose rows open to
//! their findings. Read only so far; what NetBox has, no live checks. The filter by task and
//! „nur mit Befund“ are kept in the browser (`localStorage`, comfort only).
pub mod loopback;
pub mod rows;
pub mod table;

use crate::{
    components::findings::finding_list,
    error::FrontendError,
    graphql::{
        authenticated::{InterfaceView, findings::Severity, interface_view},
        query_authenticated,
    },
    i18n::interfaces as t,
};
use patternfly_yew::prelude::{
    Badge, Card, CardBody, CardHeader, CardTitle, ExpandableSection, Level, Spinner, Switch,
    Title, ToggleGroup, ToggleGroupItem,
};
use rows::{Row, Task, View};
use std::rc::Rc;
use table::{Entry, TaskTable};
use yew::{Component, Context, Html, Properties, html, html_nested, platform::spawn_local};

const FILTER_KEY: &str = "interfaces.task";
const ONLY_FINDINGS_KEY: &str = "interfaces.onlyFindings";

#[derive(Clone, PartialEq, Properties)]
pub struct InterfacesProps {
    pub id: u32,
    /// counts up when NetBox or the device changed: read again
    #[prop_or_default]
    pub refresh: u32,
}

pub struct Interfaces {
    view: Option<Result<Rc<View>, FrontendError>>,
    /// `None`: all tasks
    filter: Option<Task>,
    only_findings: bool,
}

pub enum InterfacesMsg {
    Data(Result<View, FrontendError>),
    Filter(Option<Task>),
    OnlyFindings(bool),
}

impl Component for Interfaces {
    type Message = InterfacesMsg;
    type Properties = InterfacesProps;

    fn create(_ctx: &Context<Self>) -> Self {
        let filter = stored(FILTER_KEY)
            .and_then(|key| Task::ALL.into_iter().find(|task| task.key() == key));
        Self {
            view: None,
            filter,
            only_findings: stored(ONLY_FINDINGS_KEY).as_deref() == Some("true"),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            InterfacesMsg::Data(view) => self.view = Some(view.map(Rc::new)),
            InterfacesMsg::Filter(filter) => {
                self.filter = filter;
                store(FILTER_KEY, filter.map(Task::key).unwrap_or_default());
            }
            InterfacesMsg::OnlyFindings(only) => {
                self.only_findings = only;
                store(ONLY_FINDINGS_KEY, if only { "true" } else { "false" });
            }
        }
        true
    }

    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        if ctx.props() != old {
            fetch(ctx);
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let body = match &self.view {
            None => html!(<Spinner/>),
            Some(Err(error)) => error.to_html(),
            Some(Ok(view)) => self.sections(ctx, view),
        };
        html! {
            <Card class="interfaces">
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>{body}</CardBody>
            </Card>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            fetch(ctx);
        }
    }
}

impl Interfaces {
    fn sections(&self, ctx: &Context<Self>, view: &View) -> Html {
        let count = |task: Task| view.rows.iter().filter(|r| r.task == task).count();
        let loopback = view.rows.iter().find(|r| r.task == Task::Loopback);
        let mut tasks = Task::ALL
            .into_iter()
            .filter(|task| *task == Task::Loopback || count(*task) > 0)
            .collect::<Vec<_>>();
        // a task with errors first
        tasks.sort_by_key(|task| {
            let errors = if *task == Task::Loopback {
                has_error(loopback.iter().flat_map(|r| &r.findings).chain(&view.device_findings))
            } else {
                view.rows
                    .iter()
                    .filter(|r| r.task == *task)
                    .any(|r| has_error(&r.findings))
            };
            !errors
        });
        let filter = html! {
            <div class="interfaces-filter">
                <ToggleGroup compact=true>
                    <ToggleGroupItem
                        text={format!("{} {}", t::ALL, view.rows.len())}
                        selected={self.filter.is_none()}
                        onchange={ctx.link().callback(|_| InterfacesMsg::Filter(None))}
                    />
                    { for tasks.iter().map(|task| {
                        let task = *task;
                        html_nested!(<ToggleGroupItem
                            text={format!("{} {}", t::task(task), count(task))}
                            selected={self.filter == Some(task)}
                            onchange={ctx.link().callback(move |_| InterfacesMsg::Filter(Some(task)))}
                        />)
                    }) }
                </ToggleGroup>
                <Switch
                    label={t::ONLY_FINDINGS}
                    checked={self.only_findings}
                    onchange={ctx.link().callback(InterfacesMsg::OnlyFindings)}
                />
            </div>
        };
        let netbox: Rc<str> = view.netbox.as_str().into();
        let sections = tasks
            .iter()
            .filter(|task| self.filter.is_none_or(|filter| filter == **task))
            .map(|task| self.section(*task, view, &netbox))
            .collect::<Vec<_>>();
        let empty = sections.iter().all(Option::is_none);
        html! {<>
            {filter}
            { for sections.into_iter().flatten() }
            if empty {
                <p>{t::NONE_WITH_FINDINGS}</p>
            }
            if !view.other_findings.is_empty() && self.filter.is_none() {
                <section class="interfaces-section">
                    <Title level={Level::H3}>{t::OTHER_FINDINGS}</Title>
                    {finding_list(&netbox, &view.other_findings)}
                </section>
            }
        </>}
    }

    /// The section of `task`; none when „nur mit Befund“ leaves nothing in it.
    fn section(&self, task: Task, view: &View, netbox: &Rc<str>) -> Option<Html> {
        let rows = view
            .rows
            .iter()
            .filter(|r| r.task == task)
            .filter(|r| !self.only_findings || !r.findings.is_empty())
            .collect::<Vec<_>>();
        let content = if task == Task::Loopback {
            let loopback = view.rows.iter().find(|r| r.task == Task::Loopback);
            let findings = loopback.map_or(0, |r| r.findings.len()) + view.device_findings.len();
            if self.only_findings && findings == 0 {
                return None;
            }
            loopback::view(netbox, loopback, &view.device_findings)
        } else {
            if rows.is_empty() {
                return None;
            }
            let (free, used): (Vec<&Row>, Vec<&Row>) = rows.into_iter().partition(|r| r.free);
            let entries = Rc::new(
                used.into_iter()
                    .map(|row| Entry {
                        row: Rc::new(row.clone()),
                        netbox: netbox.clone(),
                    })
                    .collect::<Vec<_>>(),
            );
            let free_text = t::free_ports(free.len());
            html! {<>
                if !entries.is_empty() {
                    <TaskTable {task} {entries}/>
                }
                if !free.is_empty() {
                    <ExpandableSection toggle_text_hidden={free_text.clone()} toggle_text_expanded={free_text}>
                        <p>{ free.iter().map(|r| r.interface.name.as_str()).collect::<Vec<_>>().join(", ") }</p>
                    </ExpandableSection>
                }
            </>}
        };
        let count = view.rows.iter().filter(|r| r.task == task).count();
        Some(html! {
            <section class="interfaces-section">
                <Title level={Level::H3}>{t::section(task)}{" "}<Badge read=true>{count}</Badge></Title>
                {content}
            </section>
        })
    }
}

fn has_error<'f>(
    findings: impl IntoIterator<Item = &'f crate::graphql::authenticated::findings::FindingsTopologyFindings>,
) -> bool {
    findings
        .into_iter()
        .any(|f| matches!(f.severity, Severity::ERROR))
}

fn fetch(ctx: &Context<Interfaces>) {
    let scope = ctx.link().clone();
    let device = i64::from(ctx.props().id);
    spawn_local(async move {
        let data = query_authenticated::<InterfaceView, _>(
            scope.clone(),
            interface_view::Variables { device },
        )
        .await
        .and_then(View::of);
        scope.send_message(InterfacesMsg::Data(data));
    });
}

/// A value the browser keeps; none where it keeps nothing (a private window, blocked storage).
fn stored(key: &str) -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
}

fn store(key: &str, value: &str) {
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.set_item(key, value);
    }
}
