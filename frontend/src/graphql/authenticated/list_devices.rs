//! The sites with their devices (sites page, breadcrumb) and a site's devices (devices page).
use super::{schema::schema, types::Severity};

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "SubscriptionAuthenticated")]
pub struct ListSites {
    pub topology: SitesTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "TopologyHolder")]
pub struct SitesTopology {
    pub all_sites: Vec<Site>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct Site {
    pub id: i32,
    pub name: String,
    pub devices: Vec<SiteDevice>,
    pub findings: Vec<FindingSeverity>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct SiteDevice {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "Finding")]
pub struct FindingSeverity {
    pub severity: Severity,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SiteDevicesVariables {
    pub site: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "SiteDevicesVariables"
)]
pub struct SiteDevices {
    pub topology: SiteDevicesTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "SiteDevicesVariables"
)]
pub struct SiteDevicesTopology {
    #[arguments(id: $site)]
    pub site_by_id: Option<SiteWithDevices>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct SiteWithDevices {
    pub devices: Vec<DeviceRow>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct DeviceRow {
    pub id: i32,
    pub name: String,
    pub management_address: Option<ManagementAddress>,
    pub serial: Option<String>,
    pub findings: Vec<FindingSeverity>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "IpAddressAccess")]
pub struct ManagementAddress {
    pub address: Option<IpNet>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "IpNetGraphql")]
pub struct IpNet {
    pub ip: String,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct PingDeviceVariables {
    pub id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "PingDeviceVariables"
)]
pub struct PingDevice {
    pub topology: PingTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "PingDeviceVariables"
)]
pub struct PingTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<PingedDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct PingedDevice {
    pub access: Option<Pinged>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "AccessibleDevice")]
pub struct Pinged {
    pub ping: Vec<PingResult>,
    pub device_stats: DeviceStats,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated")]
pub struct PingResult {
    pub duration: u64,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated")]
pub struct DeviceStats {
    pub routerboard: Routerboard,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "GraphqlSystemRouterboard")]
pub struct Routerboard {
    pub device_type: String,
    pub serial_number: String,
}
