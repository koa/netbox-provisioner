//! Scope: The views of a diff wherever one is shown (device page, batch apply, helper preview,
//! site check): its summary by table, the changed fields, the script and the script back; not
//! what is shown around it.
use crate::graphql::authenticated::types::{CfgDiff, EntryChange, EntryChangeKind, TableChanges};
use i18n::diff_view as t;
use patternfly_yew::prelude::{CodeBlock, CodeBlockCode, Tab, Tabs};
use yew::{Html, Properties, function_component, html, html_nested, use_state};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DiffViewProps {
    pub diff: CfgDiff,
    /// with the summary as first tab; without where it is shown next to it already (the site
    /// check's details)
    #[prop_or(true)]
    pub summary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Summary,
    Fields,
    Script,
    Revert,
}

/// The diff in tabs, the summary first.
#[function_component]
pub fn DiffView(props: &DiffViewProps) -> Html {
    let first = if props.summary {
        View::Summary
    } else {
        View::Fields
    };
    let view = use_state(|| first);
    let onselect = {
        let view = view.clone();
        move |shown| view.set(shown)
    };
    let diff = &props.diff;
    let mut tabs = Vec::new();
    if props.summary {
        tabs.push(html_nested! {
            <Tab<View> index={View::Summary} title={t::SUMMARY}>{summary(&diff.tables)}</Tab<View>>
        });
    }
    tabs.push(html_nested! {
        <Tab<View> index={View::Fields} title={t::FIELDS}>{fields(&diff.entries)}</Tab<View>>
    });
    tabs.push(html_nested! {
        <Tab<View> index={View::Script} title={t::SCRIPT}>
            <CodeBlock><CodeBlockCode>{diff.script.clone()}</CodeBlockCode></CodeBlock>
        </Tab<View>>
    });
    tabs.push(html_nested! {
        <Tab<View> index={View::Revert} title={t::REVERT}>
            <p class="diff-hint">{t::REVERT_HINT}</p>
            <CodeBlock><CodeBlockCode>{diff.revert_script.clone()}</CodeBlockCode></CodeBlock>
        </Tab<View>>
    });
    html! {
        <div class="diff-view">
            <Tabs<View> selected={*view} {onselect}>{ for tabs }</Tabs<View>>
        </div>
    }
}

/// How many changes the tables have; none: the device is as NetBox has it.
pub fn total(tables: &[TableChanges]) -> u32 {
    tables
        .iter()
        .map(|table| (table.added + table.changed + table.removed) as u32)
        .sum()
}

/// What needs a closer look: addresses and static leases removed, OSPF, bridge ports.
fn closer_look(tables: &[TableChanges]) -> Vec<String> {
    let removed = |names: &[&str]| {
        tables
            .iter()
            .filter(|table| names.contains(&table.table.as_str()))
            .map(|table| i64::from(table.removed))
            .sum::<i64>()
    };
    let mut closer_look = Vec::new();
    let addresses = removed(&["ip/address", "ipv6/address"]);
    if addresses > 0 {
        closer_look.push(t::removes_addresses(addresses));
    }
    let leases = removed(&["ip/dhcp-server/lease"]);
    if leases > 0 {
        closer_look.push(t::removes_leases(leases));
    }
    if tables.iter().any(|m| m.table.starts_with("routing/ospf")) {
        closer_look.push(t::TOUCHES_OSPF.to_string());
    }
    if tables.iter().any(|m| m.table == "interface/bridge/port") {
        closer_look.push(t::TOUCHES_BRIDGE_PORTS.to_string());
    }
    closer_look
}

/// Each table's count.
fn table_list(tables: &[TableChanges]) -> String {
    tables
        .iter()
        .map(|m| t::table(&m.table, m.added.into(), m.changed.into(), m.removed.into()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The changes counted, what needs a closer look and each table's count.
fn summary(tables: &[TableChanges]) -> Html {
    let closer_look = closer_look(tables);
    html! {
        <div class="diff-summary">
            <strong>{t::changes(total(tables))}</strong>
            { for closer_look.into_iter().map(|text| html!(<>{", "}<strong>{text}</strong></>)) }
            <div>{table_list(tables)}</div>
        </div>
    }
}

/// The summary in one line, as high as a line of text (the site check's rows): what doesn't
/// fit ends in „…“ and is in the tooltip.
pub fn summary_line(tables: &[TableChanges]) -> Html {
    let closer_look = closer_look(tables);
    let list = table_list(tables);
    let mut whole = t::changes(total(tables));
    for text in &closer_look {
        whole.push_str(", ");
        whole.push_str(text);
    }
    whole.push_str(": ");
    whole.push_str(&list);
    html! {
        <span class="diff-line" title={whole}>
            <strong>{t::changes(total(tables))}</strong>
            { for closer_look.into_iter().map(|text| html!(<>{", "}<strong>{text}</strong></>)) }
            {": "}{list}
        </span>
    }
}

fn kind(kind: &EntryChangeKind) -> String {
    t::kind(match kind {
        EntryChangeKind::Add => t::Kind::Add,
        EntryChangeKind::Set => t::Kind::Set,
        EntryChangeKind::Remove => t::Kind::Remove,
        EntryChangeKind::Move => t::Kind::Move,
        EntryChangeKind::Other(other) => t::Kind::Other(other),
    })
}

fn value(value: &Option<String>) -> Html {
    match value {
        Some(value) => html!(<code>{value.clone()}</code>),
        None => html!({ t::NONE }),
    }
}

/// Each entry with its fields before and after, like two rows of a table compared: one table,
/// so the columns line up across the entries, each entry a group under its own head.
fn fields(entries: &[EntryChange]) -> Html {
    html! {
        <div class="diff-fields">
            <table class="pf-v6-c-table pf-m-compact">
                <thead class="pf-v6-c-table__thead">
                    <tr class="pf-v6-c-table__tr">
                        <th class="pf-v6-c-table__th">{t::FIELD}</th>
                        <th class="pf-v6-c-table__th">{t::BEFORE}</th>
                        <th class="pf-v6-c-table__th">{t::AFTER}</th>
                    </tr>
                </thead>
                { for entries.iter().map(entry_rows) }
            </table>
        </div>
    }
}

fn entry_rows(entry: &EntryChange) -> Html {
    html! {
        <tbody class="pf-v6-c-table__tbody">
            <tr class="pf-v6-c-table__tr diff-entry">
                <th class="pf-v6-c-table__th" colspan="3">
                    <strong>{format!("/{}", entry.table)}</strong>
                    {" "}
                    {kind(&entry.kind)}
                    if let Some(key) = &entry.entry {
                        {" "}<code>{key.clone()}</code>
                    }
                </th>
            </tr>
            { for entry.fields.iter().map(|field| html! {
                <tr class="pf-v6-c-table__tr">
                    <td class="pf-v6-c-table__td">{field.field.clone()}</td>
                    <td class="pf-v6-c-table__td diff-before">{value(&field.before)}</td>
                    <td class="pf-v6-c-table__td diff-after">{value(&field.after)}</td>
                </tr>
            }) }
        </tbody>
    }
}
