//! Initial setup of a device that can't be reached over IP yet (factory settings, or not
//! configured for its place in the network): a neighbor that is already reachable opens a
//! `/tool mac-telnet` session to the device, logs in (setting the password from the device's
//! credentials when RouterOS asks for a new one) and puts the NetBox address of the cabled port
//! on it. From then on the device is reached over that address and configured by the normal
//! apply.
//!
//! NetBox doesn't know MAC addresses: which physical device gets the NetBox device's config is
//! chosen by the operator, from what the neighbor discovers on its port ([`Discovered`], MNDP;
//! a factory bridge announces, and answers mac-telnet on, the bridge's own MAC) or typed in.
//!
//! The API can't do this: `/tool/mac-telnet` there takes a host but gives no interactive
//! session, so it runs on the neighbor's console over SSH ([`Terminal`]).
use crate::{
    Error,
    config::{CONFIG, MikrotikCredentials},
    error_info::{ApiError, BootstrapObstacle},
    device::{
        Credentials, Path, connect, other_device_named,
        proxied_stream,
        raw::{field, rows},
        terminal::{LOGIN_FLAGS, Terminal, TerminalError, Tunnel},
    },
    graphql::scalars::{ScalarDuration, ScalarMac},
    job::Log,
    topology::access::{device::DeviceAccess, interface::InterfaceAccess},
};
use async_graphql::{ComplexObject, SimpleObject};
use ipnet::IpNet;
use log::{info, warn};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::MacAddress;
use std::{collections::HashSet, net::IpAddr, str::FromStr, time::Duration};
use tokio::time::{Instant, sleep, timeout};

/// Marks what the setup added besides the address, so it can be told apart on the device.
const COMMENT: &str = "netbox-provisioner setup";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
/// For the whole login and setup over mac-telnet, sessions that end early tried again.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);
/// For the next prompt of a login: RouterOS answers within seconds, or the client hangs.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);
/// For a command in a mac-telnet session. A session that hangs ends by itself (`/tool
/// mac-telnet` prints [`SESSION_ENDED`] after about 20 s on CHR), so this only notices one
/// that never ends.
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);

/// How [`bootstrap`] would reach a device.
#[derive(Clone, SimpleObject)]
#[graphql(complex)]
pub struct BootstrapPlan {
    /// the device that runs `/tool mac-telnet`
    pub neighbor: String,
    /// its port the cable starts at
    pub neighbor_port: String,
    #[graphql(skip)]
    pub setup: MacTelnetSetup,
    #[graphql(skip)]
    neighbor_device: DeviceAccess,
    /// where the neighbor is reached (over SSH)
    #[graphql(skip)]
    neighbor_address: IpAddr,
}

#[ComplexObject]
impl BootstrapPlan {
    /// the port of the device being set up, by its factory name
    async fn port(&self) -> &str {
        &self.setup.port
    }
    /// what that port gets, from NetBox
    async fn address(&self) -> String {
        self.setup.address.to_string()
    }
    /// What the neighbor discovers on its port (`/ip/neighbor`), the candidates to set up. Over
    /// a bridge (a radio link) that includes devices further away.
    async fn discovered(&self) -> Result<Vec<Discovered>, ApiError> {
        Ok(discover(self).await?)
    }
}

/// A device the neighbor sees on its port.
#[derive(Clone, Debug, SimpleObject)]
pub struct Discovered {
    pub mac: ScalarMac,
    pub identity: String,
    pub board: String,
    pub version: String,
    /// the port as the neighbor names it (`ether2,bridge` behind a bridge)
    pub interface: String,
}

/// What [`set_up`] does over a neighbor's console.
#[derive(Clone, Debug)]
pub struct MacTelnetSetup {
    /// tried in this order, at least one; empty in a [`plan`] until the operator picks the device
    pub macs: Box<[MacAddress]>,
    /// factory name of the port the MAC belongs to
    pub port: String,
    /// address the port gets
    pub address: IpNet,
    /// NetBox has the address on the port's bridge: it goes on the bridge the port is in on the
    /// device (or, if the port is in none, the bridge that answered mac-telnet, and the port
    /// back into it) instead of on the port taken out of its bridge. A bridge carrying a radio
    /// link stays whole that way.
    pub on_bridge: bool,
    /// the neighbor's address on the same cable: the provisioner's connections to the device
    /// come from there (through an SSH tunnel to the neighbor, see [`tunnel`]), so a factory
    /// firewall lets it through and the device needs no route
    pub neighbor_address: IpAddr,
    /// the neighbor's port to the device as the neighbor names it, set by [`bootstrap`]:
    /// `/tool mac-telnet` is tried with it as `interface` and without (see [`log_in`])
    pub neighbor_port: Option<String>,
}

