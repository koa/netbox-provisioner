//! What applying the generated config would do, or leave, that the operator should know before
//! (`AccessibleDevice.generateCfg`). Data only: the frontend words it.
use async_graphql::{SimpleObject, Union};

#[derive(Clone, Debug, PartialEq, Union)]
pub enum ApplyWarning {
    PoeCut(PoeCut),
    RadioBound(RadioBound),
    LegacyRadioManaged(LegacyRadioManaged),
    CapOffOutsideWlanGroup(CapOffOutsideWlanGroup),
    WatchTargetSilent(WatchTargetSilent),
    ApiClientsOutside(ApiClientsOutside),
}

/// Clients connected to the plain API right now (the provisioner among them) whose address
/// `api_networks` doesn't hold: the API stays as the device has it, as the restriction would
/// lock them out (the provisioner reaches lab-wap over IPv6, which a list of IPv4 networks
/// misses).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct ApiClientsOutside {
    pub clients: Vec<Box<str>>,
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

/// CAPsMAN has bound `radio`, which RouterOS then refuses to change, and it uses the
/// configuration `configuration` instead of `expected` (NetBox's).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct RadioBound {
    pub radio: Box<str>,
    pub configuration: Option<Box<str>>,
    pub expected: Box<str>,
}

/// CAPsMAN manages the legacy radio `radio` (its name on the device), which RouterOS then
/// refuses to change: it keeps its name and settings, NetBox's name for it is `expected`.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct LegacyRadioManaged {
    pub radio: Box<str>,
    pub expected: Box<str>,
}

/// The board has radios, but NetBox uses none of them and has the device in no Wireless LAN
/// Group: its CAP is off after the apply (`/interface/wifi` switched off, `/interface/wireless`
/// left off, as the initial setup leaves it), so it serves no WLAN.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct CapOffOutsideWlanGroup {
    /// the package of the board's radios (`hw_facts::radio_package`)
    pub package: Box<str>,
}
