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
