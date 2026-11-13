//! Scope: The subscription for a job and its events; nothing else.
//!
//! A job and its events (`job`, `components::job`), subscribed: each answer brings the events
//! after `after`; and the button to cancel it (`cancelJob`).
use super::{
    schema::schema,
    types::{
        DeviceModeChange, ErrorInfo, JobId, JobKind, JobPhase, JobState, LeftoverAction, Partition,
        Watching, WideInt,
    },
};

#[derive(cynic::QueryVariables, Debug)]
pub struct JobEventsVariables {
    pub id: i32,
    pub after: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "JobEventsVariables"
)]
pub struct JobEvents {
    #[arguments(id: $id)]
    pub job: Option<Job>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", variables = "JobEventsVariables")]
pub struct Job {
    pub id: i32,
    pub kind: JobKind,
    pub state: JobState,
    pub error: Option<ErrorInfo>,
    pub took: Option<u64>,
    pub phase: Option<JobPhase>,
    #[arguments(after: $after)]
    pub events: Vec<Logged>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Logged {
    pub index: i32,
    /// since the start of the job, in nanoseconds
    pub at: u64,
    pub event: Event,
}

/// What happened; one this page doesn't know yet (and a site's diff, never a device's job) is
/// `Unknown`.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub enum Event {
    Connecting(Connecting),
    Connected(Connected),
    UpdateTarget(UpdateTarget),
    WatchdogNotSet(WatchdogNotSet),
    PoeChain(PoeChain),
    WaitingForDevice(WaitingForDevice),
    UpdateStart(UpdateStart),
    UpdateStepPlanned(UpdateStepPlanned),
    SshPasswordLogin(SshPasswordLogin),
    PackageDownloaded(PackageDownloaded),
    PackageUploading(PackageUploading),
    PackageUploaded(PackageUploaded),
    Reboot(Reboot),
    PowerCycled(PowerCycled),
    RouterOsRunning(RouterOsRunning),
    FirmwareUpgrade(FirmwareUpgrade),
    UpdateDone(UpdateDone),
    Downgraded(Downgraded),
    LeftoverCleared(LeftoverCleared),
    ApplyPlanned(ApplyPlanned),
    FallbackArmed(FallbackArmed),
    FallbackUnavailable(FallbackUnavailable),
    ApplyRunning(ApplyRunning),
    ApplyWritten(ApplyWritten),
    ApplyStarted(ApplyStarted),
    ApplyChecked(ApplyChecked),
    FallbackStopped(FallbackStopped),
    FallbackLeft(FallbackLeft),
    FallbackFired(FallbackFired),
    ScriptProgress(ScriptProgress),
    RadiosReprovisioned(RadiosReprovisioned),
    PasswordSet(PasswordSet),
    WwwCertificateInstalled(WwwCertificateInstalled),
    BatchApply(BatchApply),
    BatchStopped(BatchStopped),
    BatchIncomplete(BatchIncomplete),
    SetupStarted(SetupStarted),
    ConsoleOutput(ConsoleOutput),
    WaitingForApi(WaitingForApi),
    SetupDone(SetupDone),
    DeviceModeConfirm(DeviceModeConfirm),
    DeviceModeChanged(DeviceModeChanged),
    RepartitionStarted(RepartitionStarted),
    Repartitioned(Repartitioned),
    PartitionRunning(PartitionRunning),
    PartitionSynced(PartitionSynced),
    UpdateFallbackArmed(UpdateFallbackArmed),
    UpdateFallbackDisarmed(UpdateFallbackDisarmed),
    UpdateWithoutFallback(UpdateWithoutFallback),
    OspfNeighborsBack(OspfNeighborsBack),
    WatchdogSuspended(WatchdogSuspended),
    WatchdogRestored(WatchdogRestored),
    WatchdogNotRestored(WatchdogNotRestored),
    WatchdogLeft(WatchdogLeft),
    Resumed(Resumed),
    WatchdogKept(WatchdogKept),
    WatchdogStillHeld(WatchdogStillHeld),
    JournalNotWritten(JournalNotWritten),
    NetboxWritten(NetboxWritten),
    NetboxUndone(NetboxUndone),
    #[cynic(fallback)]
    Unknown,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Connecting {
    pub device: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Connected {
    pub device: String,
    pub address: String,
    pub neighbor: Option<String>,
    pub identity: String,
    pub same_name: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateTarget {
    pub version: String,
    pub channel: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogNotSet {
    pub device: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoeChain {
    pub suppliers: Vec<String>,
    pub consumers: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WaitingForDevice {
    pub device: String,
    pub reason: DeviceWait,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateStart {
    pub version: String,
    pub architecture: String,
    pub packages: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateStepPlanned {
    pub version: String,
    pub files: Vec<String>,
    pub uninstall: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SshPasswordLogin {
    pub setting: String,
    pub value: String,
    pub restored: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PackageDownloaded {
    pub file: String,
    pub bytes: WideInt,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PackageUploading {
    pub file: String,
    pub bytes: WideInt,
    pub total: WideInt,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PackageUploaded {
    pub file: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Reboot {
    pub reason: RebootReason,
    pub timeout: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PowerCycled {
    pub supplier: String,
    pub port: String,
    pub device: String,
    pub seconds: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RouterOsRunning {
    pub version: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FirmwareUpgrade {
    pub current: String,
    pub upgrade: String,
    pub running: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateDone {
    pub from: String,
    pub to: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Downgraded {
    pub from: String,
    pub to: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LeftoverCleared {
    pub action: LeftoverAction,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApplyPlanned {
    pub script: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FallbackArmed {
    pub delay: u64,
    pub window: Option<u64>,
    pub radio_wait: Option<u64>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FallbackUnavailable {
    pub device_mode: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApplyRunning {
    pub timeout: u64,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApplyWritten {
    pub lead: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApplyStarted {
    pub starts_in: u64,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ApplyChecked {
    pub remaining: String,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FallbackStopped {
    pub after: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FallbackLeft {
    pub fires_in: u64,
    pub reason: FallbackLeftReason,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FallbackFired {
    pub after: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ScriptProgress {
    pub lines: i32,
    pub section: Option<i32>,
    pub done: bool,
    pub error_line: Option<i32>,
    pub error: Option<String>,
    pub probe: Option<String>,
    pub probe_answered: bool,
    pub lost: Vec<i32>,
    pub back: Vec<i32>,
    pub rearmed: Option<u64>,
    pub after_fallback: bool,
    pub marks: String,
}

/// The radios of a CAPsMAN provisioned again (`CapsmanRadios`).
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum CapsmanRadios {
    Wifi,
    Legacy,
    #[cynic(fallback)]
    Other(String),
}

impl CapsmanRadios {
    /// The RouterOS menu, as the job names it.
    pub fn menu(&self) -> &str {
        match self {
            CapsmanRadios::Wifi => "/interface/wifi/radio",
            CapsmanRadios::Legacy => "/caps-man/radio",
            CapsmanRadios::Other(other) => other,
        }
    }
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RadiosReprovisioned {
    pub menu: CapsmanRadios,
    pub count: i32,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WwwCertificateInstalled {
    pub name: Option<String>,
    pub days: i32,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PasswordSet {
    pub user: String,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BatchApply {
    pub devices: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BatchStopped {
    pub device: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BatchIncomplete {
    pub devices: Vec<String>,
    pub not_applied: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SetupStarted {
    pub neighbor: String,
    pub neighbor_port: String,
    pub macs: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ConsoleOutput {
    pub text: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WaitingForApi {
    pub address: String,
    pub timeout: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SetupDone {
    pub address: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceModeConfirm {
    pub changes: Vec<DeviceModeChange>,
    pub timeout: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceModeChanged {
    pub changes: Vec<DeviceModeChange>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RepartitionStarted {
    pub count: i32,
    pub files: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Repartitioned {
    pub partitions: Vec<Partition>,
    pub missing_files: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PartitionRunning {
    pub name: String,
    pub version: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PartitionSynced {
    pub partition: Option<String>,
    pub config_only: bool,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateFallbackArmed {
    pub partition: String,
    pub version: String,
    pub delay: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateFallbackDisarmed {
    pub partition: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct UpdateWithoutFallback {
    pub partitions: i32,
    pub neighbors: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct OspfNeighborsBack {
    pub neighbors: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogSuspended {
    pub device: String,
    pub watching: Watching,
    pub safety_net: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogRestored {
    pub device: String,
    pub watching: Watching,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogNotRestored {
    pub device: String,
    pub watching: Watching,
    pub error: ErrorInfo,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogLeft {
    pub device: String,
    pub error: ErrorInfo,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Resumed {
    pub after: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogKept {
    pub device: String,
    pub watching: Watching,
    pub safety_net: u64,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogStillHeld {
    pub device: String,
    pub others: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct JournalNotWritten {
    pub error: ErrorInfo,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NetboxWritten {
    pub path: String,
    pub display: String,
    pub changes: Vec<FieldChange>,
    pub deleted: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NetboxUndone {
    pub path: String,
    pub display: String,
    pub deleted: bool,
    pub recreated: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FieldChange {
    pub field: String,
    pub value: Option<String>,
    pub previous: Option<String>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct CancelJobVariables {
    pub id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "CancelJobVariables"
)]
pub struct CancelJob {
    #[arguments(id: $id)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub cancel_job: JobId,
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum FallbackLeftReason {
    Cancelled,
    NotReached,
    NotStopped,
    Due,
    NotApplied,
    BatchIncomplete,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum DeviceWait {
    SupplierRebooting,
    ConsumerUpdating,
    BehindRebooting,
    BehindUpdating,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RebootReason {
    Packages,
    Firmware,
    Requested,
    DeviceMode,
    Repartition,
    Partition,
    Downgrade,
    ApplyFallback,
    #[cynic(fallback)]
    Other(String),
}
