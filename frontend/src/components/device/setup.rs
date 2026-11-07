//! Scope: The initial setup of an unreachable device over its neighbor on the device page;
//! not asking the neighbor nor starting the job (the page).
use crate::{
    error::ServerProblem,
    graphql::authenticated::{
        device::{BootstrapPlan, Discovered},
        types::ErrorInfo,
    },
    i18n::device::setup,
};
use patternfly_yew::prelude::{Button, ButtonVariant, Spinner, SpinnerSize, TextInput};
use yew::{AttrValue, Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct SetupProps {
    /// the device's name in NetBox
    pub name: AttrValue,
    pub plan: Option<BootstrapPlan>,
    /// why there is no plan
    pub problem: Option<ErrorInfo>,
    /// what the neighbor sees on its port; `None` while it is asked
    pub candidates: Option<Result<Vec<Discovered>, String>>,
    /// a job runs, or writing is locked
    pub disabled: bool,
    /// set up the device with this MAC
    pub on_setup: Callback<String>,
}

/// What the neighbor sees at its port, or a MAC to type; or why there is no setup.
pub struct Setup {
    /// a MAC typed in instead of picking a candidate
    manual_mac: String,
}

pub enum SetupMsg {
    ManualMac(String),
}

impl Component for Setup {
    type Message = SetupMsg;
    type Properties = SetupProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            manual_mac: String::new(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            SetupMsg::ManualMac(mac) => self.manual_mac = mac,
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let plan = match (&props.plan, &props.problem) {
            (Some(plan), _) => plan,
            (None, Some(problem)) => {
                return html!(<p>{setup::impossible(&ServerProblem::from(problem).text())}</p>);
            }
            (None, None) => return html!(),
        };
        let candidates = match &props.candidates {
            None => {
                html!(<p><Spinner size={SpinnerSize::Md}/>{setup::searching(&plan.neighbor, &plan.neighbor_port)}</p>)
            }
            Some(Err(error)) => html!(<p>{setup::search_failed(&plan.neighbor, error)}</p>),
            Some(Ok(found)) if found.is_empty() => {
                html!(<p>{setup::nothing_found(&plan.neighbor, &plan.neighbor_port)}</p>)
            }
            Some(Ok(found)) => html! {
                <table class="bootstrap-candidates">
                    <thead><tr>
                        <th>{setup::IDENTITY}</th><th>{setup::BOARD}</th><th>{setup::ROUTEROS}</th><th>{setup::MAC}</th><th>{setup::PORT}</th><th/>
                    </tr></thead>
                    <tbody>
                    { for found.iter().map(|c| {
                        let mac = c.mac.0.clone();
                        html! {<tr>
                            <td>{&c.identity}</td><td>{&c.board}</td><td>{&c.version}</td>
                            <td><code>{&c.mac.0}</code></td><td>{&c.interface}</td>
                            <td><Button
                                variant={ButtonVariant::Secondary}
                                label={setup::set_up_as(&props.name)}
                                disabled={props.disabled}
                                onclick={props.on_setup.reform(move |_| mac.clone())}
                            /></td>
                        </tr>}
                    })}
                    </tbody>
                </table>
            },
        };
        let manual = self.manual_mac.clone();
        html! {<>
            <p>{setup::plan(&plan.neighbor, &plan.neighbor_port, &plan.port, &plan.address)}</p>
            <p>{setup::RESCUE}</p>
            {candidates}
            <div class="bootstrap-manual">
                <TextInput
                    value={self.manual_mac.clone()}
                    placeholder={setup::MAC_PLACEHOLDER}
                    onchange={ctx.link().callback(SetupMsg::ManualMac)}
                />
                <Button
                    variant={ButtonVariant::Secondary}
                    label={setup::WITH_MAC}
                    disabled={props.disabled || manual.trim().is_empty()}
                    onclick={props.on_setup.reform(move |_| manual.clone())}
                />
            </div>
        </>}
    }
}