/// The setup logs in on `neighbor`'s console and opens MAC-telnet on `neighbor_port` to the
/// device with one of `macs`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SetupStarted {
    pub neighbor: Box<str>,
    pub neighbor_port: Box<str>,
    pub macs: Box<[Box<str>]>,
}

/// The console session is over; the setup waits up to `timeout` for the device's API at
/// `address`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WaitingForApi {
    pub address: Box<str>,
    pub timeout: ScalarDuration,
}

/// The device answers the API at `address`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SetupDone {
    pub address: Box<str>,
}

/// How long the device may take to answer the API after the setup.
const API_TIMEOUT: Duration = Duration::from_secs(30);

impl BootstrapPlan {
    pub fn port_address(&self) -> IpAddr {
        self.setup.address.addr()
    }
}

/// Finds a cabled port of `device` with an IPv4 address in NetBox (on the port, or on the bridge
/// it is a port of) whose far end is on a device that can be reached already (the management
/// proxy or a device with a primary IP) and has an address in the same subnet. The address goes
/// on the port itself for the setup; the apply moves it where NetBox has it.
pub fn plan(device: &DeviceAccess) -> Result<BootstrapPlan, Error> {
    Ok(plans(device)?.swap_remove(0))
}

/// Every [`plan`] of `device`, the one from the management proxy's device first: that one is
/// reachable whatever the network does, while another neighbor may be cut off itself (an MTU
/// change breaks OSPF until both ends of a link have it).
pub fn plans(device: &DeviceAccess) -> Result<Vec<BootstrapPlan>, Error> {
    let no_path = |reason| Error::NoBootstrapPath(device.name().into(), reason);
    let mut reason = BootstrapObstacle::NoIpv4Port;
    let mut plans = Vec::new();
    for port in device.interfaces() {
        let (port_address, on_bridge) = match ipv4(&port) {
            Some(address) => (address, false),
            None => match port.bridge().and_then(|b| ipv4(&b)) {
                Some(address) => (address, true),
                None => continue,
            },
        };
        reason = BootstrapObstacle::NoReachableNeighbor;
        for far in port.connected_interfaces() {
            let Some(neighbor) = far.device() else { continue };
            let Some(neighbor_address) = reachable_address(&neighbor) else {
                continue;
            };
            let Some(neighbor_port_address) = ipv4(&far)
                .filter(|a| a.network() == port_address.network())
                .map(|a| a.addr())
            else {
                continue;
            };
            let Some(factory_name) = port.external_port() else {
                continue;
            };
            plans.push(BootstrapPlan {
                neighbor: neighbor.name().into(),
                neighbor_port: far.name().into(),
                setup: MacTelnetSetup {
                    macs: Box::new([]),
                    port: factory_name.to_string(),
                    address: port_address,
                    on_bridge,
                    neighbor_address: neighbor_port_address,
                    neighbor_port: None,
                },
                neighbor_device: neighbor,
                neighbor_address,
            });
        }
    }
    if plans.is_empty() {
        return Err(no_path(reason));
    }
    let proxy = CONFIG.management_proxy.as_ref().map(|proxy| proxy.device.as_ref());
    plans.sort_by_key(|plan| proxy != Some(plan.neighbor.as_str()));
    Ok(plans)
}

/// The first IPv4 address NetBox has on `port`.
fn ipv4(port: &InterfaceAccess) -> Option<IpNet> {
    port.ips()
        .iter()
        .filter_map(|ip| ip.net())
        .find(|net| net.addr().is_ipv4())
}

/// Where `device` is reached: the management proxy's device directly at the proxy's address
/// (its primary IP may only be reachable through its own proxy), any other at its primary IP.
pub fn reachable_address(device: &DeviceAccess) -> Option<IpAddr> {
    match &CONFIG.management_proxy {
        Some(proxy) if proxy.device.as_ref() == device.name() => Some(proxy.address.ip()),
        _ => device.primary_ip(),
    }
}

