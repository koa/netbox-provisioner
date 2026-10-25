//! What a job left on the device (`AccessibleDevice.leftovers`), each with what the operator
//! can do about it; nothing is cleared by itself.
use crate::{
    graphql::authenticated::device_overview::DeviceOverviewTopologyDeviceByIdAccessLeftovers as Left,
    i18n::{common::CANCEL, device::leftovers as t},
};
use patternfly_yew::prelude::{Alert, AlertType, Button, ButtonVariant};
use yew::{Callback, Component, Context, Html, Properties, html};

/// What the operator asked for (`LeftoverAction`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Leftover {
    StopApplyFallback,
    ApplyFallbackNow,
    StopUpdateFallback,
    RestoreWatchdogs,
    RemovePackages,
}

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct LeftoversProps {
    pub left: Left,
    /// a job runs, or writing is locked
    pub disabled: bool,
    pub on_clear: Callback<Leftover>,
}

pub struct Leftovers {
    /// "Jetzt zurück" was clicked, waiting for the confirmation
    confirm_now: bool,
}

pub enum LeftoversMsg {
    ConfirmNow(bool),
    Clear(Leftover),
}

impl Component for Leftovers {
    type Message = LeftoversMsg;
    type Properties = LeftoversProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { confirm_now: false }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            LeftoversMsg::ConfirmNow(confirm) => self.confirm_now = confirm,
            LeftoversMsg::Clear(leftover) => {
                self.confirm_now = false;
                ctx.props().on_clear.emit(leftover);
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let LeftoversProps { left, disabled, .. } = ctx.props();
        let disabled = *disabled;
        let button = |label: &'static str, leftover: Leftover| {
            html! {
                <Button
                    variant={ButtonVariant::Secondary}
                    label={label}
                    disabled={disabled}
                    onclick={ctx.link().callback(move |_| LeftoversMsg::Clear(leftover))}
                />
            }
        };
        html! {
            <Alert inline=true title={t::TITLE} r#type={AlertType::Warning}>
                <p>{t::TEXT}</p>
                <ul class="device-leftovers">
                    if let Some(fallback) = &left.apply_fallback {
                        <li>
                            {t::apply_fallback(fallback.next_run.as_deref())}
                            <div>
                                if self.confirm_now {
                                    {t::APPLY_FALLBACK_CONFIRM}
                                    <Button
                                        variant={ButtonVariant::Danger}
                                        label={t::APPLY_FALLBACK_NOW_CONFIRMED}
                                        disabled={disabled}
                                        onclick={ctx.link().callback(|_| LeftoversMsg::Clear(Leftover::ApplyFallbackNow))}
                                    />
                                    {" "}
                                    <Button
                                        variant={ButtonVariant::Link}
                                        label={CANCEL}
                                        onclick={ctx.link().callback(|_| LeftoversMsg::ConfirmNow(false))}
                                    />
                                } else {
                                    {button(t::STOP_APPLY_FALLBACK, Leftover::StopApplyFallback)}
                                    {" "}
                                    <Button
                                        variant={ButtonVariant::Secondary}
                                        label={t::APPLY_FALLBACK_NOW}
                                        disabled={disabled}
                                        onclick={ctx.link().callback(|_| LeftoversMsg::ConfirmNow(true))}
                                    />
                                }
                            </div>
                        </li>
                    }
                    if left.update_fallback.is_some() {
                        <li>
                            {t::UPDATE_FALLBACK}
                            <div>{button(t::STOP_UPDATE_FALLBACK, Leftover::StopUpdateFallback)}</div>
                        </li>
                    }
                    if let Some(watchdogs) = &left.watchdogs {
                        <li>
                            {t::watchdogs(
                                &watchdogs
                                    .watching
                                    .watch_address
                                    .iter()
                                    .map(|address| t::watch_address(address))
                                    .chain(watchdogs.watching.poe_pings.iter().map(|ping| t::poe_ping(&ping.port, &ping.address)))
                                    .collect::<Vec<_>>(),
                                watchdogs.next_run.as_deref(),
                            )}
                            <div>{button(t::RESTORE_WATCHDOGS, Leftover::RestoreWatchdogs)}</div>
                        </li>
                    }
                    if left.apply_script_running {
                        <li>{t::APPLY_SCRIPT_RUNNING}</li>
                    }
                    if !left.packages.is_empty() {
                        <li>
                            {t::packages(&left.packages.iter().map(|p| p.file.clone()).collect::<Vec<_>>())}
                            <div>{button(t::REMOVE_PACKAGES, Leftover::RemovePackages)}</div>
                        </li>
                    }
                </ul>
            </Alert>
        }
    }
}
