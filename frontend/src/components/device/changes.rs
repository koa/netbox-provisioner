//! Scope: The card „Änderungen“ on the device page: what an apply would change, or why the
//! device can't be reached, and the buttons to apply; not the setup it shows then (`setup`).
use crate::{
    components::device::batch_apply::{BatchApply, BatchJob},
    graphql::authenticated::types::ApplyWarning as Warning,
    i18n::{apply_warning, device as t},
    reading_time::local_time,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, CodeBlock,
    CodeBlockCode,
};
use yew::{Callback, Children, Html, Properties, function_component, html};

/// The planned changes and what applying them would do that NetBox can't show (PoE switched
/// off).
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedCfg {
    pub script: Box<str>,
    pub warnings: Vec<Warning>,
    /// the login went with the previous password: applying sets the current one, also without a
    /// script
    pub previous_password: bool,
    /// WebFig over HTTPS needs its certificate: applying installs it first, also without a
    /// script
    pub www_certificate_needed: bool,
    /// when the device was read for it (milliseconds since the epoch, from the age it came with)
    pub as_of: f64,
}

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ChangesProps {
    pub id: u32,
    /// the device could be reached
    pub reachable: bool,
    /// the changes; `None`: the generator failed (or the device couldn't be reached)
    pub cfg: Option<GeneratedCfg>,
    /// why it couldn't be reached
    pub access_problem: Option<Box<str>>,
    /// a job runs, or writing is locked
    pub disabled: bool,
    /// read the device anew
    pub on_reload: Callback<()>,
    pub on_apply: Callback<()>,
    pub on_batch_started: Callback<Vec<BatchJob>>,
    /// shown on an unreachable device: its setup over the neighbor
    #[prop_or_default]
    pub setup: Html,
    /// shown on an unreachable device: the form for another address or other credentials
    #[prop_or_default]
    pub children: Children,
}

/// „Änderungen“.
#[function_component]
pub fn Changes(props: &ChangesProps) -> Html {
    let cfg = props.cfg.as_ref();
    let script = cfg.map(|cfg| cfg.script.as_ref()).unwrap_or_default();
    let outside_script = cfg.is_some_and(|cfg| cfg.previous_password || cfg.www_certificate_needed);
    let nothing_to_do = script.trim().is_empty() && !outside_script;
    let changes = match cfg {
        _ if !props.reachable => {
            html!(<Alert inline=true title={t::UNREACHABLE} r#type={AlertType::Warning}>
            if let Some(problem) = &props.access_problem {
                <p>{problem.as_ref()}</p>
            }
            {props.setup.clone()}
            <p>{t::UNREACHABLE_HINT}</p>
            {props.children.clone()}
        </Alert>)
        }
        None => html!(<Alert inline=true title={t::NOT_GENERATED} r#type={AlertType::Warning}>
            <p>{t::NOT_GENERATED_HINT}</p>
        </Alert>),
        Some(_) if nothing_to_do => {
            html!(<Alert inline=true title={t::IN_LINE} r#type={AlertType::Success}/>)
        }
        Some(cfg) => html! {<>
            if cfg.www_certificate_needed {
                <Alert inline=true title={t::WWW_CERTIFICATE_NEEDED} r#type={AlertType::Info}>
                    <p>{t::WWW_CERTIFICATE_NEEDED_HINT}</p>
                </Alert>
            }
            if cfg.previous_password {
                <Alert inline=true title={t::PREVIOUS_PASSWORD} r#type={AlertType::Info}>
                    <p>{t::PREVIOUS_PASSWORD_HINT}</p>
                </Alert>
            }
            { for cfg.warnings.iter().map(|warning| html!(
                <Alert inline=true title={apply_warning::describe(warning)} r#type={AlertType::Warning}/>
            ))}
            if !script.trim().is_empty() {
                <CodeBlock><CodeBlockCode>{ script }</CodeBlockCode></CodeBlock>
            }
        </>},
    };
    html! {
        <Card class="device-cfg">
            <CardHeader><CardTitle>{t::CHANGES}</CardTitle></CardHeader>
            <CardBody>
                <p class="as-of">
                    if let Some(generated) = cfg {
                        {t::as_of(&local_time(generated.as_of))}
                    }
                    <Button
                        variant={ButtonVariant::Link}
                        label={t::RELOAD}
                        disabled={props.disabled}
                        onclick={props.on_reload.reform(|_| ())}
                    />
                </p>
                {changes}
                if props.reachable {
                    <Button
                        variant={ButtonVariant::Primary}
                        label={t::APPLY}
                        disabled={nothing_to_do || props.disabled}
                        onclick={props.on_apply.reform(|_| ())}
                    />
                    <BatchApply
                        id={props.id}
                        disabled={props.disabled}
                        on_started={props.on_batch_started.clone()}
                    />
                }
            </CardBody>
        </Card>
    }
}