async fn discover(plan: &BootstrapPlan) -> Result<Vec<Discovered>, Error> {
    let credentials = Credentials::Named(
        plan.neighbor_device
            .credentials()
            .ok_or(Error::MissingCredentials)?
            .into(),
    );
    let neighbor =
        connect::<SimpleResult>(plan.neighbor_address, &Path::Routed, &credentials).await?;
    // NetBox has the factory name, `/ip/neighbor` the one the provisioner gave the port
    let mut names = vec![plan.neighbor_port.clone()];
    names.extend(port_name(&neighbor, &plan.neighbor_port).await?);
    let mut found = Vec::new();
    for row in rows(&neighbor, "/ip/neighbor/print").await? {
        let interface = field(&row, "interface").unwrap_or_default();
        let Some(mac) = field(&row, "mac-address").and_then(|mac| MacAddress::from_str(mac).ok())
        else {
            continue;
        };
        if !interface.split(',').any(|i| names.iter().any(|n| n == i)) {
            continue;
        }
        let text = |name| field(&row, name).unwrap_or_default().to_string();
        found.push(Discovered {
            mac: mac.into(),
            identity: text("identity"),
            board: text("board"),
            version: text("version"),
            interface: interface.into(),
        });
    }
    Ok(found)
}

/// Checks that the neighbor forwards the connections to the device (see [`tunnel`]) and
/// returns what it calls its port to the device, NetBox' name when that can't be read. Before
/// the console session, so a neighbor without forwarding leaves the device untouched.
async fn check_neighbor(plan: &BootstrapPlan) -> Result<String, Error> {
    let credentials = Credentials::Named(
        plan.neighbor_device
            .credentials()
            .ok_or(Error::MissingCredentials)?
            .into(),
    );
    let neighbor =
        match connect::<SimpleResult>(plan.neighbor_address, &Path::Routed, &credentials).await {
            Ok(neighbor) => neighbor,
            Err(error) => {
                warn!("cannot check {} over the API: {error}", plan.neighbor);
                return Ok(plan.neighbor_port.clone());
            }
        };
    // RouterOS never answers a forward it doesn't allow, the tunnel would only time out
    if let Some(row) = rows(&neighbor, "/ip/ssh/print").await?.first()
        && let Some(forwarding) = field(row, "forwarding-enabled")
        && !matches!(forwarding, "local" | "both")
    {
        return Err(Error::NeighborNoForwarding(
            plan.neighbor.as_str().into(),
            forwarding.into(),
        ));
    }
    Ok(match port_name(&neighbor, &plan.neighbor_port).await {
        Ok(Some(name)) => name,
        Ok(None) => plan.neighbor_port.clone(),
        Err(error) => {
            warn!("cannot read the port names of {}: {error}", plan.neighbor);
            plan.neighbor_port.clone()
        }
    })
}

/// The name the neighbor gives its Ethernet port with the factory name `factory_name`, if it
/// has one.
async fn port_name(
    neighbor: &RawDevice<SimpleResult>,
    factory_name: &str,
) -> Result<Option<String>, Error> {
    Ok(rows(neighbor, "/interface/ethernet/print")
        .await?
        .iter()
        .find(|row| field(row, "default-name") == Some(factory_name))
        .and_then(|row| field(row, "name"))
        .map(String::from))
}

