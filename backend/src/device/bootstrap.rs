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
    device::{
        Credentials, Path, connect,
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
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::MacAddress;
use std::{net::IpAddr, str::FromStr, time::Duration};
use tokio::time::{Instant, sleep};

/// Marks what the setup added besides the address, so it can be told apart on the device.
const COMMENT: &str = "netbox-provisioner setup";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
/// `/tool mac-telnet` gives up on a silent MAC after about 10 s
const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);

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
    async fn discovered(&self) -> Result<Vec<Discovered>, Error> {
        discover(self).await
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
    let mut reason = "no port with an IPv4 address in NetBox";
    let mut plans = Vec::new();
    for port in device.interfaces() {
        let (port_address, on_bridge) = match ipv4(&port) {
            Some(address) => (address, false),
            None => match port.bridge().and_then(|b| ipv4(&b)) {
                Some(address) => (address, true),
                None => continue,
            },
        };
        reason = "no cable from a port with IPv4 address to a reachable device";
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
    for row in rows(&neighbor, "/interface/ethernet/print").await? {
        if field(&row, "default-name") == Some(plan.neighbor_port.as_str())
            && let Some(name) = field(&row, "name")
        {
            names.push(name.into());
        }
    }
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
    let result = set_up(&mut terminal, &plan.setup, user, password).await;
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
    let path = Path::Neighbor(tunnel(plan).await?.into());
    log.unless_cancelled(wait_for_api(device, address, &path)).await??;
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
            let stream = tokio::net::TcpStream::connect((plan.neighbor_address, 22))
                .await
                .map_err(|e| TerminalError::Ssh(e.into()))?;
            Tunnel::connect_with_stream(stream, neighbor.user(), neighbor.password()).await?
        }
    })
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
    // factory settings first: `admin` without password; then the device's own credentials, for
    // a device that has them already but no address yet
    let mut attempts = vec![("admin", "")];
    if !password.is_empty() || user != "admin" {
        attempts.push((user, password));
    }
    let mut logged_in = None;
    'attempts: for (login_user, login_password) in attempts {
        for mac in &setup.macs {
            if let Some(how) = login(terminal, mac, login_user, login_password, password).await? {
                logged_in = Some((login_user, how, mac));
                break 'attempts;
            }
        }
    }
    let Some((login_user, how, mac)) = logged_in else {
        return Err(Error::Bootstrap(
            macs(&setup.macs).into(),
            "login refused or no answer over mac-telnet".into(),
        ));
    };
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
        // out may cut this session, so it happens in the background once it is closed.
        commands.push(format!(
            ":execute {{:delay 2s; /interface bridge port remove [find interface={port}]}}"
        ));
    }
    for command in commands {
        let output = terminal
            .run(&off_the_neighbor(&command), COMMAND_TIMEOUT)
            .await?;
        if let Some(error) = console_error(&output) {
            return Err(Error::Bootstrap(command.into(), error.into()));
        }
        if output.contains("skipped on the neighbor") {
            return Err(Error::Bootstrap(
                command.into(),
                "the mac-telnet session ended before this".into(),
            ));
        }
    }
    terminal.send_line("/quit").await?;
    terminal
        .expect(&["Welcome back!"], COMMAND_TIMEOUT)
        .await?;
    terminal.expect(&["] > "], COMMAND_TIMEOUT).await?;
    Ok(())
}

/// One `/tool mac-telnet` login; `None` if it was refused or nobody answered (the neighbor's own
/// prompt is back then).
async fn login(
    terminal: &mut Terminal,
    mac: &MacAddress,
    user: &str,
    password: &str,
    new_password: &str,
) -> Result<Option<LoggedIn>, TerminalError> {
    terminal
        .send_line(&format!("/tool mac-telnet {mac}"))
        .await?;
    terminal.expect(&["Login: "], COMMAND_TIMEOUT).await?;
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
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        let within = deadline.saturating_duration_since(Instant::now());
        // "repeat new password>" before "new password>", it ends at the same place
        const PROMPTS: [&str; 7] = [
            "Welcome back!",
            "[Y/n]",
            // the notice about the default configuration of a factory device: keep it, the
            // apply reconciles it with NetBox later
            "or hit any other key to continue",
            "repeat new password> ",
            "new password> ",
            "Login: ",
            "] > ",
        ];
        match PROMPTS[terminal.expect(&PROMPTS, within).await?] {
            "Welcome back!" => {
                terminal.expect(&["] > "], COMMAND_TIMEOUT).await?;
                return Ok(None);
            }
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
                return Ok(None);
            }
            _ => {
                // RouterOS redraws the prompt once more after the login
                terminal.settle(Duration::from_millis(800)).await?;
                return Ok(Some(how));
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

/// Escapes a value for a double quoted RouterOS console string.
fn quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// Waits until the device answers the API at its new address with its credentials.
async fn wait_for_api(device: &DeviceAccess, address: IpAddr, path: &Path) -> Result<(), Error> {
    let credentials = Credentials::Named(
        device
            .credentials()
            .ok_or(Error::MissingCredentials)?
            .into(),
    );
    let deadline = Instant::now() + API_TIMEOUT;
    loop {
        match connect::<SimpleResult>(address, path, &credentials).await {
            Ok(_) => return Ok(()),
            Err(error) if Instant::now() > deadline => return Err(error),
            Err(_) => sleep(Duration::from_secs(2)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{console_error, quote};

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
}
