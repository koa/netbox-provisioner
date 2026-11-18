//! Scope: The warnings of an apply the operator should know before; not computing them (the step
//! that sees the case).
//!
//! What applying the generated config would do, or leave, that the operator should know before
//! (`AccessibleDevice.generateCfg`). Data only: the frontend words it.
use async_graphql::{SimpleObject, Union};

#[derive(Clone, Debug, PartialEq, Union)]
pub enum ApplyWarning {
    PoeCut(PoeCut),
    CapOffOutsideWlanGroup(CapOffOutsideWlanGroup),
    WatchTargetSilent(WatchTargetSilent),
    ApiClientsOutside(ApiClientsOutside),
    OspfAuthKept(OspfAuthKept),
}

/// The OSPF credentials of the segment at `port` aren't the ones the provisioner has for it,
/// and this apply leaves them (A10): only an apply of every end of the segment together moves
/// them, and only when nothing is in doubt; `doubt` says what is.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct OspfAuthKept {
    pub port: Box<str>,
    /// the segment's IPv4 network
    pub network: Box<str>,
    pub doubt: OspfAuthDoubt,
}

#[derive(Clone, Debug, PartialEq, Union)]
pub enum OspfAuthDoubt {
    EndsOutside(OspfEndsOutside),
    EndNotOspf(OspfEndNotOspf),
    ForeignNeighbor(OspfForeignNeighbor),
    NeighborMissing(OspfNeighborMissing),
    CredentialsDiffer(OspfCredentialsDiffer),
}

/// Ends of the segment that aren't in the apply.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct OspfEndsOutside {
    pub devices: Box<[Box<str>]>,
}

/// NetBox has `address` in the segment where it is no OSPF end the provisioner sets up: not
/// on an interface with `ospf` of a RouterOS device with a primary IPv4, or with another
/// prefix length, or the device's config has no OSPF port in the segment there.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct OspfEndNotOspf {
    pub address: Box<str>,
    pub device: Option<Box<str>>,
}

/// `device` has an OSPF neighbor at the segment's port that is none of its ends.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct OspfForeignNeighbor {
    pub device: Box<str>,
    pub router_id: Box<str>,
}

/// `device` doesn't see the end `missing` as a neighbor at the segment's port (`Full` or
/// `2-Way`), in OSPFv2 or v3: the segment may be down.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct OspfNeighborMissing {
    pub device: Box<str>,
    pub missing: Box<str>,
}

/// The ends have different credentials now: they aren't in line with each other.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct OspfCredentialsDiffer {
    pub devices: Box<[Box<str>]>,
}

/// Clients connected to the plain API right now (the provisioner among them) whose address
/// `api_networks` doesn't hold: the API stays as the device has it, as the restriction would
/// lock them out (the provisioner reaches lab-wap over IPv6, which a list of IPv4 networks
/// misses).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct ApiClientsOutside {
    pub clients: Box<[Box<str>]>,
}

/// A watchdog would watch `address` (on the PoE port `port`, else `/system/watchdog`), but it
/// doesn't answer from the device: what the device has there stays, since watching what is
/// silent reboots or power cycles for nothing (a device just set up has no loopback yet).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct WatchTargetSilent {
    pub address: Box<str>,
    /// the name the target gives the port
    pub port: Option<Box<str>>,
    /// the device NetBox has at the address
    pub device: Box<str>,
}

/// The PoE out of `port` powers something, and the target switches it off (NetBox has no
/// `poe_mode=pse` there).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct PoeCut {
    /// the name the target gives the port
    pub port: Box<str>,
    pub default_name: Box<str>,
}

/// The board has radios, but NetBox uses none of them and has the device in no Wireless LAN
/// Group: its CAP is off after the apply (`/interface/wifi` switched off, `/interface/wireless`
/// left off, as the initial setup leaves it), so it serves no WLAN.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct CapOffOutsideWlanGroup {
    /// the package of the board's radios (`hw_facts::radio_package`)
    pub package: Box<str>,
}
