//! Scope: What `/metrics` reads of a device beyond the network map's reading: its WLAN clients and
//! radios, interface counters, Ethernet links and DHCP leases; not their rendering as metrics
//! (`crate::metrics`).
//!
//! What the mikrotik-metrik-exporter read (C14), as a fact per device
//! (`FactKind::DeviceMetrics`), over the same way as everything else (`DeviceAccess::connect`).
//! Health, resources and PoE come from the network map's reading
//! (`network_state::device_data`), which the map's viewers share.
//!
//! - **WLAN clients:** the registration tables of `/interface/wifi` and of the legacy CAPsMAN
//!   (`/caps-man`), with the CAP each client is on: a client of a virtual interface is on the
//!   radio of its master interface, the radio names the CAP (`identity@address`, and
//!   `remote-cap-identity` in the legacy CAPsMAN); and of `/interface/wireless` for its own
//!   radios outside a CAPsMAN (a radio link, a standalone AP), with their signal to noise.
//! - **Radios** of `/interface/wireless`: SSID and band, noise floor and frequency (`monitor`,
//!   its `channel` `5500/20-Ceee/ac/…`), for each running one; and at a controller of the
//!   legacy CAPsMAN its CAPs' radios (`/caps-man/interface`).
//! - **Interfaces:** `/interface`, which carries the counters without `stats`.
//! - **Ethernet links:** `/interface/ethernet/monitor` of every `ether` interface, link and rate.
//!
//! Every menu is read with its mikrotik-model type; only the monitors go through `device::raw`.
//! - **Leases:** the bound ones of `/ip/dhcp-server/lease`, to name a WLAN client by its DHCP
//!   hostname wherever its lease is (on the router, not the AP).
//!
//! A menu the device doesn't have (no `wifi` package, no legacy CAPsMAN) has no rows
//! (`is_missing_menu`).

use super::{AccessibleDevice, raw};
use crate::{
    Error,
    facts::{self, Fact, FactKey, FactKind, Freshness},
};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::value::RosValue;
use mikrotik_model::{
    MacAddress, MikrotikDevice,
    ascii::AsciiString,
    mikrotik_api::TrapCategory,
    model::{
        CapsManActualInterfaceConfiguration, CapsManInterface, CapsManRadioState,
        CapsManRegistrationTableState, InterfaceState, InterfaceWifi, InterfaceWifiRadioState,
        InterfaceWifiRegistrationTableState, InterfaceWireless,
        InterfaceWirelessRegistrationTableState, IpDhcpServerLease,
    },
    resource::{self, DeserializeRosResource, RosResource, collect_resource},
};
use std::{collections::HashMap, net::IpAddr, time::Duration};

#[derive(Debug, Clone, Default)]
pub struct DeviceMetrics {
    pub wlan_clients: Box<[WlanClient]>,
    pub radios: Box<[WirelessRadio]>,
    pub interfaces: Box<[InterfaceCounters]>,
    pub ethernet: Box<[EthernetLink]>,
    pub leases: Box<[Lease]>,
}

/// The CAPsMAN a WLAN client is registered at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capsman {
    /// `/interface/wifi`
    Wifi,
    /// `/caps-man`, of the `wireless` package
    Legacy,
    /// none: a radio of the device itself in `/interface/wireless` (a radio link, a standalone
    /// AP)
    Standalone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WlanClient {
    pub mac_address: MacAddress,
    /// the interface it is registered at (a CAP's in a CAPsMAN)
    pub interface: Box<str>,
    pub ssid: Box<str>,
    /// the identity of the CAP the client is on; empty for a radio of the device itself
    pub ap: Box<str>,
    pub capsman: Capsman,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    /// dBm
    pub signal: i16,
    /// dB, only `/interface/wireless` reports it
    pub signal_to_noise: Option<i16>,
    pub uptime: Duration,
}

/// A running radio of `/interface/wireless`, or of a CAP of the legacy CAPsMAN (`/caps-man`)
/// at its controller.
#[derive(Debug, Clone, PartialEq)]
pub struct WirelessRadio {
    pub name: Box<str>,
    /// empty on a CAP for a radio its CAPsMAN manages: the controller has it
    pub ssid: Box<str>,
    /// the identity of the CAP it is on; empty for a radio of the device itself
    pub ap: Box<str>,
    pub capsman: Capsman,
    /// `band`: `5ghz-a/n/ac`, …
    pub band: Box<str>,
    pub frequency_mhz: Option<u32>,
    /// dBm
    pub noise_floor: Option<i16>,
}

