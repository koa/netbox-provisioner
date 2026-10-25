//! The watchdogs every device gets (`docs/watchdog.md`): the hardware watchdog,
//! which reboots a hung device, and the ping watchdog on its parent on the way to the
//! management roots (`topology::management`), which reboots a device that runs but can't
//! reach the network anymore. A port powering a RouterOS device power cycles it when it
//! doesn't answer (`power-cycle-ping`), so a hung radio bridge comes back without its router
//! rebooting.
use super::{
    BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError,
    warning::{ApplyWarning, WatchTargetSilent},
};
use crate::topology::{
    access::{device::DeviceAccess, interface::InterfaceAccess},
    management::{ManagementTree, shared_interfaces},
};
use mikrotik_model::{model::SystemWatchdogCfg, value::HasNone};
use std::{collections::BTreeSet, net::IpAddr, time::Duration};

/// How long the watched address may stay silent before the device reboots, RouterOS' longest
/// (CHR 7.24.5: 10 s to 10 min). It reboots after one such period without an answer, not
/// after several (CHR 7.24.5: 1 min and 3 min after the last answer), so the neighbor's own
/// reboot and OSPF finding another way must fit in it. A radio that listens for radar for
/// 10 minutes doesn't, which is why the apply suspends it (`device::watchdog`).
pub const PING_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// No pinging that long after a boot: a device whose parent stays away reboots once every
/// `PING_START_AFTER_BOOT` + [`PING_TIMEOUT`] plus the boot.
pub const PING_START_AFTER_BOOT: Duration = Duration::from_secs(15 * 60);
/// How long a powered RouterOS device may stay silent before its port power cycles it; longer
/// than its reboot.
pub const POE_PING_TIMEOUT: Duration = Duration::from_secs(5 * 60);

impl BaseDeviceDataTarget {
    /// The hardware watchdog with a support output after a crash, the ping watchdog on the
    /// device's parent (`watched_address`) and the power cycling of the RouterOS devices it
    /// powers. Called again with another tree, it replaces what it set before.
    pub fn setup_watchdog(
        &mut self,
        device: &DeviceAccess,
        tree: &ManagementTree,
    ) -> Result<(), SetupError> {
        self.watchdog = SystemWatchdogCfg {
            watchdog_timer: true,
            automatic_supout: true,
            auto_send_supout: false,
            watch_address: watched_address(device, tree).map_or(HasNone::NoneValue, HasNone::Value),
            ping_start_after_boot: PING_START_AFTER_BOOT,
            ping_timeout: PING_TIMEOUT,
        };
        for port in device.interfaces().iter().filter(|port| port.enable_poe()) {
            let Some(port_id) = port.external_port() else {
                continue;
            };
            let Some(ethernet) = self.get_ethernet_port(port_id)? else {
                continue;
            };
            // a board without power cycling doesn't report it (`hw_facts`)
            if ethernet.power_cycle_ping_enabled.is_none() {
                continue;
            }
            let powered = port
                .connected_interfaces()
                .iter()
                .find_map(|far| far.device().filter(DeviceAccess::has_routeros).map(|_| far.clone()))
                .and_then(|far| cable_address(&far));
            ethernet.power_cycle_ping_enabled = Some(powered.is_some());
            ethernet.power_cycle_ping_address = powered;
            ethernet.power_cycle_ping_timeout = powered.map(|_| POE_PING_TIMEOUT);
        }
        Ok(())
    }

    /// The addresses [`Self::setup_watchdog`] watches, to ask whether they answer.
    pub fn watched_addresses(&self) -> Vec<IpAddr> {
        self.watchdog
            .watch_address
            .value()
            .copied()
            .into_iter()
            .chain(
                self.ethernet
                    .values()
                    .filter(|port| port.power_cycle_ping_enabled == Some(true))
                    .filter_map(|port| port.power_cycle_ping_address),
            )
            .collect()
    }

