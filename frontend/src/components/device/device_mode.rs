//! Scope: The device mode on the device page, with turning on the features the provisioner needs;
//! nothing else.
use crate::{
    components::widgets::confirm::Confirm,
    graphql::authenticated::device::DeviceMode as Mode,
    i18n::{device as t, device_mode},
};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DeviceModeProps {
    pub mode: Mode,
    /// a job runs, or writing is locked
    pub disabled: bool,
    /// „Device-Mode anpassen“, confirmed
    pub on_change: Callback<()>,
}

/// The device mode as a `<dt>`/`<dd>` pair, with the features the provisioner needs turned on
/// where some are off.
pub struct DeviceMode {
    /// „Device-Mode anpassen“ was clicked, waiting for the confirmation
    confirm: bool,
}

pub enum DeviceModeMsg {
    Confirm(bool),
    Change,
}

impl Component for DeviceMode {
    type Message = DeviceModeMsg;
    type Properties = DeviceModeProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { confirm: false }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DeviceModeMsg::Confirm(confirm) => self.confirm = confirm,
            DeviceModeMsg::Change => {
                self.confirm = false;
                ctx.props().on_change.emit(());
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let mode = &ctx.props().mode;
        let changes = mode.changes.iter().map(|c| (&c.feature, &c.reason));
        let missing = device_mode::features(changes);
        let adjust = Confirm {
            asking: self.confirm,
            question: t::DEVICE_MODE_CONFIRM.into(),
            now: t::DEVICE_MODE_NOW,
            ask: t::DEVICE_MODE_ADJUST.into(),
            disabled: ctx.props().disabled,
        };
        html! {<>
            <dt>{t::DEVICE_MODE}</dt><dd>
                { device_mode::name(&mode.mode) }
                if !missing.is_empty() {
                    { t::device_mode_missing(&missing) }
                    <div class="device-mode">
                        { adjust.view(
                            ctx.link().callback(|_| DeviceModeMsg::Change),
                            ctx.link().callback(DeviceModeMsg::Confirm),
                        ) }
                    </div>
                }
            </dd>
        </>}
    }
}
