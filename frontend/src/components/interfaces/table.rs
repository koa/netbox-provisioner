//! The table of a task: a row per L3 interface, its findings when opened.
use super::{
    live::{self, RowLive},
    rows::{Action, Address, Device, Iface, Kind, PortRow, RadioPath, Row, Task},
};
use crate::{
    components::{
        findings::finding_list,
        menu::popup::{MenuActionItem, MenuGroup, PopupMenu},
        netbox_helper::HelperKind,
    },
    graphql::authenticated::findings::Severity,
    i18n::interfaces as t,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Cell, CellContext, Color, Icon, Label,
    MemoizedTableModel, MenuToggleVariant, Span, Spinner, SpinnerSize, Table, TableColumn,
    TableEntryRenderer, TableGridMode, TableHeader, TableMode, UseTableData, use_table_data,
};
use std::rc::Rc;
use yew::{Callback, Html, Properties, function_component, html, html_nested};
use yew_nested_router::components::Link;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Column {
    Port,
    Address,
    Nat,
    FarEnd,
    Network,
    Mtu,
    Path,
    Vlan,
    Ipv4,
    Ipv6,
    Dhcp,
    NetboxHas,
    Seen,
    State,
    Netbox,
    Actions,
}

impl Column {
    fn label(self) -> &'static str {
        match self {
            Column::Port => t::PORT,
            Column::Address => t::ADDRESS,
            Column::Nat => t::NAT,
            Column::FarEnd => t::FAR_END,
            Column::Network => t::NETWORK,
            Column::Mtu => t::MTU,
            Column::Path => t::PATH,
            Column::Vlan => t::VLAN,
            Column::Ipv4 => t::IPV4,
            Column::Ipv6 => t::IPV6,
            Column::Dhcp => t::DHCP,
            Column::NetboxHas => t::NETBOX_HAS,
            Column::Seen => t::SEES,
            Column::State => t::STATE,
            Column::Netbox => t::NETBOX,
            Column::Actions => t::ACTIONS,
        }
    }
}

/// The columns of a task (`docs/interface-view.md`); the loopback has none, it is a
/// description list.
fn columns(task: Task) -> &'static [Column] {
    match task {
        Task::Loopback => &[],
        Task::Uplink => &[Column::Port, Column::Address, Column::Nat, Column::State, Column::Netbox],
        Task::Direct => &[
            Column::Port,
            Column::FarEnd,
            Column::Network,
            Column::Mtu,
            Column::State,
            Column::Actions,
        ],
        Task::RadioLink => &[Column::Port, Column::Path, Column::Network, Column::State, Column::Actions],
        Task::Customer => &[
            Column::Port,
            Column::Vlan,
            Column::Ipv4,
            Column::Ipv6,
            Column::Dhcp,
            Column::State,
            Column::Actions,
        ],
        Task::Unknown => &[Column::Port, Column::NetboxHas, Column::Seen, Column::State, Column::Actions],
    }
}

#[derive(Clone, PartialEq)]
pub struct Entry {
    pub row: Rc<Row>,
    /// a port of the row's bridge, shown below it with its own actions
    pub port: Option<Rc<PortRow>>,
    /// NetBox' web UI, ending in `/`
    pub netbox: Rc<str>,
    pub live: RowLive,
    /// written to NetBox here, the device not yet applied: opens the helper that wrote, which
    /// offers to take it back
    pub changed: Option<Callback<()>>,
    /// opens the helper of an action
    pub on_action: Callback<Action>,
    /// „Anwenden“ of a changed row: to the card „Änderungen“
    pub on_apply: Callback<()>,
}

