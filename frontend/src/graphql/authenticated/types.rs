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
/// An `Int` the backend fills from a `u64`, beyond the 32 bits of GraphQL's `Int` (bytes, the
/// hosts of an IPv6 pool): an `i32` would fail the whole answer.
#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(transparent)]
pub struct WideInt(pub u64);
cynic::impl_scalar!(WideInt, schema::Int);

/// The state of an apply (`ApplyState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
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
    Other(String),
}

/// The state of an update or a downgrade (`UpdateState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
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
    Other(String),
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

/// Where a job stands (`JobState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
    #[cynic(fallback)]
    Other(String),
}

/// An error the backend reports (`ErrorInfo`), worded through `error::ServerProblem`.
#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ErrorInfo {
    pub code: String,
    pub params: Vec<ErrorParam>,
    pub message: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ErrorParam {
    pub name: String,
    pub value: String,
}

/// What applying would cut or leave behind (`ApplyWarning`), shown before applying.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub enum ApplyWarning {
    PoeCut(PoeCut),
    CapOffOutsideWlanGroup(CapOffOutsideWlanGroup),
    WatchTargetSilent(WatchTargetSilent),
    ApiClientsOutside(ApiClientsOutside),
    #[cynic(fallback)]
    Unknown,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoeCut {
    pub port: String,
    pub default_name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CapOffOutsideWlanGroup {
    pub package: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchTargetSilent {
    pub address: String,
    pub port: Option<String>,
    pub device: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApiClientsOutside {
    pub clients: Vec<String>,
}

/// What doesn't fit at a PoE port (`PoeProblem`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum PoeProblem {
    NoPoeOut,
    NoLoad,
    PdMissing,
    FeedsInstead,
    Undocumented,
    #[cynic(fallback)]
    Other,
}

/// The state a job is in, by its kind (`JobPhaseObject`); each phase's `state` is aliased, the
/// union can't merge fields of one name and different types.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "JobPhaseObject")]
pub enum JobPhase {
    ApplyPhase(ApplyPhase),
    UpdatePhase(UpdatePhase),
    RebootPhase(RebootPhase),
    PowerCyclePhase(PowerCyclePhase),
    DeviceModePhase(DeviceModePhase),
    BootstrapPhase(BootstrapPhase),
    CleanupPhase(CleanupPhase),
    NetboxPhase(NetboxPhase),
    #[cynic(fallback)]
    Unknown,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApplyPhase {
    #[cynic(alias, rename = "state")]
    pub apply: ApplyState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdatePhase {
    #[cynic(alias, rename = "state")]
    pub update: UpdateState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RebootPhase {
    #[cynic(alias, rename = "state")]
    pub reboot: RebootState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PowerCyclePhase {
    #[cynic(alias, rename = "state")]
    pub power_cycle: PowerCycleState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceModePhase {
    #[cynic(alias, rename = "state")]
    pub device_mode: DeviceModeState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BootstrapPhase {
    #[cynic(alias, rename = "state")]
    pub bootstrap: BootstrapState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CleanupPhase {
    #[cynic(alias, rename = "state")]
    pub cleanup: CleanupState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NetboxPhase {
    #[cynic(alias, rename = "state")]
    pub netbox: NetboxState,
}

/// The state of a reboot (`RebootState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RebootState {
    Reading,
    WaitingForChain,
    Rebooting,
    #[cynic(fallback)]
    Other(String),
}

/// The state of a power cycle (`PowerCycleState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum PowerCycleState {
    Reading,
    WaitingForChain,
    Cutting,
    WaitingForDevice,
    #[cynic(fallback)]
    Other(String),
}

/// The state of a device-mode change (`DeviceModeState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum DeviceModeState {
    Reading,
    WaitingForChain,
    Requesting,
    WaitingForConfirmation,
    Rebooting,
    #[cynic(fallback)]
    Other(String),
}

/// The state of a setup (`BootstrapState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum BootstrapState {
    SuspendingWatchdogs,
    CheckingNeighbor,
    Console,
    WaitingForApi,
    RestoringWatchdogs,
    #[cynic(fallback)]
    Other(String),
}

/// The state of a cleanup (`CleanupState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum CleanupState {
    Clearing,
    WaitingForChain,
    Rebooting,
    #[cynic(fallback)]
    Other(String),
}

/// The state of a NetBox write (`NetboxState`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum NetboxState {
    Planning,
    Writing,
    Undoing,
    Refetching,
    #[cynic(fallback)]
    Other(String),
}

/// Which way PoE goes at a port (`PoeMode`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum PoeMode {
    Pse,
    Pd,
    #[cynic(fallback)]
    Other(String),
}

/// The NetBox RF role of a radio (`RadioRole`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RadioRole {
    Ap,
    Station,
    #[cynic(fallback)]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::{PoeProblem, WideInt};

    /// The free hosts of an IPv6 pool come as `u64::MAX` (`topology::findings`).
    #[test]
    fn wide_int_takes_more_than_32_bits() {
        let wide: WideInt = serde_json::from_str("18446744073709551615").unwrap();
        assert_eq!(wide, WideInt(u64::MAX));
    }

    /// A value a newer backend added doesn't fail the answer.
    #[test]
    fn unknown_enum_value_falls_back() {
        let problem: PoeProblem = serde_json::from_str("\"SOMETHING_NEW\"").unwrap();
        assert_eq!(problem, PoeProblem::Other);
    }
}