/// The link of an Ethernet port (`/interface/ethernet/monitor`).
#[derive(Debug, Clone, PartialEq)]
pub struct EthernetLink {
    pub name: Box<str>,
    /// `status` `link-ok`
    pub up: bool,
    /// `rate` (`1Gbps`), while the link is up
    pub rate_bits_per_second: Option<u64>,
}

/// A row of `/interface/print`; a counter the interface doesn't report is `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCounters {
    pub name: Box<str>,
    /// `type`: `ether`, `bridge`, `vlan`, `wifi`, …
    pub kind: Box<str>,
    pub running: bool,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    pub rx_packets: Option<u64>,
    pub tx_packets: Option<u64>,
    pub rx_errors: Option<u64>,
    pub tx_errors: Option<u64>,
    pub rx_drops: Option<u64>,
    pub tx_drops: Option<u64>,
    pub link_downs: Option<u64>,
}

/// A bound lease of the device's DHCP servers.
#[derive(Debug, Clone, PartialEq)]
pub struct Lease {
    pub server: Box<str>,
    pub address: IpAddr,
    pub mac_address: MacAddress,
    pub hostname: Option<Box<str>>,
}

impl AccessibleDevice {
    /// The device's [`DeviceMetrics`], as fresh as `freshness` asks (a scrape: begun since it).
    pub async fn metrics(&self, freshness: Freshness) -> Fact<DeviceMetrics> {
        let key = FactKey::new(FactKind::DeviceMetrics, Some(self.device_config.id()), "");
        let device = self.duplicate();
        let ask = async move { read(&device).await };
        facts::get(key, freshness, ask).await
    }
}

async fn read(device: &AccessibleDevice) -> Result<DeviceMetrics, Error> {
    let client = &device.client;
    let api = device.connect_again().await?;
    let capsman = LegacyCapsman::read(client).await?;
    let mut wlan_clients = wifi_clients(client).await?;
    wlan_clients.extend(legacy_clients(client, &capsman).await?);
    let Wireless {
        clients: standalone,
        mut radios,
    } = wireless(client, &api).await?;
    wlan_clients.extend(standalone);
    radios.extend(capsman_radios(client, &capsman).await?);
    let interfaces = collect_resource::<InterfaceState>(client).await?;
    let ethernet = ethernet_links(&api, &interfaces).await?;
    let interfaces = interfaces.iter().map(interface_counters).collect();
    let leases = rows_if_present::<IpDhcpServerLease>(client)
        .await?
        .into_iter()
        .filter_map(bound_lease)
        .collect();
    Ok(DeviceMetrics {
        wlan_clients: wlan_clients.into(),
        radios: radios.into(),
        interfaces,
        ethernet,
        leases,
    })
}

/// The rows of `R`'s menu; none where the device hasn't the menu (its package missing).
async fn rows_if_present<R: DeserializeRosResource + RosResource>(
    client: &MikrotikDevice,
) -> Result<Box<[R]>, Error> {
    match collect_resource::<R>(client).await {
        Err(resource::Error::Trap(trap)) if is_missing_menu(trap.category, &trap.message) => {
            Ok(Box::new([]))
        }
        rows => Ok(rows?),
    }
}

/// RouterOS's answer to a menu it hasn't: `no such command prefix` without a category (the
/// package missing), or a missing item or command.
pub(crate) fn is_missing_menu(category: Option<TrapCategory>, message: &[u8]) -> bool {
    match category {
        None => message == b"no such command prefix",
        Some(category) => category == TrapCategory::MissingItemOrCommand,
    }
}