impl Entry {
    /// The row's actions, those of the live checks first: they take what the device sees, and
    /// replace one of the row with the same helper (a link from the port, the other end open).
    fn actions(&self) -> Vec<Action> {
        let live = match &self.live {
            RowLive::Checked(live) => live.actions.as_slice(),
            RowLive::Pending | RowLive::Unchecked => &[],
        };
        let own = match &self.port {
            Some(port) => &port.actions,
            None => &self.row.actions,
        };
        // „PoE-Eintrag entfernen“ only at its port
        let here = self.port.as_ref().map_or(self.row.interface.id, |p| p.interface.id) as u32;
        let live = live
            .iter()
            .filter(|a| a.helper != HelperKind::Poe || a.interface == Some(here))
            .cloned()
            .collect::<Vec<_>>();
        let mut actions = live.clone();
        actions.extend(
            own.iter()
                .filter(|a| !live.iter().any(|l| (l.kind, l.helper) == (a.kind, a.helper)))
                .cloned(),
        );
        actions
    }
    fn netbox_url(&self) -> String {
        let id = self.port.as_ref().map_or(self.row.interface.id, |p| p.interface.id);
        format!("{}dcim/interfaces/{id}/", self.netbox)
    }
    /// A bridge's port: its name and label, where its cable goes, its state, its actions.
    fn render_port_cell(&self, port: &PortRow, column: Column) -> Html {
        match column {
            Column::Port => html! {
                <span class="interface-port">
                    {"↳ "}<span class="interface-name">{&port.interface.name}</span>
                    if let Some(label) = &port.label {
                        <div class="interface-ports">{label}</div>
                    }
                </span>
            },
            Column::FarEnd | Column::NetboxHas => far_ends(&port.far_ends),
            Column::State => match &self.live {
                RowLive::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
                RowLive::Checked(live) => state(live.severities().iter()),
                RowLive::Unchecked => Html::default(),
            },
            Column::Actions => self.menu(),
            _ => Html::default(),
        }
    }
    /// The kebab menu: the actions and „In NetBox öffnen“.
    fn menu(&self) -> Html {
        let open = {
            let url = self.netbox_url();
            Callback::from(move |()| {
                if let Some(window) = web_sys::window() {
                    let _ = window.open_with_url_and_target(&url, "_blank");
                }
            })
        };
        html! {
            <div class="row-actions">
                <PopupMenu variant={MenuToggleVariant::Plain} icon={html!({Icon::EllipsisV})} aria_label={t::ACTIONS}>
                    <MenuGroup>
                        { for self.actions().iter().map(|action| {
                            let on_action = self.on_action.clone();
                            let chosen = action.clone();
                            html!(<MenuActionItem onclick={Callback::from(move |()| on_action.emit(chosen.clone()))}>
                                {action.label()}
                            </MenuActionItem>)
                        }) }
                        <MenuActionItem onclick={open}>{t::OPEN_IN_NETBOX}</MenuActionItem>
                    </MenuGroup>
                </PopupMenu>
            </div>
        }
    }
}

/// The buttons of the actions, the first suggestion primary, taking the link out as danger.
pub fn action_buttons(actions: &[Action], on_action: &Callback<Action>) -> Html {
    if actions.is_empty() {
        return Html::default();
    }
    html! {
        <div class="row-buttons">
            { for actions.iter().enumerate().map(|(index, action)| {
                let variant = match action.kind {
                    Kind::Remove => ButtonVariant::Danger,
                    Kind::Suggestion if index == 0 => ButtonVariant::Primary,
                    _ => ButtonVariant::Secondary,
                };
                let on_action = on_action.clone();
                let chosen = action.clone();
                html!(<Button {variant} label={action.label()}
                    onclick={Callback::from(move |_| on_action.emit(chosen.clone()))}/>)
            }) }
        </div>
    }
}

impl TableEntryRenderer<Column> for Entry {
    fn render_cell(&self, context: CellContext<'_, Column>) -> Cell {
        let row = &self.row;
        if let Some(port) = &self.port {
            return self.render_port_cell(port, *context.column).into();
        }
        match context.column {
            Column::Port => port(row),
            Column::Address => {
                if row.dhcp_client {
                    html!(<>{t::DHCP_CLIENT}{" "}{addresses(&row.addresses)}</>)
                } else {
                    addresses(&row.addresses)
                }
            }
            Column::Nat => yes(row.nat),
            Column::FarEnd => far_ends(&row.far_ends),
            Column::Network => addresses(&row.addresses),
            Column::Mtu => html!({ row.mtu.map(|mtu| mtu.to_string()).unwrap_or_else(|| "1500".into()) }),
            Column::Path => row.radio_path.as_ref().map(path).unwrap_or_default(),
            Column::Vlan => match &row.untagged_vlan {
                Some(vlan) => html!({ format!("{} ({})", vlan.vid, vlan.name) }),
                None => html!({ t::NONE }),
            },
            Column::Ipv4 => customer_addresses(row, false),
            Column::Ipv6 => customer_addresses(row, true),
            Column::Dhcp => yes(row.dhcp),
            Column::NetboxHas => netbox_has(row),
            Column::Seen => match &self.live {
                RowLive::Pending => html!(<Spinner size={SpinnerSize::Sm}/>),
                RowLive::Unchecked => html!({ t::NONE }),
                RowLive::Checked(live) if live.seen.is_empty() => html!({ t::SEES_NOBODY }),
                RowLive::Checked(live) => {
                    crate::components::neighbor_check::seen_list(&self.netbox, &live.seen)
                }
            },
            Column::State if self.changed.is_some() => changed_label(),
            Column::State => {
                let live_severities = match &self.live {
                    RowLive::Checked(live) => live.severities(),
                    _ => Vec::new(),
                };
                html! {<>
                    {state(row.findings.iter().map(|f| &f.severity).chain(&live_severities))}
                    { match &self.live {
                        RowLive::Pending => html!(<>{" "}<Spinner size={SpinnerSize::Sm}/></>),
                        RowLive::Unchecked => html!(<>{" "}{unchecked_label()}</>),
                        RowLive::Checked(_) => Html::default(),
                    } }
                </>}
            }
            Column::Netbox => html!(<a href={self.netbox_url()}
                target="_blank" rel="noopener">{t::OPEN_IN_NETBOX}</a>),
            Column::Actions => self.menu(),
        }
        .into()
    }

