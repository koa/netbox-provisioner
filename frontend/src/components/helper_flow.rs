//! A helper in a modal, from a row of `components::interfaces` (its suggestion, taking the link
//! out, a task for it) or its menu „Weitere Helfer“: a stepper Plan → Vorschau → In NetBox
//! schreiben → Anwenden above the helper's form, plan and preview (`NetboxHelper`, embedded).
//! Written, „Jetzt anwenden“ goes to the card „Änderungen“, „Später“ closes it.
use crate::{
    components::{
        adjust_target::SelectedTarget,
        netbox_helper::{HelperPreset, NetboxHelper, Phase},
    },
    i18n::helper_flow as t,
};
use patternfly_yew::prelude::{
    Bullseye, Button, ButtonVariant, Modal, ModalVariant, ProgressStepper, ProgressStepperStep,
    ProgressStepperStepStatus,
};
use yew::{Callback, Component, Context, Html, Properties, html, html_nested};

#[derive(Clone, PartialEq, Properties)]
pub struct HelperFlowProps {
    pub id: u32,
    pub target: SelectedTarget,
    pub preset: HelperPreset,
    pub title: String,
    pub onclose: Callback<()>,
    /// NetBox was written to (or that was taken back)
    #[prop_or_default]
    pub on_written: Callback<()>,
    /// the write is done (`true`), or taken back (`false`)
    #[prop_or_default]
    pub on_done: Callback<bool>,
    /// „Jetzt anwenden“: to the card „Änderungen“
    #[prop_or_default]
    pub on_apply: Callback<()>,
}

pub struct HelperFlow {
    phase: Phase,
}

pub enum HelperFlowMsg {
    Phase(Phase),
}

impl Component for HelperFlow {
    type Message = HelperFlowMsg;
    type Properties = HelperFlowProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { phase: Phase::Plan }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            HelperFlowMsg::Phase(phase) => {
                if phase == Phase::Written {
                    ctx.props().on_done.emit(true);
                } else if self.phase == Phase::Written {
                    ctx.props().on_done.emit(false);
                }
                let changed = self.phase != phase;
                self.phase = phase;
                changed
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        // the step it is at, and whether writing failed
        let (current, failed) = match self.phase {
            Phase::Plan => (0, false),
            Phase::Preview => (1, false),
            Phase::Writing => (2, false),
            Phase::WriteFailed => (2, true),
            Phase::Written => (3, false),
        };
        let step = |index: usize, label: &'static str| {
            let status = if index == current && failed {
                ProgressStepperStepStatus::Danger
            } else if index < current || (index == 3 && current == 3) {
                ProgressStepperStepStatus::Success
            } else if index == current {
                ProgressStepperStepStatus::Info
            } else {
                ProgressStepperStepStatus::Pending
            };
            html_nested!(<ProgressStepperStep {status} is_current={index == current}><span>{label}</span></ProgressStepperStep>)
        };
        let onclose = props.onclose.clone();
        let footer = if self.phase == Phase::Written {
            let apply = {
                let (onclose, on_apply) = (props.onclose.clone(), props.on_apply.clone());
                Callback::from(move |_| {
                    on_apply.emit(());
                    onclose.emit(());
                })
            };
            html! {<>
                <Button variant={ButtonVariant::Primary} label={t::APPLY_NOW} onclick={apply}/>
                <Button variant={ButtonVariant::Link} label={t::LATER} onclick={onclose.reform(|_| ())}/>
            </>}
        } else {
            html!(<Button variant={ButtonVariant::Link} label={t::CLOSE} onclick={onclose.reform(|_| ())}/>)
        };
        html! {
            <div class="pf-v6-c-backdrop helper-flow">
                <Bullseye>
                    <Modal
                        title={props.title.clone()}
                        variant={ModalVariant::Large}
                        onclose={props.onclose.clone()}
                        disable_close_click_outside=true
                        {footer}
                    >
                        <ProgressStepper>
                            { for [t::PLAN, t::PREVIEW, t::WRITE, t::APPLY]
                                .into_iter()
                                .enumerate()
                                .map(|(index, label)| step(index, label)) }
                        </ProgressStepper>
                        <NetboxHelper
                            id={props.id}
                            target={props.target.clone()}
                            preset={props.preset.clone()}
                            embedded=true
                            on_phase={ctx.link().callback(HelperFlowMsg::Phase)}
                            on_written={props.on_written.clone()}
                        />
                    </Modal>
                </Bullseye>
            </div>
        }
    }
}