async fn wifi_clients(client: &MikrotikDevice) -> Result<Vec<WlanClient>, Error> {
    let registrations = rows_if_present::<InterfaceWifiRegistrationTableState>(client).await?;
    if registrations.is_empty() {
        return Ok(Vec::new());
    }
    let masters = rows_if_present::<InterfaceWifi>(client)
        .await?
        .into_iter()
        .filter_map(|wifi| Some((wifi.cfg.name.clone(), wifi.cfg.master_interface.clone()?)))
        .collect::<HashMap<_, _>>();
    let caps = rows_if_present::<InterfaceWifiRadioState>(client)
        .await?
        .into_iter()
        .filter_map(|radio| Some((radio.interface.clone(), radio.cap.clone()?)))
        .collect::<HashMap<_, _>>();
    Ok(registrations
        .iter()
        .map(|client| {
            let radio = masters.get(&client.interface).unwrap_or(&client.interface);
            WlanClient {
                mac_address: client.mac_address,
                interface: client.interface.to_string().into(),
                ssid: client.ssid.to_string().into(),
                ap: caps.get(radio).map(cap_identity).unwrap_or_default(),
                capsman: Capsman::Wifi,
                rx_bytes: client.bytes.rx,
                tx_bytes: client.bytes.tx,
                rx_packets: client.packets.rx,
                tx_packets: client.packets.tx,
                signal: client.signal,
                signal_to_noise: None,
                uptime: client.uptime,
            }
        })
        .collect())
}

/// The interfaces and radios of the legacy CAPsMAN, read once for its clients and radios;
/// none without one.
pub(crate) struct LegacyCapsman {
    pub interfaces: Box<[CapsManInterface]>,
    pub radios: Box<[CapsManRadioState]>,
}

impl LegacyCapsman {
    async fn read(client: &MikrotikDevice) -> Result<Self, Error> {
        let interfaces = rows_if_present::<CapsManInterface>(client).await?;
        let radios = match interfaces.is_empty() {
            true => Box::new([]) as Box<[_]>,
            false => rows_if_present::<CapsManRadioState>(client).await?,
        };
        Ok(LegacyCapsman { interfaces, radios })
    }

    /// The radio `interface` is on: its master, for a virtual interface.
    fn radio_of<'n>(&'n self, interface: &'n AsciiString) -> &'n AsciiString {
        self.interfaces
            .iter()
            .find(|i| i.cfg.name == *interface)
            .and_then(|i| i.cfg.master_interface.as_ref()?.value())
            .unwrap_or(interface)
    }

    /// The identity of the CAP `interface` is on; empty if not known.
    fn cap_of(&self, interface: &AsciiString) -> Box<str> {
        let radio = self.radio_of(interface);
        self.radios
            .iter()
            .find(|r| r.interface == *radio)
            .map(|r| r.remote_cap_identity.to_string().into())
            .unwrap_or_default()
    }
}

async fn legacy_clients(
    client: &MikrotikDevice,
    capsman: &LegacyCapsman,
) -> Result<Vec<WlanClient>, Error> {
    if capsman.interfaces.is_empty() {
        return Ok(Vec::new());
    }
    let registrations = rows_if_present::<CapsManRegistrationTableState>(client).await?;
    Ok(registrations
        .iter()
        .map(|client| WlanClient {
            mac_address: client.mac_address,
            interface: client.interface.to_string().into(),
            ssid: client.ssid.to_string().into(),
            ap: capsman.cap_of(&client.interface),
            capsman: Capsman::Legacy,
            rx_bytes: client.bytes.rx,
            tx_bytes: client.bytes.tx,
            rx_packets: client.packets.rx,
            tx_packets: client.packets.tx,
            signal: client.rx_signal,
            signal_to_noise: None,
            uptime: client.uptime,
        })
        .collect())
}

/// What [`wireless`] found.
struct Wireless {
    clients: Vec<WlanClient>,
    /// the radios running
    radios: Vec<WirelessRadio>,
}

/// The clients and running radios of `/interface/wireless`: its registration table, the
/// radios for SSID and band, `monitor` for noise floor and frequency.
///
/// A radio a CAPsMAN manages has no SSID of its own there (ap-klr-solar, a CAP of the legacy
/// CAPsMAN): its SSID and its clients are the controller's (`legacy_clients`,
/// `capsman_radios`), so its clients are left out here.
async fn wireless(
    client: &MikrotikDevice,
    api: &RawDevice<SimpleResult>,
) -> Result<Wireless, Error> {
    let radios = rows_if_present::<InterfaceWireless>(client).await?;
    let running = radios
        .iter()
        .filter(|radio| radio.status.running)
        .collect::<Vec<_>>();
    if running.is_empty() {
        return Ok(Wireless {
            clients: Vec::new(),
            radios: Vec::new(),
        });
    }
    let ssids = radios
        .iter()
        .filter_map(|radio| Some((&radio.cfg.name, radio.cfg.ssid.as_ref()?)))
        .filter(|(_, ssid)| !ssid.0.is_empty())
        .collect::<HashMap<_, _>>();
    let clients = collect_resource::<InterfaceWirelessRegistrationTableState>(client)
        .await?
        .iter()
        .filter_map(|registration| standalone_client(registration, &ssids))
        .collect();
    let names = running
        .iter()
        .map(|radio| radio.cfg.name.to_string())
        .collect::<Vec<_>>();
    // answered in the order of `numbers`
    let monitor = raw::command(
        api,
        "/interface/wireless/monitor",
        &[("numbers", &names.join(",")), ("once", "")],
    )
    .await?;
    let running_radios = running
        .iter()
        .zip(&monitor)
        .map(|(radio, monitor)| wireless_radio(radio, monitor))
        .collect();
    Ok(Wireless {
        clients,
        radios: running_radios,
    })
}