    fn render_details(&self) -> Vec<Span> {
        let row = &self.row;
        if let Some(port) = &self.port {
            return vec![Span::max(html! {
                <div class="interface-details">
                    {live::details(&self.netbox, &self.live, device)}
                    <dl class="interface-facts">
                        <dt>{t::BRIDGE}</dt><dd>{&row.interface.name}</dd>
                        <dt>{t::FAR_END}</dt><dd>{far_ends(&port.far_ends)}</dd>
                    </dl>
                    {action_buttons(&self.actions(), &self.on_action)}
                </div>
            })];
        }
        // the live checks' findings follow in their own list
        let live_clean = !matches!(&self.live, RowLive::Checked(live) if !live.severities().is_empty());
        let findings = if row.findings.is_empty() && live_clean {
            html!(<p>{t::NO_FINDINGS}</p>)
        } else if row.findings.is_empty() {
            Html::default()
        } else {
            finding_list(&self.netbox, &row.findings)
        };
        let tags = t::tags(row.ospf, row.dhcp, row.dhcp_client, row.nat, row.internet);
        let changed = self.changed.as_ref().map(|undo| {
            let on_apply = self.on_apply.clone();
            html! {
                <Alert inline=true r#type={AlertType::Info} title={t::CHANGED_TITLE}>
                    <p>{t::CHANGED_TEXT}</p>
                    <div class="row-buttons">
                        <Button variant={ButtonVariant::Primary} label={t::APPLY}
                            onclick={Callback::from(move |_| on_apply.emit(()))}/>
                        <Button variant={ButtonVariant::Secondary} label={t::UNDO}
                            onclick={undo.reform(|_| ())}/>
                    </div>
                </Alert>
            }
        });
        vec![Span::max(html! {
            <div class="interface-details">
                {changed}
                {findings}
                {live::details(&self.netbox, &self.live, device)}
                <dl class="interface-facts">
                    if !row.ports.is_empty() {
                        <dt>{t::PORTS}</dt>
                        <dd>{ row.ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ") }</dd>
                    }
                    if !row.addresses.is_empty() {
                        <dt>{t::ADDRESSES}</dt><dd>{addresses(&row.addresses)}</dd>
                    }
                    if !tags.is_empty() {
                        <dt>{t::TAGS}</dt><dd>{tags}</dd>
                    }
                    if let Some(label) = &row.label {
                        <dt>{t::LABEL}</dt><dd>{label}</dd>
                    }
                    if !row.tagged_vlans.is_empty() {
                        <dt>{t::VLAN}</dt>
                        <dd>{ format!("{} {}", t::TAGGED, row.tagged_vlans.iter().map(|v| v.vid.to_string()).collect::<Vec<_>>().join(", ")) }</dd>
                    }
                </dl>
                {action_buttons(&self.actions(), &self.on_action)}
            </div>
        })]
    }
}

#[derive(Clone, PartialEq, Properties)]
pub struct TaskTableProps {
    pub task: Task,
    pub entries: Rc<Vec<Entry>>,
}

#[function_component(TaskTable)]
pub fn task_table(props: &TaskTableProps) -> Html {
    let (entries, onexpand) = use_table_data(MemoizedTableModel::new(props.entries.clone()));
    let header = html_nested! {
        <TableHeader<Column>>
            { for columns(props.task).iter().map(|column| html_nested!(
                <TableColumn<Column> label={column.label()} index={*column}/>
            )) }
        </TableHeader<Column>>
    };
    html! {
        <Table<Column, UseTableData<Column, MemoizedTableModel<Entry>>>
            class="interface-table"
            mode={TableMode::CompactExpandable}
            grid={TableGridMode::Medium}
            {header}
            {entries}
            {onexpand}
        />
    }
}

/// The worst of `severities` as a label: errors, warnings, hints, or in order.
pub fn state<'s>(severities: impl IntoIterator<Item = &'s Severity>) -> Html {
    let (mut errors, mut warnings, mut hints) = (0, 0, 0);
    for severity in severities {
        match severity {
            Severity::ERROR => errors += 1,
            Severity::WARNING => warnings += 1,
            Severity::HINT | Severity::Other(_) => hints += 1,
        }
    }
    let (label, color, icon) = if errors > 0 {
        (t::errors(errors), Color::Red, Icon::ExclamationCircle)
    } else if warnings > 0 {
        (t::warnings(warnings), Color::Orange, Icon::ExclamationTriangle)
    } else if hints > 0 {
        (t::hints(hints), Color::Blue, Icon::InfoCircle)
    } else {
        (t::IN_ORDER.to_string(), Color::Green, Icon::CheckCircle)
    };
    html!(<Label compact=true {color} {icon} {label}/>)
}