/// The MACs to try, for messages.
fn macs(macs: &[MacAddress]) -> String {
    macs.iter()
        .map(MacAddress::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn credentials_of(device: &DeviceAccess) -> Result<&'static MikrotikCredentials, Error> {
    device
        .credentials()
        .and_then(|name| CONFIG.mikrotik_credentials.get(name))
        .ok_or(Error::MissingCredentials)
}

/// Sets `device` up along `plan`, the console session going to `log` as it comes (passwords
/// replaced by `***`). `plan.setup.macs` is the device chosen by the operator. Cancelled, it
/// stops before the console session or while it waits for the API, not within the session.
/// Writes without asking for `allow_apply`, that is up to the caller (`WritesAllowed`).
pub async fn bootstrap(device: &DeviceAccess, plan: &BootstrapPlan, log: &Log) -> Result<(), Error> {
    let target = credentials_of(device)?;
    let neighbor = credentials_of(&plan.neighbor_device)?;
    let user = target.user();
    let password = target.password().unwrap_or_default();
    info!(
        "setting up {} from {} {} over mac-telnet to {}",
        device.name(),
        plan.neighbor,
        plan.neighbor_port,
        macs(&plan.setup.macs)
    );
    log.check()?;
    let mut setup = plan.setup.clone();
    setup.neighbor_port = Some(check_neighbor(plan).await?);
    log.emit(SetupStarted {
        neighbor: plan.neighbor.as_str().into(),
        neighbor_port: plan.neighbor_port.as_str().into(),
        macs: plan.setup.macs.iter().map(|mac| mac.to_string().into()).collect(),
    });
    let mut terminal = match proxied_stream(plan.neighbor_address, 22).await? {
        Some(stream) => {
            Terminal::connect_with_stream(stream, neighbor.user(), neighbor.password()).await?
        }
        None => {
            Terminal::connect(
                (plan.neighbor_address, 22).into(),
                neighbor.user(),
                neighbor.password(),
            )
            .await?
        }
    };
    terminal.hide(password);
    terminal.stream_to(log);
    let result = set_up(&mut terminal, &setup, user, password).await;
    terminal.close().await;
    if let Err(error) = &result {
        warn!("setup of {} failed: {error}", device.name());
    }
    result?;
    let address = plan.port_address();
    log.emit(WaitingForApi {
        address: address.to_string().into(),
        timeout: API_TIMEOUT.into(),
    });
    log.unless_cancelled(wait_for_api(device, address, plan)).await??;
    log.emit(SetupDone {
        address: address.to_string().into(),
    });
    Ok(())
}

/// An SSH tunnel to `plan`'s neighbor, which makes the connections to the device it sets up:
/// from its own address on their cable, so the device needs no route back to the provisioner
/// before its first apply brings OSPF. Needs `/ip ssh forwarding-enabled=local` on the neighbor,
/// which every generated config has.
pub async fn tunnel(plan: &BootstrapPlan) -> Result<Tunnel, Error> {
    let neighbor = credentials_of(&plan.neighbor_device)?;
    Ok(match proxied_stream(plan.neighbor_address, 22).await? {
        Some(stream) => {
            Tunnel::connect_with_stream(stream, neighbor.user(), neighbor.password()).await?
        }
        None => {
            // without a limit, Linux gives up on an unanswered connect after about 2 min: an
            // apply's checks of a device behind a radio link that was down came that rarely
            let stream = timeout(
                super::OPEN_TIMEOUT,
                tokio::net::TcpStream::connect((plan.neighbor_address, 22)),
            )
            .await
            .map_err(|_| Error::Timeout(plan.neighbor_address))?
            .map_err(|e| TerminalError::Ssh(e.into()))?;
            Tunnel::connect_with_stream(stream, neighbor.user(), neighbor.password()).await?
        }
    })
}

/// How a [`login`] ended.
enum Login {
    Done(LoggedIn),
    /// RouterOS said the credentials are wrong (also when the device answered too late)
    Refused,
    /// the session ended without an answer to the login
    Undecided,
}

/// What the device is logged in with after [`login`].
enum LoggedIn {
    /// the password from the credentials (set at RouterOS' request, or already in place)
    WithPassword,
    /// with an empty password and nobody asked for a new one (older RouterOS)
    WithoutPassword,
}

/// Logs in to the device at one of `setup.macs` from the neighbor's console `terminal` (factory
/// settings, or else `user` with `password`), makes sure `user` has `password` and gives the
/// port its address. Back at the neighbor's prompt afterwards.
pub async fn set_up(
    terminal: &mut Terminal,
    setup: &MacTelnetSetup,
    user: &str,
    password: &str,
) -> Result<(), Error> {
    // When the mac-telnet session ends early (the device reboots, a bridge change cuts it), the
    // next lines land on the neighbor's own console. Each command is guarded by a global set
    // only on the neighbor (globals are per device), so there it does nothing.
    terminal
        .run(&format!(":global {NEIGHBOR_MARK} true"), COMMAND_TIMEOUT)
        .await?;
    let result = set_up_marked(terminal, setup, user, password).await;
    if let Err(error) = terminal
        .run(
            &format!("/system script environment remove [find name=\"{NEIGHBOR_MARK}\"]"),
            COMMAND_TIMEOUT,
        )
        .await
    {
        warn!("{NEIGHBOR_MARK} stays on the neighbor: {error}");
    }
    result
}

/// Set on the neighbor's console while [`set_up`] runs.
const NEIGHBOR_MARK: &str = "netboxProvisionerNeighbor";

/// `command`, run only where [`NEIGHBOR_MARK`] isn't set.
fn off_the_neighbor(command: &str) -> String {
    format!(
        "{{:global {NEIGHBOR_MARK}; :if ([:typeof ${NEIGHBOR_MARK}] = \"nothing\") do={{{command}}} else={{:put (\"skipped on the \" . \"neighbor\")}}}}"
    )
}

async fn set_up_marked(
    terminal: &mut Terminal,
    setup: &MacTelnetSetup,
    user: &str,
    password: &str,
) -> Result<(), Error> {
    // A session can end at any point, when the device answers late (busy, just booted:
    // `/tool mac-telnet` gives up then). The commands are safe to run again, so after a login
    // that is up they run from the start, until all of them are through in one session.
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        let (login_user, how, mac) = log_in(terminal, setup, user, password, deadline).await?;
        let commands = setup_commands(setup, user, password, login_user, how, mac);
        match run_in_session(terminal, &commands, password).await? {
            None => return Ok(()),
            Some(ended_before) if Instant::now() > deadline => {
                return Err(Error::ConsoleSessionEnded(ended_before));
            }
            Some(ended_before) => {
                info!("mac-telnet session to {mac} ended before {ended_before}, again");
            }
        }
    }
}

