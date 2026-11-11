//! Scope: The device page's queries and mutations; not those of a card with a module of its own.
//!
//! The device page (`pages::site::device::show`): its overview, subscribed (the device, its jobs,
//! what the provisioner would change, its RouterOS, partitions, device mode, leftovers and log),
//! and the jobs it starts (apply, setup, update, downgrade, reboot, power cycle, …).
use super::{
    schema::schema,
    types::{
        ApplyWarning, Channel, DeviceModeChange, DeviceRef, DeviceTarget, DeviceVariablesFields,
        ErrorInfo, JobId, JobState, LeftoverAction, Mac, Partition, Watching,
    },
};

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct AllowApply {
    pub allow_apply: bool,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "DeviceVariables"
)]
pub struct RefreshDevice {
    #[arguments(id: $id)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub refresh_device: bool,
}

/// A device and how to reach it: another address, other credentials.
#[derive(cynic::QueryVariables, Debug)]
pub struct TargetVariables {
    pub id: i32,
    pub target: DeviceTarget,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "TargetVariables"
)]
pub struct DeviceOverview {
    pub topology: OverviewTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "TargetVariables"
)]
pub struct OverviewTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<OverviewDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "DeviceAccess",
    variables = "TargetVariables"
)]
pub struct OverviewDevice {
    pub name: String,
    pub jobs: Vec<DeviceJob>,
    pub bootstrap_plan: Option<BootstrapPlan>,
    pub bootstrap_problem: Option<ErrorInfo>,
    pub update_neighbors: Vec<String>,
    pub power_supply: Option<PowerSupply>,
    pub watchdogs: Vec<Watchdog>,
    #[arguments(
        target: $target,
    )]
    pub access: Option<OverviewAccess>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "Job")]
pub struct DeviceJob {
    pub id: i32,
    pub state: JobState,
    pub device_id: Option<i32>,
    pub device_name: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BootstrapPlan {
    pub neighbor: String,
    pub neighbor_port: String,
    pub port: String,
    pub address: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PowerSupply {
    pub device: DeviceRef,
    pub port: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Watchdog {
    pub device: String,
    pub supplier: bool,
    pub watching: Watching,
    pub problem: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "AccessibleDevice")]
pub struct OverviewAccess {
    pub connected: Connected,
    pub device_stats: DeviceStats,
    pub generate_cfg: Option<OverviewCfg>,
    pub device_mode: Option<DeviceMode>,
    pub partitions: Option<Partitions>,
    pub leftovers: Option<Leftovers>,
    pub log: Option<Vec<LogEntry>>,
    pub routeros: Option<Routeros>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Connected {
    pub address: String,
    pub neighbor: Option<String>,
    pub identity: String,
    pub same_name: bool,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated")]
pub struct DeviceStats {
    pub routerboard: Routerboard,
    pub netbox_device_type: String,
    pub board_matches: bool,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "GraphqlSystemRouterboard")]
pub struct Routerboard {
    pub device_type: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "GeneratedCfg")]
pub struct OverviewCfg {
    pub age: i32,
    pub script: String,
    pub warnings: Vec<ApplyWarning>,
    pub previous_password: bool,
    pub www_certificate: WwwCertificate,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum WwwCertificate {
    Off,
    InPlace,
    Needed,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceMode {
    pub mode: String,
    pub changes: Vec<DeviceModeChange>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Partitions {
    pub partitions: Vec<Partition>,
    pub can_split: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Leftovers {
    pub apply_fallback: Option<ScheduledLeft>,
    pub update_fallback: Option<ScheduledLeft>,
    pub watchdogs: Option<WatchdogsLeft>,
    pub apply_script_running: bool,
    pub packages: Vec<PackageLeft>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ScheduledLeft {
    pub next_run: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogsLeft {
    pub next_run: Option<String>,
    pub watching: Watching,
    pub held_by: Option<WatchdogsHeld>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WatchdogsHeld {
    pub job: i32,
    pub device: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PackageLeft {
    pub file: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LogEntry {
    pub time: String,
    pub topics: String,
    pub message: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "RouterOsVersions")]
pub struct Routeros {
    pub version: String,
    pub packages: Vec<String>,
    pub wanted: Vec<String>,
    pub default_channel: Channel,
    pub adjust_packages: bool,
    pub previous: Option<String>,
    pub channels: Vec<ChannelUpdate>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ChannelUpdate {
    pub channel: Channel,
    pub newest: String,
    pub upgrade: bool,
    pub update: bool,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "TargetVariables"
)]
pub struct ApplyDeviceCfg {
    #[arguments(
        id: $id,
        target: $target,
    )]
    pub apply_cfg: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "TargetVariables"
)]
pub struct RebootDevice {
    #[arguments(
        id: $id,
        target: $target,
    )]
    pub reboot_device: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "TargetVariables"
)]
pub struct ChangeDeviceMode {
    #[arguments(
        id: $id,
        target: $target,
    )]
    pub change_device_mode: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "TargetVariables"
)]
pub struct RepartitionDevice {
    #[arguments(
        id: $id,
        target: $target,
    )]
    pub repartition_device: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "DeviceVariables"
)]
pub struct PowerCycle {
    #[arguments(id: $id)]
    pub power_cycle: JobId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct BootstrapCandidates {
    pub topology: CandidatesTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct CandidatesTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<CandidatesDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct CandidatesDevice {
    pub bootstrap_plan: Option<CandidatesPlan>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "BootstrapPlan")]
pub struct CandidatesPlan {
    pub discovered: Vec<Discovered>,
}

/// A device the neighbor sees on its port, to set up.
#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Discovered {
    pub mac: Mac,
    pub identity: String,
    pub board: String,
    pub version: String,
    pub interface: String,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct BootstrapVariables {
    pub id: i32,
    pub mac: Mac,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "BootstrapVariables"
)]
pub struct BootstrapDevice {
    #[arguments(id: $id, mac: $mac)]
    pub bootstrap: JobId,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct UpdateVariables {
    pub id: i32,
    pub channel: Option<Channel>,
    pub target: DeviceTarget,
    pub suspend_watchdogs: Option<bool>,
    pub without_fallback: Option<bool>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "UpdateVariables"
)]
pub struct UpdateRouteros {
    #[arguments(
        id: $id,
        channel: $channel,
        target: $target,
        suspendWatchdogs: $suspend_watchdogs,
        withoutFallback: $without_fallback,
    )]
    pub update_routeros: JobId,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct DowngradeVariables {
    pub id: i32,
    pub version: String,
    pub target: DeviceTarget,
    pub suspend_watchdogs: Option<bool>,
    pub without_fallback: Option<bool>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "DowngradeVariables"
)]
pub struct DowngradeRouteros {
    #[arguments(
        id: $id,
        version: $version,
        target: $target,
        suspendWatchdogs: $suspend_watchdogs,
        withoutFallback: $without_fallback,
    )]
    pub downgrade_routeros: JobId,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ClearLeftoverVariables {
    pub id: i32,
    pub action: LeftoverAction,
    pub target: DeviceTarget,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "ClearLeftoverVariables"
)]
pub struct ClearLeftover {
    #[arguments(
        id: $id,
        action: $action,
        target: $target,
    )]
    pub clear_leftover: JobId,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct BootPartitionVariables {
    pub id: i32,
    pub partition: String,
    pub target: DeviceTarget,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "BootPartitionVariables"
)]
pub struct BootPartition {
    #[arguments(
        id: $id,
        partition: $partition,
        target: $target,
    )]
    pub boot_partition: JobId,
}