/// Checked from NetBox alone: the device couldn't be asked.
pub fn unchecked_label() -> Html {
    html!(<Label compact=true color={Color::Grey} label={t::WITHOUT_DEVICE.to_string()}/>)
}

/// A row written to NetBox, its device not applied yet.
pub fn changed_label() -> Html {
    html!(<Label compact=true color={Color::Blue} icon={Icon::InfoCircle} label={t::CHANGED.to_string()}/>)
}

/// The name; a bridge's ports follow as rows of their own.
fn port(row: &Row) -> Html {
    html!(<span class="interface-name">{&row.interface.name}</span>)
}

fn yes(set: bool) -> Html {
    html!({ if set { "✓" } else { t::NONE } })
}

fn addresses(addresses: &[Address]) -> Html {
    if addresses.is_empty() {
        return html!({ t::NONE });
    }
    html!({ for addresses.iter().map(|a| html!(<div>{&a.address}</div>)) })
}

/// A customer port's addresses of one family, or where its VLAN has its L3.
fn customer_addresses(row: &Row, v6: bool) -> Html {
    let own = row
        .addresses
        .iter()
        .filter(|a| a.is_v6() == v6)
        .cloned()
        .collect::<Vec<_>>();
    if !own.is_empty() {
        return addresses(&own);
    }
    if !row.addresses.is_empty() || v6 {
        return html!({ t::NONE });
    }
    if row.gateways.is_empty() {
        return html!({ t::NO_GATEWAY });
    }
    html! {<>
        {t::L3_ON}
        { for row.gateways.iter().enumerate().map(|(i, d)| html!(<>
            if i > 0 { {", "} }
            {device(d)}
        </>)) }
    </>}
}

/// A device, as a link to its page here.
pub fn device(device: &Device) -> Html {
    match device.site {
        Some(site) => html!(<Link<AppRoute> to={AppRoute::device(site as u32, device.id as u32)}>{&device.name}</Link<AppRoute>>),
        None => html!({ &device.name }),
    }
}

fn interface(interface: &Iface) -> Html {
    html!(<>{device(&interface.device)}{" "}{&interface.name}</>)
}

fn far_ends(far_ends: &[Iface]) -> Html {
    if far_ends.is_empty() {
        return html!({ t::NO_CABLE });
    }
    html!({ for far_ends.iter().map(|f| html!(<div>{interface(f)}</div>)) })
}

/// `bridge ┄ ssid ┄ peer bridge → router`, as small labels breaking on a phone.
fn path(path: &RadioPath) -> Html {
    let part = |interface: &Option<Iface>| match interface {
        Some(i) => html!(<span class="path-part">{self::interface(i)}</span>),
        None => html!(<span class="path-part path-missing">{"?"}</span>),
    };
    html! {
        <span class="radio-path">
            {part(&path.router_port)}
            <span class="path-link">{"──"}</span>
            {part(&path.bridge_port)}
            <span class="path-link">{ format!("┄ {} ┄", path.ssid.as_deref().unwrap_or("?")) }</span>
            {part(&path.peer_bridge_port)}
            <span class="path-link">{"──"}</span>
            {part(&path.peer_router_port)}
        </span>
    }
}

/// An unknown interface: what NetBox has on it.
fn netbox_has(row: &Row) -> Html {
    let mut has = Vec::new();
    if !row.far_ends.is_empty() {
        has.push(html!(<div>{t::FAR_END}{": "}{far_ends(&row.far_ends)}</div>));
    }
    if !row.addresses.is_empty() {
        has.push(html!(<div>{addresses(&row.addresses)}</div>));
    }
    let tags = t::tags(row.ospf, row.dhcp, row.dhcp_client, row.nat, row.internet);
    if !tags.is_empty() {
        has.push(html!(<div>{tags}</div>));
    }
    if let Some(label) = &row.label {
        has.push(html!(<div>{label}</div>));
    }
    if has.is_empty() {
        return html!({ t::NO_CABLE });
    }
    html!({ for has })
}