/// Logs in to one of `setup.macs`: with the factory settings (`admin` without password), or
/// else as `user` with `password`, for a device that has them already but no address yet.
/// Only a session that is up decides: RouterOS also says "incorrect username or password"
/// when the device answered too late, so every pair of credentials and MAC is tried again
/// until `deadline`, and counts as refused only if its last try was.
async fn log_in<'a>(
    terminal: &mut Terminal,
    setup: &'a MacTelnetSetup,
    user: &'a str,
    password: &str,
    deadline: Instant,
) -> Result<(&'a str, LoggedIn, &'a MacAddress), Error> {
    let mut attempts = vec![("admin", "")];
    if !password.is_empty() || user != "admin" {
        attempts.push((user, password));
    }
    // Some RouterOS versions only answer with the neighbor's port as `interface` (seen on a
    // CRS), others only without (CHR 7.24.5); the one that got a login prompt stays
    let mut interfaces = vec![None];
    if let Some(port) = &setup.neighbor_port {
        interfaces.insert(0, Some(port.as_str()));
    }
    let mut answered: Vec<Option<Option<&str>>> = vec![None; setup.macs.len()];
    let mut refused = HashSet::new();
    loop {
        for (attempt, &(login_user, login_password)) in attempts.iter().enumerate() {
            for (index, mac) in setup.macs.iter().enumerate() {
                let tried = match answered[index] {
                    Some(interface) => vec![interface],
                    None => interfaces.clone(),
                };
                for interface in tried {
                    match login(terminal, mac, interface, login_user, login_password, password)
                        .await?
                    {
                        Login::Done(how) => return Ok((login_user, how, mac)),
                        Login::Refused => {
                            refused.insert((attempt, index));
                            answered[index] = Some(interface);
                            break;
                        }
                        Login::Undecided => {
                            refused.remove(&(attempt, index));
                        }
                    }
                }
            }
        }
        if Instant::now() > deadline {
            return Err(if refused.len() == attempts.len() * setup.macs.len() {
                Error::ConsoleLoginRefused(macs(&setup.macs).into())
            } else {
                Error::ConsoleNoAnswer(macs(&setup.macs).into())
            });
        }
    }
}