    /// Where a watched address isn't in `answering`, what `current` has there stays: a
    /// watchdog only starts on what answers. A silent address is a warning, with the device
    /// `name_of` gives it.
    pub fn keep_silent_watchdogs(
        &mut self,
        current: &BaseDeviceDataCurrent,
        answering: &BTreeSet<IpAddr>,
        name_of: impl Fn(IpAddr) -> Box<str>,
    ) -> Vec<ApplyWarning> {
        let mut warnings = Vec::new();
        if let Some(address) = self.watchdog.watch_address.value().copied()
            && !answering.contains(&address)
        {
            self.watchdog.watch_address = current.watchdog.watch_address.clone();
            warnings.push(ApplyWarning::WatchTargetSilent(WatchTargetSilent {
                address: address.to_string().into(),
                port: None,
                device: name_of(address),
            }));
        }
        for (default_name, port) in self.ethernet.iter_mut() {
            let Some(address) = port.power_cycle_ping_address.filter(|_| port.power_cycle_ping_enabled == Some(true))
            else {
                continue;
            };
            if answering.contains(&address) {
                continue;
            }
            let kept = current.ethernet.iter().find(|current| current.default_name == *default_name);
            port.power_cycle_ping_enabled = kept.and_then(|kept| kept.data.power_cycle_ping_enabled);
            port.power_cycle_ping_address = kept.and_then(|kept| kept.data.power_cycle_ping_address);
            port.power_cycle_ping_timeout = kept.and_then(|kept| kept.data.power_cycle_ping_timeout);
            warnings.push(ApplyWarning::WatchTargetSilent(WatchTargetSilent {
                address: address.to_string().into(),
                port: Some(port.name.to_string().into()),
                device: name_of(address),
            }));
        }
        warnings
    }
}

/// The IPv4 address of a powered device at the port cabled to it (or its bridge): the one the
/// initial setup gives it, before its loopback exists; else its primary IPv4.
fn cable_address(port: &InterfaceAccess) -> Option<IpAddr> {
    port.ips()
        .iter()
        .chain(port.bridge().map(|bridge| bridge.ips()).unwrap_or_default().iter())
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, ipnet::IpNet::V4(net) if net.prefix_len() < 32))
        .map(|net| net.addr())
        .or_else(|| port.device().and_then(|device| address(&device)))
}

/// What the ping watchdog of `device` watches: the primary IPv4 of its parent. None for a
/// root, a device no root is reached from, and one that reaches its parent only through ports
/// powering a device: rebooting would cut that device's power, maybe while it installs an
/// update (berghof-burri bricked burri-schopf that way). That is also the far router of a radio
/// bridge it powers, which shares the bridge's network with the near router. The port's power
/// cycling looks after the powered device instead.
pub fn watched_address(device: &DeviceAccess, tree: &ManagementTree) -> Option<IpAddr> {
    let parent = tree.parent(device.id())?;
    let parent = device.topology().get_device_by_id(&parent)?;
    if shared_interfaces(device, &parent)
        .iter()
        .all(|interface| through_powered_ports(device, interface))
    {
        return None;
    }
    address(&parent)
}

/// Whether every cabled port of `interface` (itself, or the ports of a bridge) powers what it
/// is cabled to.
fn through_powered_ports(device: &DeviceAccess, interface: &InterfaceAccess) -> bool {
    let ports = if interface.is_bridge() {
        device
            .interfaces()
            .iter()
            .filter(|port| port.bridge().is_some_and(|bridge| bridge.id() == interface.id()))
            .cloned()
            .collect()
    } else {
        vec![interface.clone()]
    };
    let mut cabled = ports.iter().filter(|port| !port.connected_interfaces().is_empty()).peekable();
    cabled.peek().is_some() && cabled.all(InterfaceAccess::enable_poe)
}

fn address(device: &DeviceAccess) -> Option<IpAddr> {
    device.primary_ip_v4().and_then(|ip| ip.addr())
}