/// A client of `/interface/wireless/registration-table`, its SSID that of its interface; none
/// for a radio a CAPsMAN manages (not in `ssids`).
pub(crate) fn standalone_client(
    registration: &InterfaceWirelessRegistrationTableState,
    ssids: &HashMap<&AsciiString, &AsciiString>,
) -> Option<WlanClient> {
    let ssid = ssids.get(&registration.interface)?;
    Some(WlanClient {
        mac_address: registration.mac_address,
        interface: registration.interface.to_string().into(),
        ssid: ssid.to_string().into(),
        ap: Box::from(""),
        capsman: Capsman::Standalone,
        rx_bytes: registration.bytes.rx,
        tx_bytes: registration.bytes.tx,
        rx_packets: registration.packets.rx,
        tx_packets: registration.packets.tx,
        // `-71`, older versions `-71@6Mbps`
        signal: registration
            .signal_strength
            .to_string()
            .split('@')
            .next()?
            .parse()
            .ok()?,
        signal_to_noise: registration.signal_to_noise,
        uptime: registration.uptime,
    })
}

/// A running radio and its row of `monitor`; one without SSID is managed by a CAPsMAN.
pub(crate) fn wireless_radio(radio: &InterfaceWireless, monitor: &raw::Row) -> WirelessRadio {
    let ssid = radio
        .cfg
        .ssid
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    WirelessRadio {
        name: radio.cfg.name.to_string().into(),
        ap: Box::from(""),
        capsman: match ssid.is_empty() {
            true => Capsman::Legacy,
            false => Capsman::Standalone,
        },
        ssid: ssid.into(),
        band: radio.cfg.band.as_ref().map(ros_text).unwrap_or_default(),
        frequency_mhz: raw::field(monitor, "channel").and_then(channel_frequency),
        noise_floor: raw::field(monitor, "noise-floor").and_then(|value| value.parse().ok()),
    }
}

/// A value as RouterOS writes it (an enum's name).
fn ros_text<V: RosValue>(value: &V) -> Box<str> {
    String::from_utf8_lossy(&value.encode_ros()).into()
}

/// The frequency of a channel as RouterOS writes it: `5500/20-Ceee/ac/DP(13dBm)`.
fn channel_frequency(channel: &str) -> Option<u32> {
    channel.split('/').next()?.parse().ok()
}

/// The running radios of the legacy CAPsMAN's CAPs, at the controller: SSID and band from
/// `/caps-man/actual-interface-configuration` (what the interface's configuration gives), the
/// frequency from `current-channel`, the CAP from `/caps-man/radio`. A virtual interface's CAP
/// and band are those of its master.
async fn capsman_radios(
    client: &MikrotikDevice,
    capsman: &LegacyCapsman,
) -> Result<Vec<WirelessRadio>, Error> {
    if !capsman.interfaces.iter().any(|i| i.status.running) {
        return Ok(Vec::new());
    }
    let actual = collect_resource::<CapsManActualInterfaceConfiguration>(client).await?;
    Ok(capsman_radio_rows(capsman, &actual))
}

