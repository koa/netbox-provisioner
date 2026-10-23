//! The loopback: a list of its facts, with the findings about the device as a whole.
use super::{
    rows::{Action, Row},
    table::{action_buttons, changed_label, state},
};
use crate::{
    components::findings::finding_list,
    graphql::authenticated::findings::FindingsTopologyFindings,
    i18n::interfaces as t,
};
use patternfly_yew::prelude::{EmptyState, Size};
use yew::{Callback, Html, html};

/// `lo` if NetBox has it, the findings about the device and what the helpers can do: the
/// loopback's actions, which are there without `lo` too.
pub fn view(
    netbox: &str,
    row: Option<&Row>,
    device_findings: &[FindingsTopologyFindings],
    actions: &[Action],
    changed: bool,
    on_action: &Callback<Action>,
) -> Html {
    let findings = row
        .map(|row| &row.findings[..])
        .unwrap_or_default()
        .iter()
        .chain(device_findings)
        .cloned()
        .collect::<Vec<_>>();
    let list = if findings.is_empty() {
        html!()
    } else {
        finding_list(netbox, &findings)
    };
    let Some(row) = row else {
        return html! {<>
            <EmptyState title={t::NO_LOOPBACK} size={Size::Small}>{t::NO_LOOPBACK_TEXT}</EmptyState>
            {list}
            {action_buttons(actions, on_action)}
        </>};
    };
    let family = |v6: bool| {
        let addresses = row
            .addresses
            .iter()
            .filter(|a| a.is_v6() == v6)
            .map(|a| a.address.as_str())
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            t::NONE.to_string()
        } else {
            addresses.join(", ")
        }
    };
    html! {<>
        <dl class="interface-facts">
            <dt>{t::PORT}</dt>
            <dd>
                <a href={format!("{netbox}dcim/interfaces/{}/", row.interface.id)} target="_blank" rel="noopener">
                    {&row.interface.name}
                </a>
            </dd>
            <dt>{t::IPV4}</dt><dd>{family(false)}</dd>
            <dt>{t::IPV6}</dt><dd>{family(true)}</dd>
            <dt>{t::STATE}</dt>
            <dd>
                if changed {
                    {changed_label()}
                } else {
                    {state(findings.iter().map(|f| &f.severity))}
                }
            </dd>
        </dl>
        {list}
        {action_buttons(actions, on_action)}
    </>}
}
