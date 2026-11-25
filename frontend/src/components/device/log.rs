//! Scope: The device page's card of the device's log; nothing else.
use crate::{graphql::authenticated::device::LogEntry, i18n::device as t};
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, CodeBlock, CodeBlockCode};
use yew::{Html, Properties, function_component, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DeviceLogProps {
    /// the newest entries, oldest first
    pub log: Box<[LogEntry]>,
}

/// The newest entries of the device's log, folded; nothing without any.
#[function_component(DeviceLog)]
pub fn device_log(props: &DeviceLogProps) -> Html {
    let log = &props.log;
    if log.is_empty() {
        return Html::default();
    }
    html! {
        <Card class="device-log">
            <CardHeader><CardTitle>{t::LOG}</CardTitle></CardHeader>
            <CardBody>
                <details>
                    <summary>{t::log_entries(log.len())}</summary>
                    <CodeBlock>
                        <CodeBlockCode>
                            { for log.iter().map(|entry| format!("{} {} {}\n", entry.time, entry.topics, entry.message)) }
                        </CodeBlockCode>
                    </CodeBlock>
                </details>
            </CardBody>
        </Card>
    }
}