pub(crate) fn capsman_radio_rows(
    capsman: &LegacyCapsman,
    actual: &[CapsManActualInterfaceConfiguration],
) -> Vec<WirelessRadio> {
    let actual_of = |interface: &AsciiString| actual.iter().find(|a| a.cfg.name == *interface);
    let band_of = |interface: &AsciiString| {
        actual_of(interface)
            .and_then(|a| a.cfg.channel_band.as_ref())
            .map(ros_text)
    };
    capsman
        .interfaces
        .iter()
        .filter(|interface| interface.status.running)
        .map(|interface| {
            let name = &interface.cfg.name;
            WirelessRadio {
                name: name.to_string().into(),
                ssid: actual_of(name)
                    .and_then(|a| a.cfg.configuration_ssid.as_ref())
                    .map(|ssid| ssid.to_string().into())
                    .unwrap_or_default(),
                ap: capsman.cap_of(name),
                capsman: Capsman::Legacy,
                band: band_of(name)
                    .or_else(|| band_of(capsman.radio_of(name)))
                    .unwrap_or_default(),
                frequency_mhz: interface
                    .status
                    .current_channel
                    .as_ref()
                    .and_then(|channel| channel_frequency(&channel.to_string())),
                noise_floor: None,
            }
        })
        .collect()
}

/// The link of every Ethernet port (`ether` in `/interface`), from one `monitor` of all of
/// them.
async fn ethernet_links(
    api: &RawDevice<SimpleResult>,
    interfaces: &[InterfaceState],
) -> Result<Box<[EthernetLink]>, Error> {
    let names = interfaces
        .iter()
        .filter(|interface| interface._type.0.as_ref() == b"ether")
        .map(|interface| interface.name.to_string())
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Ok(Box::new([]));
    }
    Ok(raw::command(
        api,
        "/interface/ethernet/monitor",
        &[("numbers", &names.join(",")), ("once", "")],
    )
    .await?
    .iter()
    .filter_map(ethernet_link)
    .collect())
}

/// A row of `/interface/ethernet/monitor`.
pub(crate) fn ethernet_link(row: &raw::Row) -> Option<EthernetLink> {
    let up = raw::field(row, "status") == Some("link-ok");
    Some(EthernetLink {
        name: raw::field(row, "name")?.into(),
        up,
        rate_bits_per_second: raw::field(row, "rate")
            .filter(|_| up)
            .and_then(bits_per_second),
    })
}

/// `10Mbps`, `1Gbps`, `2.5Gbps` as bits per second.
pub(crate) fn bits_per_second(rate: &str) -> Option<u64> {
    let (number, factor) = if let Some(number) = rate.strip_suffix("Gbps") {
        (number, 1e9)
    } else if let Some(number) = rate.strip_suffix("Mbps") {
        (number, 1e6)
    } else {
        (rate.strip_suffix("Kbps")?, 1e3)
    };
    let value = number.parse::<f64>().ok()? * factor;
    Some(value.round() as u64)
}

/// The identity in a radio's `cap` of `/interface/wifi/radio` (`identity@address`).
fn cap_identity(cap: &mikrotik_model::ascii::AsciiString) -> Box<str> {
    let cap = cap.to_string();
    cap.split('@').next().unwrap_or_default().into()
}

/// The counters of an interface of `/interface`.
pub(crate) fn interface_counters(interface: &InterfaceState) -> InterfaceCounters {
    InterfaceCounters {
        name: interface.name.to_string().into(),
        kind: interface._type.to_string().into(),
        running: interface.running,
        rx_bytes: interface.rx_byte,
        tx_bytes: interface.tx_byte,
        rx_packets: interface.rx_packet,
        tx_packets: interface.tx_packet,
        rx_errors: interface.rx_error,
        tx_errors: interface.tx_error,
        rx_drops: interface.rx_drop,
        tx_drops: interface.tx_drop,
        link_downs: interface.link_downs,
    }
}

/// The lease while it is bound to a client: its address and MAC as the client has them.
fn bound_lease(lease: IpDhcpServerLease) -> Option<Lease> {
    let status = lease.status;
    if status.status.as_ref().map(|s| s.0.as_ref()) != Some(b"bound".as_slice()) {
        return None;
    }
    Some(Lease {
        server: status
            .active_server
            .or(lease.cfg.server)
            .map(|server| server.to_string().into())
            .unwrap_or_default(),
        address: status
            .active_address
            .and_then(|address| address.to_string().parse().ok())
            .unwrap_or(lease.cfg.address),
        mac_address: status.active_mac_address.or(lease.cfg.mac_address)?,
        hostname: status.host_name.map(|name| name.to_string().into()),
    })
}

#[cfg(test)]
mod test;
