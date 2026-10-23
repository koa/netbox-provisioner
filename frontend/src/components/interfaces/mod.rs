//! The device's interfaces by task (`DeviceAccess.interfaceView`,
//! `docs/concepts/interface-view.md`): a section per task, each with a table whose rows open to
//! their findings. What NetBox has comes first; the live checks (`live.rs`) follow in a query
//! of their own and mark a row too, „Live prüfen“ asks again. A row's actions open a helper in a modal
//! (`components::helper_flow`), with what the backend suggests chosen; „Weitere Helfer“ in the
//! header opens any helper. A row written to stays marked until the device is applied. The
//! filter by task and „nur mit Befund“ are kept in the browser (`localStorage`, comfort only).
pub mod live;
pub mod loopback;
pub mod rows;
pub mod table;

use crate::{
    components::{
        adjust_target::SelectedTarget,
        findings::finding_list,
        helper_flow::HelperFlow,
        menu::popup::{MenuActionItem, MenuGroup, PopupMenu},
        netbox_helper::{HelperKind, HelperPreset},
    },
    error::FrontendError,
    graphql::{
        authenticated::{InterfaceView, findings::Severity, interface_view},
        query_authenticated,
    },
    i18n::interfaces as t,
};
use live::Live;
use patternfly_yew::prelude::{
    Alert, AlertType, Badge, Button, ButtonVariant, Card, CardBody, CardHeader,
    CardHeaderActionsObject, CardTitle, ExpandableSection, Icon, Level, MenuToggleVariant, Spinner,
    Switch, Title, ToggleGroup, ToggleGroupItem,
};
use rows::{Action, Row, Task, View};
use std::{collections::BTreeMap, rc::Rc};
use table::{Entry, TaskTable};
use yew::{
    Callback, Component, Context, Html, Properties, html, html_nested, platform::spawn_local,
};

const FILTER_KEY: &str = "interfaces.task";
const ONLY_FINDINGS_KEY: &str = "interfaces.onlyFindings";

#[derive(Clone, PartialEq, Properties)]
pub struct InterfacesProps {
    pub id: u32,
    /// counts up when NetBox or the device changed: read again
    #[prop_or_default]
    pub refresh: u32,
    /// counts up when the device was applied: no row is changed anymore
    #[prop_or_default]
    pub applied: u32,
    /// how the helpers' previews reach the device
    #[prop_or_default]
    pub target: SelectedTarget,
    /// NetBox was written to: the page reads the changes again
    #[prop_or_default]
    pub on_written: Callback<()>,
    /// „Anwenden“: to the card „Änderungen“
    #[prop_or_default]
    pub on_apply: Callback<()>,
}

pub struct Interfaces {
    view: Option<Result<Rc<View>, FrontendError>>,
    live: Live,
    /// `None`: all tasks
    filter: Option<Task>,
    only_findings: bool,
    /// the helper open in the modal
    flow: Option<Flow>,
    /// the rows written to NetBox since the last apply (their interface), with the action that
    /// wrote: its helper offers to take the write back
    changed: BTreeMap<i64, Action>,
}

/// A helper in the modal: what it starts with, its title, the row and action it is for.
struct Flow {
    preset: HelperPreset,
    title: String,
    row: Option<(i64, Action)>,
}

pub enum InterfacesMsg {
    Data(Result<View, FrontendError>),
    Live(Live),
    /// „Live prüfen“
    CheckLive,
    Filter(Option<Task>),
    OnlyFindings(bool),
    /// a row's action (`None`: the loopback's, which may have no row)
    Open(Option<Rc<Row>>, Action),
    /// a helper of „Weitere Helfer“
    OpenHelper(HelperKind),
    Close,
    Written,
    /// the write of the open helper is done (`true`) or taken back
    Done(bool),
}

impl Component for Interfaces {
    type Message = InterfacesMsg;
    type Properties = InterfacesProps;