/// What the device needs, logged in as `login_user` at `mac`: `user` with `password`, the
/// address on the port and the API open to the neighbor. Each command does nothing where it
/// is done already.
fn setup_commands(
    setup: &MacTelnetSetup,
    user: &str,
    password: &str,
    login_user: &str,
    how: LoggedIn,
    mac: &MacAddress,
) -> Vec<String> {
    let port = &setup.port;
    let mut commands = Vec::new();
    if let LoggedIn::WithoutPassword = how {
        commands.push(format!(
            "/user set [find name=\"{login_user}\"] password=\"{}\"",
            quote(password)
        ));
    }
    if login_user != user {
        commands.push(format!(
            ":if ([:len [/user find name=\"{user}\"]] = 0) do={{/user add name=\"{user}\" group=full password=\"{}\"}}",
            quote(password)
        ));
    }
    let address = setup.address;
    // A DHCP client holding the address (a factory or old config; link-5-station) keeps
    // RouterOS from adding it statically ("already have such address"), and the setup's check
    // counts the lease as there: the apply then either fails on the address or takes the
    // lease, and the connection, away with the client. Off over mac-telnet, this needs no IP.
    commands.push(format!(
        "{{:local n 0; :foreach c in=[/ip dhcp-client find disabled=no] do={{\
         :if ([/ip dhcp-client get $c address] = \"{address}\") do={{\
         :put (\"dhcp-client on \" . [/ip dhcp-client get $c interface] . \" disabled: it held {address}\"); \
         /ip dhcp-client disable $c}}}}; \
         :while (([:len [/ip address find address=\"{address}\" dynamic=yes]] > 0) && ($n < 10)) do={{:delay 1s; :set n ($n + 1)}}}}"
    ));
    if setup.on_bridge {
        commands.push(format!(
            "{{:local i \"{port}\"; :local p [/interface bridge port find interface=\"{port}\"]; \
             :if ([:len $p] > 0) do={{:set i [/interface bridge port get ($p->0) bridge]}} else={{\
             :local b [/interface bridge find mac-address={mac}]; :if ([:len $b] > 0) do={{\
             :set i [/interface bridge get ($b->0) name]; /interface bridge port add bridge=$i interface=\"{port}\"}}}}; \
             /ip address remove [find address=\"{address}\" and interface!=$i]; \
             :if ([:len [/ip address find address=\"{address}\" interface=$i]] = 0) do={{/ip address add address=\"{address}\" interface=$i}}}}"
        ));
    } else {
        // stays inactive while the port is in a (factory) bridge, see below
        commands.push(format!(
            ":if ([:len [/ip address find address=\"{address}\" interface={port}]] = 0) do={{/ip address add address=\"{address}\" interface={port}}}"
        ));
    }
    // a factory firewall drops everything not from the LAN ports
    let source = setup.neighbor_address;
    commands.push(format!(
        ":if (([:len [/ip firewall filter find]] > 0) && ([:len [/ip firewall filter find src-address={source} comment=\"{COMMENT}\"]] = 0)) do={{/ip firewall filter add chain=input src-address={source} action=accept comment=\"{COMMENT}\" place-before=[:pick [/ip firewall filter find] 0]}}"
    ));
    commands.push("/ip service set api disabled=no".into());
    // A device reset to CAPs mode manages its radios by CAPsMAN, which refuses the first apply
    // to rename them; the apply turns the CAP on again where NetBox wants one. `:parse`, since
    // a menu is missing without its package.
    commands.push(
        ":foreach m in=(\"/interface wireless cap set enabled=no\",\"/interface wifi cap set enabled=no\") do={:do {:local f [:parse $m]; $f} on-error={}}"
            .into(),
    );
    if !setup.on_bridge {
        // A factory bridge takes the port's traffic before the address sees it. Taking the port
        // out may cut this session, so it happens in the background once no mac-telnet session
        // is left (this one ends with `/quit` below, or else when its client gives up).
        commands.push(format!(
            ":execute {{:while ([:len [/user active find via=mac-telnet]] > 0) do={{:delay 1s}}; \
             /interface bridge port remove [find interface={port}]}}"
        ));
    }
    commands.push("/quit".into());
    commands
}

/// What `/tool mac-telnet` prints when its session is over, back on the neighbor.
const SESSION_ENDED: &str = "Welcome back!";

