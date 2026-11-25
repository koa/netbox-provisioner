//! Scope: The words of a job's state by its kind (`JobPhase`); nothing else.
use crate::graphql::authenticated::types::{
    ApplyState as A, BootstrapState as B, CleanupState as C, DeviceModeState as D,
    JobPhase as Phase, NetboxState as N, PowerCycleState as P, RebootState as R, UpdateState,
};
use ::i18n::job::phase as p;

/// The name of a job's state; `None` for a kind of a newer server.
pub fn phase(phase: &Phase) -> Option<String> {
    let other = |other: &str| other.to_string();
    Some(match phase {
        Phase::ApplyPhase(phase) => match &phase.apply {
            A::Planning => p::PLANNING.into(),
            A::InstallingCertificate => p::INSTALLING_CERTIFICATE.into(),
            A::CheckingInternet => p::CHECKING_INTERNET.into(),
            A::SuspendingWatchdogs => p::SUSPENDING_WATCHDOGS.into(),
            A::ArmingFallback => p::ARMING_FALLBACK.into(),
            A::WritingScript => p::WRITING_SCRIPT.into(),
            A::StartingScript => p::STARTING_SCRIPT.into(),
            A::ScriptRunning => p::SCRIPT_RUNNING.into(),
            A::TakingBack => p::TAKING_BACK.into(),
            A::StoppingFallback => p::STOPPING_FALLBACK.into(),
            A::NotReached => p::NOT_REACHED.into(),
            A::Checking => p::CHECKING.into(),
            A::Finishing => p::FINISHING.into(),
            A::Other(o) => other(o),
        },
        Phase::UpdatePhase(phase) => update_phase(&phase.update),
        Phase::RebootPhase(phase) => match &phase.reboot {
            R::Reading => p::READING.into(),
            R::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            R::Rebooting => p::REBOOTING.into(),
            R::Other(o) => other(o),
        },
        Phase::PowerCyclePhase(phase) => match &phase.power_cycle {
            P::Reading => p::READING.into(),
            P::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            P::Cutting => p::CUTTING.into(),
            P::WaitingForDevice => p::WAITING_FOR_DEVICE.into(),
            P::Other(o) => other(o),
        },
        Phase::DeviceModePhase(phase) => match &phase.device_mode {
            D::Reading => p::READING.into(),
            D::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            D::Requesting => p::REQUESTING.into(),
            D::WaitingForConfirmation => p::WAITING_FOR_CONFIRMATION.into(),
            D::Rebooting => p::REBOOTING.into(),
            D::Other(o) => other(o),
        },
        Phase::BootstrapPhase(phase) => match &phase.bootstrap {
            B::SuspendingWatchdogs => p::SUSPENDING_WATCHDOGS.into(),
            B::CheckingNeighbor => p::CHECKING_NEIGHBOR.into(),
            B::Console => p::CONSOLE.into(),
            B::WaitingForApi => p::WAITING_FOR_API.into(),
            B::RestoringWatchdogs => p::RESTORING_WATCHDOGS.into(),
            B::Other(o) => other(o),
        },
        Phase::CleanupPhase(phase) => match &phase.cleanup {
            C::Clearing => p::CLEARING.into(),
            C::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
            C::Rebooting => p::REBOOTING.into(),
            C::Other(o) => other(o),
        },
        Phase::NetboxPhase(phase) => match &phase.netbox {
            N::Planning => p::PLANNING.into(),
            N::Writing => p::WRITING.into(),
            N::Undoing => p::UNDOING.into(),
            N::Refetching => p::REFETCHING.into(),
            N::Other(o) => other(o),
        },
        Phase::Unknown => return None,
    })
}

/// The name of an update's state (`Job.phase`, a rollout's `RolloutDevice.phase`).
pub fn update_phase(state: &UpdateState) -> String {
    use UpdateState as U;
    match state {
        U::Reading => p::READING.into(),
        U::SuspendingWatchdogs => p::SUSPENDING_WATCHDOGS.into(),
        U::WaitingForChain => p::WAITING_FOR_CHAIN.into(),
        U::RememberingNeighbors => p::REMEMBERING_NEIGHBORS.into(),
        U::ArmingFallback => p::ARMING_FALLBACK.into(),
        U::Downloading => p::DOWNLOADING.into(),
        U::Uploading => p::UPLOADING.into(),
        U::WaitingToReboot => p::WAITING_TO_REBOOT.into(),
        U::Rebooting => p::REBOOTING.into(),
        U::NotUpdated => p::NOT_UPDATED.into(),
        U::StoppingFallback => p::STOPPING_FALLBACK.into(),
        U::WaitingForNeighbors => p::WAITING_FOR_NEIGHBORS.into(),
        U::Firmware => p::FIRMWARE.into(),
        U::WaitingToRebootFirmware => p::WAITING_TO_REBOOT_FIRMWARE.into(),
        U::RebootingFirmware => p::REBOOTING_FIRMWARE.into(),
        U::RestoringWatchdogs => p::RESTORING_WATCHDOGS.into(),
        U::Other(other) => other.clone(),
    }
}
