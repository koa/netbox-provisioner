//! Scope: The query for the user's role and the residents it sees; nothing else.
use crate::graphql::schema::schema;

/// What the user is: an admin sees the admins' pages, a resident only its own.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct WhoAmI {
    pub me: Me,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct Me {
    pub role: RoleKind,
}

#[derive(cynic::Enum, Debug, Clone, PartialEq, Eq)]
pub enum RoleKind {
    Admin,
    Resident,
    Nobody,
    /// a role of a newer server
    #[cynic(fallback)]
    Other(String),
}

/// The residents the user sees: a resident itself, an admin every one.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct Residents {
    pub residents: Vec<Resident>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct Resident {
    pub id: i32,
    pub name: String,
    pub ports: Vec<ResidentPort>,
    pub networks: Vec<ResidentNetwork>,
    pub devices: Vec<ResidentDevice>,
    pub locations: Vec<ResidentLocation>,
}

/// A room a device of the resident can stand in.
#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ResidentLocation {
    pub id: i32,
    pub path: Vec<String>,
    pub site: Option<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ResidentDevice {
    pub id: i32,
    pub name: String,
    pub routeros: bool,
    pub uplink: Option<String>,
    pub ports: Vec<Named>,
    pub location: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct Named {
    pub id: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ResidentPort {
    pub id: i32,
    pub kind: PortKind,
    pub name: String,
    pub label: Option<String>,
    pub device: Named,
    pub site: Option<Named>,
    pub location: Vec<String>,
    pub on_own_device: bool,
    pub network: Option<Named>,
    pub foreign_network: bool,
    pub far_ends: Vec<FarEnd>,
    pub routeros: bool,
}

#[derive(cynic::Enum, Debug, Clone, PartialEq, Eq)]
pub enum PortKind {
    Interface,
    Socket,
    /// a kind of a newer server
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct FarEnd {
    pub device: Named,
    pub port: String,
    pub own_device: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ResidentNetwork {
    pub id: i32,
    pub name: String,
    pub wlans: Vec<ResidentWlan>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ResidentWlan {
    pub id: i32,
    pub ssid: String,
    pub enabled: bool,
    pub psk: Option<String>,
    pub access_points: i32,
}
