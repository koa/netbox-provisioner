//! Types of the backend's schema that several queries share (cynic, [`super::schema`]).
use super::schema::schema;

/// A job's kind (`JobKind`); one this frontend doesn't know yet is `Other`, with its name.
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    Update,
    Downgrade,
    Apply,
    Cleanup,
    Bootstrap,
    Reboot,
    PowerCycle,
    DeviceMode,
    Repartition,
    BootPartition,
    NetboxWrite,
    NetboxUndo,
    SiteDiff,
    #[cynic(fallback)]
    Other(String),
}

// nanoseconds (`ScalarDuration`)
cynic::impl_scalar!(u64, schema::ScalarDuration);

/// The state of an apply (`ApplyState`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyState {
    Planning,
    SuspendingWatchdogs,
    ArmingFallback,
    WritingScript,
    StartingScript,
    ScriptRunning,
    TakingBack,
    StoppingFallback,
    NotReached,
    Checking,
    Finishing,
    #[cynic(fallback)]
    Other,
}

/// The state of an update or a downgrade (`UpdateState`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Reading,
    SuspendingWatchdogs,
    WaitingForChain,
    RememberingNeighbors,
    ArmingFallback,
    Downloading,
    Uploading,
    WaitingToReboot,
    Rebooting,
    NotUpdated,
    StoppingFallback,
    WaitingForNeighbors,
    Firmware,
    WaitingToRebootFirmware,
    RebootingFirmware,
    RestoringWatchdogs,
    #[cynic(fallback)]
    Other,
}