/// Runs `commands` in the mac-telnet session, each once the one before is through, the last
/// (`/quit`) ending it. `Some(command)` if the session ended before `command` was through;
/// what was typed after its end landed on the neighbor and did nothing there.
async fn run_in_session(
    terminal: &mut Terminal,
    commands: &[String],
    password: &str,
) -> Result<Option<Box<str>>, Error> {
    let Some((quit, commands)) = commands.split_last() else {
        return Ok(None);
    };
    for command in commands {
        terminal.send_line(&off_the_neighbor(command)).await?;
        let output = match terminal.sync_unless(&[SESSION_ENDED], SESSION_TIMEOUT).await? {
            Ok(output) => output,
            Err(_) => {
                // the rest went to the neighbor, wait until it is through there
                terminal.sync(COMMAND_TIMEOUT).await?;
                return Ok(Some(hidden(command, password)));
            }
        };
        if let Some(error) = console_error(&output) {
            return Err(Error::SetupCommand(hidden(command, password), error.into()));
        }
    }
    terminal.send_line(&off_the_neighbor(quit)).await?;
    terminal.expect(&[SESSION_ENDED], SESSION_TIMEOUT).await?;
    // back on the neighbor
    terminal.sync(COMMAND_TIMEOUT).await?;
    Ok(None)
}

/// One `/tool mac-telnet` login, logged in at the device's prompt when [`Login::Done`], else
/// back at the neighbor's.
async fn login(
    terminal: &mut Terminal,
    mac: &MacAddress,
    interface: Option<&str>,
    user: &str,
    password: &str,
    new_password: &str,
) -> Result<Login, TerminalError> {
    let command = match interface {
        Some(interface) => format!("/tool mac-telnet {mac} interface=\"{}\"", quote(interface)),
        None => format!("/tool mac-telnet {mac}"),
    };
    terminal.send_line(&command).await?;
    match terminal.expect(&["Login: "], COMMAND_TIMEOUT).await {
        Ok(_) => {}
        Err(TerminalError::Timeout { .. }) => {
            // no answer on this way to the device, the client gives up on Ctrl-C
            terminal.send("\x03").await?;
            terminal.sync(COMMAND_TIMEOUT).await?;
            return Ok(Login::Undecided);
        }
        Err(error) => return Err(error),
    }
    terminal.send_line(&format!("{user}{LOGIN_FLAGS}")).await?;
    terminal.expect(&["Password: "], COMMAND_TIMEOUT).await?;
    terminal.send_line(password).await?;
    let mut how = if password.is_empty() {
        LoggedIn::WithoutPassword
    } else {
        LoggedIn::WithPassword
    };
    // RouterOS may print a password prompt twice; each is answered once
    let (mut new_answered, mut repeat_answered) = (false, false);
    let mut refused = false;
    loop {
        // "repeat new password>" before "new password>", it ends at the same place
        const PROMPTS: [&str; 8] = [
            SESSION_ENDED,
            // "Login failed, incorrect username or password", then the session ends
            "incorrect username or password",
            "[Y/n]",
            // the notice about the default configuration of a factory device: keep it, the
            // apply reconciles it with NetBox later
            "or hit any other key to continue",
            "repeat new password> ",
            "new password> ",
            "Login: ",
            "] > ",
        ];
        let prompt = match terminal.expect(&PROMPTS, ATTEMPT_TIMEOUT).await {
            Ok(prompt) => PROMPTS[prompt],
            Err(TerminalError::Timeout { .. }) => {
                // The client can wait for good on a device that answered too late; it leaves
                // the session on Ctrl-C.
                terminal.send("\x03").await?;
                terminal.expect(&[SESSION_ENDED], COMMAND_TIMEOUT).await?;
                terminal.sync(COMMAND_TIMEOUT).await?;
                return Ok(Login::Undecided);
            }
            Err(error) => return Err(error),
        };
        match prompt {
            SESSION_ENDED => {
                terminal.expect(&["] > "], COMMAND_TIMEOUT).await?;
                return Ok(if refused { Login::Refused } else { Login::Undecided });
            }
            "incorrect username or password" => refused = true,
            "[Y/n]" => terminal.send("n").await?,
            // a space, in case the key isn't taken: leading blanks don't matter on the prompt
            "or hit any other key to continue" => terminal.send(" ").await?,
            "repeat new password> " if !repeat_answered => {
                terminal.send_line(new_password).await?;
                repeat_answered = true;
                how = LoggedIn::WithPassword;
            }
            "new password> " if !new_answered => {
                terminal.send_line(new_password).await?;
                new_answered = true;
            }
            "repeat new password> " | "new password> " => {}
            "Login: " => {
                // refused: leave the session, back to the neighbor's prompt
                terminal.send("\x03").await?;
                terminal.expect(&["] > "], COMMAND_TIMEOUT).await?;
                return Ok(Login::Refused);
            }
            _ => {
                // through the prompts RouterOS redraws after the login
                return Ok(match terminal.sync_unless(&[SESSION_ENDED], SESSION_TIMEOUT).await? {
                    Ok(_) => Login::Done(how),
                    Err(_) => {
                        terminal.sync(COMMAND_TIMEOUT).await?;
                        Login::Undecided
                    }
                });
            }
        }
    }
}

