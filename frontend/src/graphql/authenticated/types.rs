//! Types of the backend's schema that several queries share (cynic, [`super::schema`]).
use super::schema::schema;

/// A job's kind (`JobKind`); one this frontend doesn't know yet is `Other`, with its name.
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
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
#[cynic(schema = "authenticated")]
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
#[cynic(schema = "authenticated")]
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

/// How bad a finding is (`Severity`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum Severity {
    Error,
    Warning,
    Hint,
    #[cynic(fallback)]
    Other,
}

/// IPv4 or IPv6 (`IpVersion`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum IpVersion {
    V4,
    V6,
    #[cynic(fallback)]
    Other,
}

// The references the backend gives to NetBox objects (`*Ref`): id and what names it.

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceRef {
    pub id: i32,
    pub name: String,
    pub site: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct InterfaceRef {
    pub id: i32,
    pub name: String,
    pub device: DeviceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct AddressRef {
    pub id: i32,
    pub address: String,
}

impl AddressRef {
    pub fn is_v6(&self) -> bool {
        self.address.contains(':')
    }
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct VlanRef {
    pub id: i32,
    pub vid: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ServiceRef {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SiteRef {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WlanGroupRef {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PrefixRef {
    pub id: i32,
    pub prefix: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PortRef {
    pub name: String,
    pub interface: Option<InterfaceRef>,
}

/// A login typed in for one connection (`AdhocCredentials`).
#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct AdhocCredentials {
    pub username: Option<String>,
    pub password: Option<String>,
}
