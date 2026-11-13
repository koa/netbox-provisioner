//! Scope: The words of a port's PoE state; nothing else.
use crate::graphql::authenticated::types::PoeOutStatus;

/// `poe-out-status`, by RouterOS's own name.
pub fn status(status: &PoeOutStatus) -> &str {
    match status {
        PoeOutStatus::Disabled => "disabled",
        PoeOutStatus::WaitingForLoad => "waiting-for-load",
        PoeOutStatus::ShortCircuit => "short-circuit",
        PoeOutStatus::PoweredOn => "powered-on",
        PoeOutStatus::Overload => "overload",
        PoeOutStatus::VoltageTooLow => "voltage-too-low",
        PoeOutStatus::VoltageTooHigh => "voltage-too-high",
        PoeOutStatus::CurrentTooLow => "current-too-low",
        PoeOutStatus::VoltageOnPoeIn => "voltage-on-poe-in",
        PoeOutStatus::Off => "off",
        PoeOutStatus::PowerReset => "power-reset",
        PoeOutStatus::ControllerInit => "controller-init",
        PoeOutStatus::ControllerUpgrade => "controller-upgrade",
        PoeOutStatus::ControllerError => "controller-error",
        PoeOutStatus::Other(other) => other,
    }
}
