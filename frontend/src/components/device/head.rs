//! Scope: The head card of the device page: its name, board, the device the connection reached
//! and how it is reached, with the form for another address or credentials; nothing else.
use crate::{
    components::device::adjust_target::AdjustTarget,
    graphql::authenticated::device::Connected,
    i18n::{device as t, target},
    target::SelectedTarget,
};
use patternfly_yew::prelude::{
    Alert, AlertType, Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle,
};
use yew::{AttrValue, Callback, Component, Context, Html, Properties, html};

/// The board a device that answered reports, against NetBox's.
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    /// as `/system/routerboard` reports it
    pub board: Box<str>,
    /// the `model` of its NetBox device type
    pub netbox_device_type: Box<str>,
    pub matches: bool,
}

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DeviceHeadProps {
    /// its name in NetBox
    pub name: AttrValue,
    /// `None` if the device couldn't be reached
    pub board: Option<Board>,
    /// the device the connection reached, `None` if it couldn't be reached
    pub connected: Option<Connected>,
    pub target: SelectedTarget,
    pub on_target: Callback<SelectedTarget>,
}

/// The head; the form for another target opens in it on a device that answered (on one that
/// doesn't, the page shows it with the changes).
pub struct DeviceHead {
    show_target: bool,
}

impl Component for DeviceHead {
    /// open or close the form
    type Message = bool;
    type Properties = DeviceHeadProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { show_target: false }
    }

    fn update(&mut self, _ctx: &Context<Self>, show: bool) -> bool {
        self.show_target = show;
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let reachable = props.board.is_some();
        let show = !self.show_target;
        html! {
            <Card class="device-head">
                <CardHeader><CardTitle>{ props.name.clone() }</CardTitle></CardHeader>
                <CardBody>
                    <dl class="device-overview">
                        if let Some(board) = &props.board {
                            <dt>{t::BOARD}</dt><dd>
                                { board.board.as_ref() }
                                if !board.matches {
                                    <Alert inline=true r#type={AlertType::Warning}
                                        title={t::other_device_type(&board.netbox_device_type)}/>
                                }
                            </dd>
                        }
                        if let Some(connected) = &props.connected {
                            <dt>{t::CONNECTED}</dt><dd>
                                { t::connected(&connected.address, connected.neighbor.as_deref(), &connected.identity) }
                                if !connected.same_name {
                                    <Alert inline=true r#type={AlertType::Warning}
                                        title={t::other_identity(&connected.identity, &props.name)}/>
                                }
                            </dd>
                        }
                        <dt>{target::ACCESS}</dt><dd>
                            { target::describe(&props.target) }
                            if reachable {
                                {" "}
                                <Button
                                    variant={ButtonVariant::Link}
                                    label={if self.show_target { target::HIDE } else { target::SHOW }}
                                    onclick={ctx.link().callback(move |_| show)}
                                />
                            }
                        </dd>
                    </dl>
                    if reachable && self.show_target {
                        <AdjustTarget onchange={props.on_target.clone()} value={props.target.clone()}/>
                    }
                </CardBody>
            </Card>
        }
    }
}
