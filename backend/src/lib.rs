extern crate core;

use crate::device::ros::SetupError;
use mikrotik_model::resource::{MissingDependenciesError, ResourceMutationError};
use thiserror::Error;

pub mod config;
pub mod context;
pub mod device;
pub mod error_info;
pub mod graphql;
pub mod helper;
pub mod job;
pub mod netbox;
pub mod topology;

#[derive(Error, Debug)]
pub enum Error {
    /// the batch stopped before anything was applied, because of `0`
    #[error("the batch stopped before applying anything: {0} failed")]
    BatchStopped(Box<str>),
    /// the batch's script didn't run on the device within `0` of starting it
    #[error("the apply script didn't start within {0:?}")]
    ScriptNotStarted(std::time::Duration),
    /// devices of the batch weren't reached after it: all of it goes back
    #[error("not reached after the apply: {0}; every device of the batch loads its config from before")]
    BatchIncomplete(Box<str>),
    #[error("Error accessing device: {0}")]
    MikrotikApi(#[from] mikrotik_api::error::Error),
    #[error("Error on data model: {0}")]
    MikrotikModel(#[from] mikrotik_model::resource::Error),
    #[error("Error credentials")]
    MissingCredentials,
    #[error("Cannot parse ip address {0}")]
    AddressParse(#[from] std::net::AddrParseError),
    #[error("Cannot generate mutations: {0}")]
    /// boxed: it is large (over 2 KB), and every `Result<_, Error>` would be as large
    ResourceMutation(Box<ResourceMutationError>),
    #[error("{0}")]
    MissingDependenciesError(Box<str>),
    #[error("Cannot write the script: {0}")]
    ErrorGeneratingString(#[from] std::fmt::Error),
    #[error("Cannot build setup {0}")]
    SetupError(#[from] SetupError),
    #[error("Cannot connect through the management proxy {0}: {1}")]
    Proxy(std::net::SocketAddr, tokio_socks::Error),
    #[error("Writing to devices is disabled (allow_apply)")]
    ApplyNotAllowed,
    #[error("No device {0}")]
    UnknownDevice(u32),
    #[error("No site {0}")]
    UnknownSite(u32),
    #[error("No interface {0}")]
    UnknownInterface(u32),
    #[error("No topology: NetBox couldn't be read")]
    NoTopology,
    #[error("No answer from {0}")]
    Timeout(std::net::IpAddr),
    #[error("{0} has no address in NetBox")]
    NoAddress(Box<str>),
    #[error("Console: {0}")]
    Terminal(#[from] device::terminal::TerminalError),
    #[error("No way known to set up {0}: {1}")]
    NoBootstrapPath(Box<str>, error_info::BootstrapObstacle),
    /// over mac-telnet to these MACs, with every credentials tried
    #[error("Login over mac-telnet to {0} refused")]
    ConsoleLoginRefused(Box<str>),
    /// over mac-telnet to these MACs, within the login timeout
    #[error("No answer to the login over mac-telnet to {0}")]
    ConsoleNoAnswer(Box<str>),
    /// a setup command (passwords replaced by `***`) and the error the console printed
    #[error("Setup command failed: {0}: {1}")]
    SetupCommand(Box<str>, Box<str>),
    /// the neighbor and its `/ip ssh forwarding-enabled`
    #[error("{0} doesn't forward connections to the device (forwarding-enabled={1})")]
    NeighborNoForwarding(Box<str>, Box<str>),
    /// the neighbor and the device's address: it took the connections, the device answered none
    #[error("{0} took the connection to {1}, but the device didn't answer the login")]
    NeighborNoAnswer(Box<str>, std::net::IpAddr),
    #[error("The mac-telnet session ended before {0}")]
    ConsoleSessionEnded(Box<str>),
    #[error("The device refused: {0}")]
    Refused(Box<str>),
    /// the partitions the device has (one is needed, with the device mode allowing more)
    #[error("Cannot split the flash: {0} partitions, or the device mode doesn't allow partitions")]
    CannotRepartition(u32),
    #[error("The device-mode change was not confirmed at the device in time")]
    DeviceModeNotConfirmed,
    /// the features still off
    #[error("The device runs without {0} in its device mode")]
    DeviceModeUnchanged(Box<str>),
    #[error("Cannot fetch {0}: {1}")]
    Download(Box<str>, reqwest::Error),
    /// what MikroTik answers at this address instead of a version
    #[error("{0} names no RouterOS version: {1}")]
    UnknownRelease(Box<str>, Box<str>),
    /// a `/system/resource` field, or that menu itself
    #[error("The device does not report {0}")]
    MissingDeviceState(&'static str),
    #[error("{0} is missing on the device after the upload")]
    UploadMissing(Box<str>),
    #[error("{file} on the device has {size} bytes, not {expected}")]
    UploadIncomplete {
        file: Box<str>,
        size: Box<str>,
        expected: usize,
    },
    /// the device runs another version or firmware than the update expected (a device without
    /// RouterBOARD has no firmware)
    #[error("Back after the reboot with RouterOS {version}, firmware {firmware:?}")]
    UnexpectedAfterReboot {
        version: Box<str>,
        firmware: Option<Box<str>>,
    },
    #[error("Not back after the reboot")]
    NotBackAfterReboot,
    /// the device switched back to the partition with the RouterOS from before the update
    #[error("The update was rolled back: the device runs RouterOS {version} from {partition}")]
    UpdateRolledBack {
        partition: Box<str>,
        version: Box<str>,
    },
    /// the OSPF neighbors adjacent before the update that aren't after it (`<instance> <router-id>`)
    #[error("OSPF neighbors missing after the update: {0}")]
    OspfNeighborsMissing(Box<str>),
    /// An update of this device would have no way back (no second partition, no neighbor to
    /// reach it through) and wasn't confirmed so (`updateRouteros(withoutFallback:)`).
    #[error("{0} has no way back from an update; confirm updating it without one")]
    NoUpdateFallback(Box<str>),
    /// no RouterOS device feeds this one over PoE (a `pse` port cabled to it in NetBox)
    #[error("No device powers {0} over PoE")]
    NoPowerSupply(Box<str>),
    /// the supplier has no Ethernet port with the factory name of NetBox's `port`
    #[error("{device} has no port {port} to switch PoE on")]
    PoePortNotFound { device: Box<str>, port: Box<str> },
    #[error("{0} runs a job already")]
    Busy(Box<str>),
    #[error("No job {0}")]
    UnknownJob(u32),
    #[error("Cancelled")]
    Cancelled,
    /// a bug: the job's work panicked with this message
    #[error("Internal error: {0}")]
    Panic(Box<str>),
    #[error("Ping: {0}")]
    Ping(#[from] surge_ping::SurgeError),
    #[error("NetBox: {0}")]
    Netbox(#[from] crate::netbox::NetboxError),
    /// the plan written isn't the one shown: NetBox changed in between
    #[error("The plan changed since it was shown")]
    PlanOutdated,
    /// what is not as the plan found it
    #[error("NetBox changed since the plan: {0}")]
    NetboxChanged(Box<str>),
    #[error("No preview: the topology isn't built from a NetBox response")]
    NoPreview,
    #[error("Cannot compute the preview: {0}")]
    Preview(Box<str>),
    /// the pool (`loopback`, `transfer`) and the IP version
    #[error("No free address in the {0} pool for IPv{1}")]
    NoFreeAddress(&'static str, u8),
    /// why the routes couldn't be read, without which no address is handed out
    #[error("Cannot check which addresses the network routes: {0}")]
    NoRouteCheck(Box<str>),
    #[error("Job {0} wrote nothing to NetBox to take back")]
    NothingToUndo(u32),
    /// both ends of a link on this device
    #[error("Both ends are on {0}")]
    SameDevice(Box<str>),
    #[error("{0} doesn't run RouterOS")]
    NoRouterOs(Box<str>),
    /// a port (`device port`) cabled elsewhere, to `far` if the cable reaches an interface
    #[error("{port} has a cable to {far:?} already")]
    PortCabled { port: Box<str>, far: Option<Box<str>> },
    /// a port (`device port`) whose cable goes to a patch panel: a direct cable would replace
    /// what NetBox documents of the building
    #[error("{0} is cabled to a patch panel")]
    PortPatched(Box<str>),
    #[error("{port} is a port of bridge {bridge}")]
    PortInBridge { port: Box<str>, bridge: Box<str> },
    /// a port with an IPv4 address not in a network with the other end
    #[error("{port} has {address} already")]
    PortAddressed { port: Box<str>, address: Box<str> },
    #[error("NetBox has no tag {0}")]
    TagMissing(Box<str>),
    /// an address a helper would delete is the primary IP of `device`
    #[error("{address} is the primary IP of {device}")]
    PrimaryAddress { address: Box<str>, device: Box<str> },
    /// a port (`device port`) without cable or transfer address
    #[error("{0} has no link to take out of NetBox")]
    NothingToDisconnect(Box<str>),
    /// an interface (`device interface`) without address, `dhcp` or VLAN
    #[error("{0} has no customer network to take out of NetBox")]
    NoCustomerNet(Box<str>),
    /// a port (`device port`) in no bridge
    #[error("{0} is in no bridge")]
    NoBridge(Box<str>),
    /// an address to take over (`192.168.1.1/24`) that NetBox has already
    #[error("NetBox has {0} already")]
    AddressInNetbox(Box<str>),
    /// an address to take over that isn't one (`address/length`)
    #[error("{0} is no address with its length")]
    InvalidAddress(Box<str>),
    /// a lease's MAC address that isn't one
    #[error("{0} is no MAC address")]
    InvalidMac(Box<str>),
    /// a lease's address in no DHCP network of the router (`router address`)
    #[error("{0} is in no DHCP network of the router")]
    NoDhcpNetwork(Box<str>),
    /// another VLAN of the device (`vlan` of `group`) has the VID
    #[error("VLAN {vlan} of {group} has VID {vid} on the device already")]
    VlanIdTaken { vid: u16, vlan: Box<str>, group: Box<str> },
    /// the VXLAN of `group` is in `bridge` on the device (`None`: the default bridge), the port
    /// would be in another one
    #[error("The VXLAN of {group} is in bridge {bridge:?} on the device")]
    VxlanInBridge { group: Box<str>, bridge: Option<Box<str>> },
    /// a port (`device port`) with `dhcp-client` or `nat`
    #[error("{0} is an uplink")]
    PortUplink(Box<str>),
    /// a port (`device port`) with this VLAN untagged and none tagged already
    #[error("{port} has VLAN {vlan} already")]
    PortHasVlan { port: Box<str>, vlan: u16 },
    #[error("{0} has no site")]
    NoSite(Box<str>),
    #[error("The device needs a name")]
    NoDeviceName,
    #[error("NetBox has a device {0} already")]
    DeviceNameTaken(Box<str>),
    /// not a WLAN group with a controller
    #[error("Unknown WLAN group {0}")]
    UnknownWlanGroup(u32),
    /// NetBox has no platform of this name
    #[error("NetBox has no platform {0}")]
    NoPlatform(Box<str>),
    /// not a prefix of the device's site
    #[error("Unknown prefix {0}")]
    UnknownPrefix(u32),
    #[error("Unknown VLAN {0}")]
    UnknownVlan(u32),
    /// a network's length not inside its parent prefix, or longer than `/30`
    #[error("No /{length} network in {parent}")]
    NetworkLength { length: u8, parent: Box<str> },
    #[error("No free /{length} in {parent}")]
    NoFreeNetwork { parent: Box<str>, length: u8 },
    /// „Kunden-Netz“ without a prefix to take a network from
    #[error("No prefix chosen")]
    NoParent,
    /// a port (`device port`) whose NetBox type is no Ethernet
    #[error("{0} is no Ethernet port")]
    NotEthernet(Box<str>),
    /// a device with no radio port, or several
    #[error("{0} has no single radio port")]
    NoSingleRadio(Box<str>),
    /// a radio bridge whose Ethernet ports reach no router
    #[error("{0} is cabled to no router")]
    NoRouterPort(Box<str>),
    /// a radio port (`device port`) with a wireless link to `far`
    #[error("{port} has a wireless link to {far:?} already")]
    WirelessLinkElsewhere { port: Box<str>, far: Option<Box<str>> },
    /// an encrypted wireless link whose PSK NetBox doesn't give (the token's rights)
    #[error("NetBox gives no PSK for the wireless link at {0}")]
    PskUnreadable(Box<str>),
    /// a network with less hosts than the ends on it
    #[error("{net} has no room for {ends} addresses")]
    NetworkTooSmall { net: Box<str>, ends: u8 },
    /// a name another interface of the device has
    #[error("{device} has an interface {name} already")]
    NameTaken { device: Box<str>, name: Box<str> },
    /// the watchdog of a device powering the one to update couldn't be suspended (`cause`, as
    /// the device or the connection answered)
    #[error("The watchdog of {device} could not be suspended: {cause}")]
    WatchdogNotSuspended { device: Box<str>, cause: Box<str> },
    /// `address`, where `device` was looked for, answers with the identity of `other`, another
    /// device in NetBox (an address taken twice, a DHCP client left on)
    #[error("{address} is {other}, not {device}")]
    WrongDevice {
        device: Box<str>,
        address: Box<str>,
        other: Box<str>,
    },
}

impl From<ResourceMutationError> for Error {
    fn from(value: ResourceMutationError) -> Self {
        Error::ResourceMutation(Box::new(value))
    }
}

impl From<MissingDependenciesError<'_, '_>> for Error {
    fn from(value: MissingDependenciesError) -> Self {
        Error::MissingDependenciesError(value.to_string().into_boxed_str())
    }
}
