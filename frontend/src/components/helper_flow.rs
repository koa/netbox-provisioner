//! A helper in a modal, from a port's row of `components::interfaces` (its suggestion, taking
//! the link out, a task for it, its settings): a stepper Plan → Vorschau → In NetBox schreiben →
//! Anwenden above the helper's form, plan and preview (`NetboxHelper`, embedded). „Funkbrücke“
//! from a free router port first asks for the bridge device to cable there (one radio, no cable
//! to a router but to this port), then embeds that device's helper. Written, „Jetzt anwenden“ goes to the card
//! „Änderungen“, „Später“ closes it.
use crate::{
    components::{
        adjust_target::SelectedTarget,
        menu::popup::Select,
        netbox_helper::{HelperPreset, NetboxHelper, Phase, RadioPreset},
    },
    error::FrontendError,
    graphql::{
        authenticated::{RadioBridgeCandidates, radio_bridge_candidates},
        query_authenticated,
    },
    i18n::helper_flow as t,
};
use patternfly_yew::prelude::{
    Bullseye, Button, ButtonVariant, Form, FormGroup, Modal, ModalVariant, ProgressStepper,
    ProgressStepperStep, ProgressStepperStepStatus, Spinner, SpinnerSize,
};
use std::fmt::{self, Display, Formatter};
use yew::{Callback, Component, Context, Html, Properties, html, html_nested, platform::spawn_local};

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
    /// „Funkbrücke“ from a free router port: the bridge devices to choose from, and the chosen
    bridges: Option<Result<Vec<Bridge>, FrontendError>>,
    bridge: Option<Bridge>,
}

pub enum HelperFlowMsg {
    Phase(Phase),
    Bridges(Result<Vec<Bridge>, FrontendError>),
    Bridge(Bridge),
}

/// A bridge device to cable to the router port.
#[derive(Debug, Clone, PartialEq)]
pub struct Bridge {
    id: u32,
    text: String,
}

impl Display for Bridge {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Component for HelperFlow {
    type Message = HelperFlowMsg;
    type Properties = HelperFlowProps;

    fn create(ctx: &Context<Self>) -> Self {
        if matches!(ctx.props().preset, HelperPreset::RadioChoose { .. }) {
            fetch_bridges(ctx);
        }
        Self {
            phase: Phase::Plan,
            bridges: None,
            bridge: None,
        }
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
            HelperFlowMsg::Bridges(bridges) => {
                self.bridges = Some(bridges);
                true
            }
            HelperFlowMsg::Bridge(bridge) => {
                self.bridge = Some(bridge);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        // from a free router port: the chosen bridge device's helper, as from a cabled one
        let preset = match (&props.preset, &self.bridge) {
            (HelperPreset::RadioChoose { router_device, router_port }, Some(bridge)) => {
                Some(HelperPreset::RadioFrom(RadioPreset {
                    bridge: bridge.id,
                    router_device: *router_device,
                    router_port: *router_port,
                }))
            }
            (HelperPreset::RadioChoose { .. }, None) => None,
            (preset, _) => Some(preset.clone()),
        };
        // another device's helper (a bridge device's from the router port) reaches it at its
        // own address, with the credentials chosen
        let helper_device = preset.as_ref().map_or(props.id, |preset| preset.device(props.id));
        let target = if helper_device == props.id {
            props.target.clone()
        } else {
            SelectedTarget {
                address: None,
                ..props.target.clone()
            }
        };
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
                        if matches!(props.preset, HelperPreset::RadioChoose { .. }) {
                            {self.bridge_choice(ctx)}
                        }
                        if let Some(preset) = preset {
                            <NetboxHelper
                                key={helper_device}
                                id={helper_device}
                                target={target}
                                {preset}
                                embedded=true
                                on_phase={ctx.link().callback(HelperFlowMsg::Phase)}
                                on_written={props.on_written.clone()}
                            />
                        }
                    </Modal>
                </Bullseye>
            </div>
        }
    }
}

impl HelperFlow {
    /// The bridge device to cable to the router port, chosen while nothing is written.
    fn bridge_choice(&self, ctx: &Context<Self>) -> Html {
        let bridges = match &self.bridges {
            None => return html!(<Spinner size={SpinnerSize::Md}/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(bridges)) => bridges,
        };
        let locked = !matches!(self.phase, Phase::Plan | Phase::Preview);
        html! {
            <Form>
                <FormGroup label={t::BRIDGE_DEVICE}>
                    if bridges.is_empty() {
                        <p>{t::NO_BRIDGE_DEVICES}</p>
                    } else if locked {
                        <p>{ self.bridge.as_ref().map(|b| b.text.clone()).unwrap_or_default() }</p>
                    } else {
                        <Select<Bridge>
                            placeholder={t::CHOOSE_BRIDGE_DEVICE}
                            selected={self.bridge.clone()}
                            entries={bridges.clone()}
                            onselect={ctx.link().callback(HelperFlowMsg::Bridge)}
                        />
                    }
                </FormGroup>
            </Form>
        }
    }
}

/// The bridge devices without a cable to a router, or with one to this router port (a Wave
/// Nano cabled before its link was in NetBox).
fn fetch_bridges(ctx: &Context<HelperFlow>) {
    let scope = ctx.link().clone();
    let id = ctx.props().id;
    let from = match &ctx.props().preset {
        HelperPreset::RadioChoose { router_port, .. } => Some(*router_port),
        _ => None,
    };
    spawn_local(async move {
        let bridges = query_authenticated::<RadioBridgeCandidates, _>(
            scope.clone(),
            radio_bridge_candidates::Variables { id: id as i64 },
        )
        .await
        .map(|data| {
            data.topology
                .device_by_id
                .map(|device| device.radio_bridge_candidates)
                .unwrap_or_default()
                .into_iter()
                .filter(|candidate| {
                    candidate
                        .router_port
                        .as_ref()
                        .is_none_or(|port| Some(port.id as u32) == from)
                })
                .map(|candidate| Bridge {
                    id: candidate.device.id as u32,
                    text: t::bridge_device(
                        &candidate.device.name,
                        &candidate.radio.name,
                        candidate.peer.as_ref().map(|peer| peer.name.as_str()),
                    ),
                })
                .collect()
        });
        scope.send_message(HelperFlowMsg::Bridges(bridges));
    });
}
