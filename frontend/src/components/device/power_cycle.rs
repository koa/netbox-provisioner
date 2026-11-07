//! Scope: „Strom aus und ein“ over the device feeding this one, and the supplier's job; nothing
//! else.
use crate::{
    components::{job::JobView, widgets::confirm::Confirm},
    graphql::authenticated::device::PowerSupply,
    i18n::device as t,
};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct PowerCycleProps {
    pub supply: PowerSupply,
    /// writing to devices is allowed; also while the device's own job runs (a device mode
    /// change waits for it)
    pub allow_apply: bool,
    /// the supplier's job switching the power off and on, once started
    pub job: Option<i32>,
    /// „Strom aus und ein“, confirmed
    pub on_cycle: Callback<()>,
}

/// „Strom aus und ein“, asked before, and the supplier's job below it.
pub struct PowerCycle {
    /// „Strom aus und ein“ was clicked, waiting for the confirmation
    confirm: bool,
}

pub enum PowerCycleMsg {
    Confirm(bool),
    Cycle,
}

impl Component for PowerCycle {
    type Message = PowerCycleMsg;
    type Properties = PowerCycleProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { confirm: false }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            PowerCycleMsg::Confirm(confirm) => self.confirm = confirm,
            PowerCycleMsg::Cycle => {
                self.confirm = false;
                ctx.props().on_cycle.emit(());
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let job = props.job.map(|id| {
            html! {
                <JobView key={id} id={id} on_finished={Callback::noop()}/>
            }
        });
        let cycle = Confirm {
            asking: self.confirm,
            question: t::power_cycle_confirm(&props.supply.device.name, &props.supply.port),
            now: t::POWER_CYCLE_NOW,
            ask: t::POWER_CYCLE.into(),
            disabled: !props.allow_apply,
        };
        html! {<>
            { cycle.view(
                ctx.link().callback(|_| PowerCycleMsg::Cycle),
                ctx.link().callback(PowerCycleMsg::Confirm),
            ) }
            {job}
        </>}
    }
}