/// The error a console command printed, if any.
fn console_error(output: &str) -> Option<&str> {
    output.lines().map(str::trim).find(|line| {
        line.starts_with("failure:")
            || line.starts_with("syntax error")
            || line.starts_with("expected ")
            || line.starts_with("bad command name")
            || line.starts_with("input does not match")
            || line.contains("no such item")
    })
}

/// `command` for a message, which the log and the UI show: `password` replaced by `***`.
fn hidden(command: &str, password: &str) -> Box<str> {
    match password {
        "" => command.into(),
        password => command.replace(&quote(password), "***").into(),
    }
}

/// Escapes a value for a double quoted RouterOS console string.
fn quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// Waits until the device answers the API at its new address with its credentials, through an
/// SSH tunnel to `plan`'s neighbor. Each try has its own time limit: neither the tunnel nor a
/// connection through it nor the login gives up by itself (the setup of link-5-ap hung here),
/// and a tunnel whose try ran out is made again.
async fn wait_for_api(
    device: &DeviceAccess,
    address: IpAddr,
    plan: &BootstrapPlan,
) -> Result<(), Error> {
    let credentials = Credentials::Named(
        device
            .credentials()
            .ok_or(Error::MissingCredentials)?
            .into(),
    );
    let deadline = Instant::now() + API_TIMEOUT;
    let path = Path::Neighbor(plan.clone().into());
    loop {
        let attempt = async {
            let client = connect::<SimpleResult>(address, &path, &credentials).await?;
            let rows = rows(&client, "/system/identity/print").await?;
            Ok(rows.first().and_then(|row| field(row, "name")).map(Box::<str>::from))
        };
        let limit = deadline
            .saturating_duration_since(Instant::now())
            .clamp(MIN_API_ATTEMPT, API_ATTEMPT);
        let error = match timeout(limit, attempt).await {
            // the address answered from another device in NetBox: waiting won't change that
            Ok(Ok(Some(identity))) if let Some(other) = other_device_named(device, &identity) => {
                return Err(Error::WrongDevice {
                    device: device.name().into(),
                    address: address.to_string().into(),
                    other,
                });
            }
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => error,
            // each try opens its own SSH session to the neighbor
            Err(_) => Error::Timeout(address),
        };
        if Instant::now() > deadline {
            return Err(error);
        }
        sleep(Duration::from_secs(2)).await;
    }
}

/// The longest one try of [`wait_for_api`] may take, and the least the last one gets.
const API_ATTEMPT: Duration = Duration::from_secs(10);
const MIN_API_ATTEMPT: Duration = Duration::from_secs(3);

#[cfg(test)]
mod tests {
    use super::{console_error, hidden, quote};

    #[test]
    fn recognizes_console_errors() {
        assert_eq!(
            console_error("failure: already have such address\n"),
            Some("failure: already have such address")
        );
        assert_eq!(console_error("syntax error (line 1 column 5)"), Some("syntax error (line 1 column 5)"));
        assert_eq!(console_error(""), None);
        assert_eq!(console_error("  name: lab-core\n"), None);
    }

    #[test]
    fn quotes_console_strings() {
        assert_eq!(quote(r#"a"b\c$d"#), r#"a\"b\\c\$d"#);
    }

    /// a failing setup command goes into the error, which the log and the UI show
    #[test]
    fn hides_the_password_in_commands() {
        let command = format!("/user set [find name=\"admin\"] password=\"{}\"", quote("p\"w$"));
        assert_eq!(
            hidden(&command, "p\"w$").as_ref(),
            "/user set [find name=\"admin\"] password=\"***\""
        );
        let command = "/ip service set api disabled=no";
        assert_eq!(hidden(command, "").as_ref(), command);
    }
}
