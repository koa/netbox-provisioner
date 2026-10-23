//! The table of a task: a row per L3 interface, its findings when opened.
use super::rows::{Address, Device, Iface, RadioPath, Row, Task};
use crate::{
    components::findings::finding_list,
    graphql::authenticated::findings::Severity,
    i18n::interfaces as t,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{
    Cell, CellContext, Color, Icon, Label, MemoizedTableModel, Span, Table, TableColumn,
    TableEntryRenderer, TableGridMode, TableHeader, TableMode, UseTableData, use_table_data,
};
use std::rc::Rc;
use yew::{Html, Properties, function_component, html, html_nested};
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
    State,
    Netbox,
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
            Column::State => t::STATE,
            Column::Netbox => t::NETBOX,
        }
    }
}

/// The columns of a task (`docs/concepts/interface-view.md`); the loopback has none, it is a
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
            Column::Netbox,
        ],
        Task::RadioLink => &[Column::Port, Column::Path, Column::Network, Column::State, Column::Netbox],
        Task::Customer => &[
            Column::Port,
            Column::Vlan,
            Column::Ipv4,
            Column::Ipv6,
            Column::Dhcp,
            Column::State,
            Column::Netbox,
        ],
        Task::Unknown => &[Column::Port, Column::NetboxHas, Column::State, Column::Netbox],
    }
}

#[derive(Clone, PartialEq)]
pub struct Entry {
    pub row: Rc<Row>,
    /// NetBox' web UI, ending in `/`
    pub netbox: Rc<str>,
}

impl TableEntryRenderer<Column> for Entry {
    fn render_cell(&self, context: CellContext<'_, Column>) -> Cell {
        let row = &self.row;
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
            Column::State => state(row.findings.iter().map(|f| &f.severity)),
            Column::Netbox => html!(<a href={format!("{}dcim/interfaces/{}/", self.netbox, row.interface.id)}
                target="_blank" rel="noopener">{t::OPEN_IN_NETBOX}</a>),
        }
        .into()
    }

    fn render_details(&self) -> Vec<Span> {
        let row = &self.row;
        let findings = if row.findings.is_empty() {
            html!(<p>{t::NO_FINDINGS}</p>)
        } else {
            finding_list(&self.netbox, &row.findings)
        };
        let tags = t::tags(row.ospf, row.dhcp, row.dhcp_client, row.nat);
        vec![Span::max(html! {
            <div class="interface-details">
                {findings}
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

/// The name, the bridge's ports below it.
fn port(row: &Row) -> Html {
    html! {<>
        <span class="interface-name">{&row.interface.name}</span>
        if !row.ports.is_empty() {
            <div class="interface-ports">
                { row.ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ") }
            </div>
        }
    </>}
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
fn device(device: &Device) -> Html {
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
    let tags = t::tags(row.ospf, row.dhcp, row.dhcp_client, row.nat);
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