    fn create(_ctx: &Context<Self>) -> Self {
        let filter = stored(FILTER_KEY)
            .and_then(|key| Task::ALL.into_iter().find(|task| task.key() == key));
        Self {
            view: None,
            live: Live::Pending,
            filter,
            only_findings: stored(ONLY_FINDINGS_KEY).as_deref() == Some("true"),
            flow: None,
            changed: BTreeMap::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            InterfacesMsg::Data(view) => self.view = Some(view.map(Rc::new)),
            InterfacesMsg::Live(live) => self.live = live,
            InterfacesMsg::CheckLive => {
                self.live = Live::Pending;
                live::fetch(ctx.link().clone(), ctx.props().id, ctx.props().target.clone());
            }
            InterfacesMsg::Filter(filter) => {
                self.filter = filter;
                store(FILTER_KEY, filter.map(Task::key).unwrap_or_default());
            }
            InterfacesMsg::OnlyFindings(only) => {
                self.only_findings = only;
                store(ONLY_FINDINGS_KEY, if only { "true" } else { "false" });
            }
            InterfacesMsg::Open(row, action) => {
                self.flow = Some(Flow {
                    title: t::flow_title(
                        Some(action.kind),
                        action.helper,
                        row.as_ref().map(|row| row.interface.name.as_str()),
                    ),
                    preset: action.preset.clone(),
                    row: row.map(|row| (row.interface.id, action)),
                });
            }
            InterfacesMsg::OpenHelper(kind) => {
                self.flow = Some(Flow {
                    title: t::flow_title(None, kind, None),
                    preset: HelperPreset::Kind(kind),
                    row: None,
                });
            }
            InterfacesMsg::Close => self.flow = None,
            InterfacesMsg::Written => ctx.props().on_written.emit(()),
            InterfacesMsg::Done(done) => {
                if let Some((row, action)) = self.flow.as_ref().and_then(|flow| flow.row.clone()) {
                    if done {
                        self.changed.insert(row, action);
                    } else {
                        self.changed.remove(&row);
                    }
                }
            }
        }
        true
    }

    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        let props = ctx.props();
        if props.applied != old.applied {
            self.changed.clear();
        }
        if props.id != old.id || props.refresh != old.refresh || props.target != old.target {
            fetch(ctx);
            ctx.link().send_message(InterfacesMsg::CheckLive);
        }
        props.applied != old.applied || props.target != old.target
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let body = match &self.view {
            None => html!(<Spinner/>),
            Some(Err(error)) => error.to_html(),
            Some(Ok(view)) => self.sections(ctx, view),
        };
        let props = ctx.props();
        let flow = self.flow.as_ref().map(|flow| html! {
            <HelperFlow
                id={props.id}
                target={props.target.clone()}
                preset={flow.preset.clone()}
                title={flow.title.clone()}
                onclose={ctx.link().callback(|()| InterfacesMsg::Close)}
                on_written={ctx.link().callback(|()| InterfacesMsg::Written)}
                on_done={ctx.link().callback(InterfacesMsg::Done)}
                on_apply={props.on_apply.clone()}
            />
        });
        let check_live = html! {
            <Button
                variant={ButtonVariant::Secondary}
                icon={Icon::SyncAlt}
                label={t::CHECK_LIVE}
                disabled={self.live == Live::Pending}
                onclick={ctx.link().callback(|_| InterfacesMsg::CheckLive)}
            />
        };
        let helpers = html! {
            <PopupMenu variant={MenuToggleVariant::Secondary} text={html!(t::MORE_HELPERS)}>
                <MenuGroup>
                    { for HelperKind::ALL.into_iter().map(|kind| html! {
                        <MenuActionItem onclick={ctx.link().callback(move |()| InterfacesMsg::OpenHelper(kind))}>
                            {kind.to_string()}
                        </MenuActionItem>
                    }) }
                </MenuGroup>
            </PopupMenu>
        };
        html! {<>
            <Card class="interfaces">
                <CardHeader actions={CardHeaderActionsObject { actions: html!(<div class="interfaces-actions">{check_live}{helpers}</div>), has_no_offset: false, class: Default::default() }}>
                    <CardTitle>{t::TITLE}</CardTitle>
                </CardHeader>
                <CardBody>{body}</CardBody>
            </Card>
            {flow}
        </>}
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            fetch(ctx);
            ctx.link().send_message(InterfacesMsg::CheckLive);
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
            .map(|task| self.section(ctx, *task, view, &netbox))
            .collect::<Vec<_>>();
        let empty = sections.iter().all(Option::is_none);
        let live_findings = match &self.live {
            Live::Checked(checked) => checked.other_findings.clone(),
            _ => Vec::new(),
        };
        let unchecked = match &self.live {
            Live::Unchecked(message) => Some(html! {
                <Alert inline=true r#type={AlertType::Warning} title={t::LIVE_UNCHECKED_TITLE}>
                    <p>{message.clone()}</p>
                </Alert>
            }),
            _ => None,
        };
        html! {<>
            {filter}
            {unchecked}
            { for sections.into_iter().flatten() }
            if empty {
                <p>{t::NONE_WITH_FINDINGS}</p>
            }
            if (!view.other_findings.is_empty() || !live_findings.is_empty()) && self.filter.is_none() {
                <section class="interfaces-section">
                    <Title level={Level::H3}>{t::OTHER_FINDINGS}</Title>
                    if !view.other_findings.is_empty() {
                        {finding_list(&netbox, &view.other_findings)}
                    }
                    if !live_findings.is_empty() {
                        {crate::components::neighbor_check::neighbor_finding_list(&netbox, &live_findings)}
                    }
                </section>
            }
        </>}
    }

    /// The section of `task`; none when „nur mit Befund“ leaves nothing in it.
    fn section(&self, ctx: &Context<Self>, task: Task, view: &View, netbox: &Rc<str>) -> Option<Html> {
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
            let on_action = ctx.link().callback(|action| InterfacesMsg::Open(None, action));
            let changed = loopback.is_some_and(|row| self.changed.contains_key(&row.interface.id));
            loopback::view(netbox, loopback, &view.device_findings, &view.loopback_actions, changed, &on_action)
        } else {
            if rows.is_empty() {
                return None;
            }
            let (free, used): (Vec<&Row>, Vec<&Row>) = rows.into_iter().partition(|r| r.free);
            let entries = Rc::new(
                used.into_iter()
                    .map(|row| {
                        let row = Rc::new(row.clone());
                        let shown = row.clone();
                        let undo = self.changed.get(&row.interface.id).map(|action| {
                            let (row, action) = (row.clone(), action.clone());
                            ctx.link().callback(move |()| {
                                InterfacesMsg::Open(Some(row.clone()), action.clone())
                            })
                        });
                        Entry {
                            live: self.live.row(row.interface.id),
                            changed: undo,
                            row,
                            netbox: netbox.clone(),
                            on_action: ctx
                                .link()
                                .callback(move |action| InterfacesMsg::Open(Some(shown.clone()), action)),
                            on_apply: ctx.props().on_apply.clone(),
                        }
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
