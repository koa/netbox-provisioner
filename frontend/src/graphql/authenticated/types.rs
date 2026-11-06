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

/// Where a job stands (`JobState`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
    #[cynic(fallback)]
    Other,
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
