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
//! - **Radios** of `/interface/wireless`: SSID and band (`print`), noise floor and frequency
//!   (`monitor`, its `channel` `5500/20-Ceee/ac/…`), for each running one.
//! - **Interfaces:** `/interface/print`, which carries the counters without `stats`.
//! - **Ethernet links:** `/interface/ethernet/monitor` of every port, link and rate.
//! - **Leases:** the bound ones of `/ip/dhcp-server/lease`, to name a WLAN client by its DHCP
//!   hostname wherever its lease is (on the router, not the AP).
//!
//! A menu the device doesn't have (no `wifi` package, no legacy CAPsMAN) has no rows
//! (`raw::is_missing_menu`).

use super::{AccessibleDevice, raw};
use crate::{
    Error,
    facts::{self, Fact, FactKey, FactKind, Freshness},
};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use mikrotik_model::value::{ParseRosValueResult, RosValue, StatsPair};
use mikrotik_model::{
    MacAddress, MikrotikDevice,
    model::{
        CapsManInterfaceById, CapsManRadioState, CapsManRegistrationTableState, InterfaceWifi,
        InterfaceWifiRadioState, InterfaceWifiRegistrationTableState, IpDhcpServerLease,
    },
    resource::{self, DeserializeRosResource, RosResource, collect_resource},
};
use std::{collections::HashMap, net::IpAddr, time::Duration};

/// How old a reading may be: Prometheus scrapes every minute or so, and the collector reads
/// once per interval (`crate::metrics`).
pub const DEVICE_METRICS_AGE: Duration = Duration::from_secs(30);

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
    /// The device's [`DeviceMetrics`], at most [`DEVICE_METRICS_AGE`] old.
    pub async fn metrics(&self) -> Fact<DeviceMetrics> {
        let key = FactKey::new(FactKind::DeviceMetrics, Some(self.device_config.id()), "");
        let device = self.duplicate();
        let ask = async move { read(&device).await };
        facts::get(key, Freshness::MaxAge(DEVICE_METRICS_AGE), ask).await
    }
}

async fn read(device: &AccessibleDevice) -> Result<DeviceMetrics, Error> {
    let client = &device.client;
    let api = device.connect_again().await?;
    let mut wlan_clients = wifi_clients(client).await?;
    wlan_clients.extend(legacy_clients(client).await?);
    let (standalone, mut radios) = wireless(&api).await?;
    wlan_clients.extend(standalone);
    radios.extend(capsman_radios(&api).await?);
    let interfaces = raw::rows(&api, "/interface/print")
        .await?
        .iter()
        .filter_map(interface_counters)
        .collect();
    let ethernet = ethernet_links(&api).await?;
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
        Err(resource::Error::Trap(trap)) if raw::is_missing_menu(trap.category, &trap.message) => {
            Ok(Box::new([]))
        }
        rows => Ok(rows?),
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

async fn legacy_clients(client: &MikrotikDevice) -> Result<Vec<WlanClient>, Error> {
    let registrations = rows_if_present::<CapsManRegistrationTableState>(client).await?;
    if registrations.is_empty() {
        return Ok(Vec::new());
    }
    let masters = rows_if_present::<CapsManInterfaceById>(client)
        .await?
        .into_iter()
        .filter_map(|interface| {
            let master = interface.data.master_interface.as_ref()?.value()?.clone();
            Some((interface.data.name.clone(), master))
        })
        .collect::<HashMap<_, _>>();
    let caps = rows_if_present::<CapsManRadioState>(client)
        .await?
        .into_iter()
        .map(|radio| (radio.interface.clone(), radio.remote_cap_identity.clone()))
        .collect::<HashMap<_, _>>();
    Ok(registrations
        .iter()
        .map(|client| {
            let radio = masters.get(&client.interface).unwrap_or(&client.interface);
            WlanClient {
                mac_address: client.mac_address,
                interface: client.interface.to_string().into(),
                ssid: client.ssid.to_string().into(),
                ap: caps
                    .get(radio)
                    .map(|cap| cap.to_string().into())
                    .unwrap_or_default(),
                capsman: Capsman::Legacy,
                rx_bytes: client.bytes.rx,
                tx_bytes: client.bytes.tx,
                rx_packets: client.packets.rx,
                tx_packets: client.packets.tx,
                signal: client.rx_signal,
                signal_to_noise: None,
                uptime: client.uptime,
            }
        })
        .collect())
}

/// The clients and running radios of `/interface/wireless`: its registration table, `print`
/// for SSID and band, `monitor` for noise floor and frequency.
///
/// A radio a CAPsMAN manages has no SSID of its own there (ap-klr-solar, a CAP of the legacy
/// CAPsMAN): its SSID and its clients are the controller's (`legacy_clients`,
/// `capsman_radios`), so its clients are left out here.
async fn wireless(
    api: &RawDevice<SimpleResult>,
) -> Result<(Vec<WlanClient>, Vec<WirelessRadio>), Error> {
    let radios = raw::command_if_present(api, "/interface/wireless/print", &[]).await?;
    let running = radios
        .iter()
        .filter(|row| raw::field(row, "running") == Some("true"))
        .filter_map(|row| raw::field(row, "name"))
        .collect::<Vec<_>>();
    if running.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let ssids = radios
        .iter()
        .filter_map(|row| Some((raw::field(row, "name")?, raw::field(row, "ssid")?)))
        .filter(|(_, ssid)| !ssid.is_empty())
        .collect::<HashMap<_, _>>();
    let clients = raw::rows(api, "/interface/wireless/registration-table/print")
        .await?
        .iter()
        .filter_map(|row| standalone_client(row, &ssids))
        .collect();
    let monitor = raw::command(
        api,
        "/interface/wireless/monitor",
        &[("numbers", &running.join(",")), ("once", "")],
    )
    .await?;
    let running_radios = radios
        .iter()
        .filter(|row| raw::field(row, "running") == Some("true"))
        .zip(&monitor)
        .filter_map(|(print, monitor)| wireless_radio(print, monitor))
        .collect();
    Ok((clients, running_radios))
}

/// A row of `/interface/wireless/registration-table`, its SSID that of its interface.
pub(crate) fn standalone_client(row: &raw::Row, ssids: &HashMap<&str, &str>) -> Option<WlanClient> {
    let field = |name| raw::field(row, name);
    let pair = |name| match StatsPair::<u64>::parse_ros(field(name)?.as_bytes()) {
        ParseRosValueResult::Value(pair) => Some(pair),
        _ => None,
    };
    let interface = field("interface")?;
    // a radio a CAPsMAN manages: the controller has the client
    let ssid = ssids.get(interface)?;
    let bytes = pair("bytes")?;
    let packets = pair("packets")?;
    Some(WlanClient {
        mac_address: field("mac-address")?.parse().ok()?,
        interface: interface.into(),
        ssid: (*ssid).into(),
        ap: Box::from(""),
        capsman: Capsman::Standalone,
        rx_bytes: bytes.rx,
        tx_bytes: bytes.tx,
        rx_packets: packets.rx,
        tx_packets: packets.tx,
        // `-71`, older versions `-71@6Mbps`
        signal: field("signal-strength")?.split('@').next()?.parse().ok()?,
        signal_to_noise: field("signal-to-noise").and_then(|value| value.parse().ok()),
        uptime: match Duration::parse_ros(field("uptime")?.as_bytes()) {
            ParseRosValueResult::Value(uptime) => uptime,
            _ => return None,
        },
    })
}

/// A running radio from its row of `print` and of `monitor`; one without SSID is managed by a
/// CAPsMAN.
pub(crate) fn wireless_radio(print: &raw::Row, monitor: &raw::Row) -> Option<WirelessRadio> {
    let ssid = raw::field(print, "ssid").unwrap_or_default();
    Some(WirelessRadio {
        name: raw::field(print, "name")?.into(),
        ssid: ssid.into(),
        ap: Box::from(""),
        capsman: match ssid.is_empty() {
            true => Capsman::Legacy,
            false => Capsman::Standalone,
        },
        band: raw::field(print, "band").unwrap_or_default().into(),
        frequency_mhz: raw::field(monitor, "channel").and_then(channel_frequency),
        noise_floor: raw::field(monitor, "noise-floor").and_then(|value| value.parse().ok()),
    })
}

/// The frequency of a channel as RouterOS writes it: `5500/20-Ceee/ac/DP(13dBm)`.
fn channel_frequency(channel: &str) -> Option<u32> {
    channel.split('/').next()?.parse().ok()
}

/// The running radios of the legacy CAPsMAN's CAPs, at the controller: SSID and band from
/// `/caps-man/actual-interface-configuration` (asked for those fields only: its rows hold the
/// passphrase), the frequency from `current-channel`, the CAP from `/caps-man/radio`. A
/// virtual interface's CAP and band are those of its master.
async fn capsman_radios(api: &RawDevice<SimpleResult>) -> Result<Vec<WirelessRadio>, Error> {
    let interfaces = raw::command_if_present(
        api,
        "/caps-man/interface/print",
        &[(".proplist", "name,running,current-channel,master-interface")],
    )
    .await?;
    if !interfaces
        .iter()
        .any(|row| raw::field(row, "running") == Some("true"))
    {
        return Ok(Vec::new());
    }
    let actual = raw::command(
        api,
        "/caps-man/actual-interface-configuration/print",
        &[(".proplist", "name,configuration.ssid,channel.band")],
    )
    .await?;
    let radios = raw::command(
        api,
        "/caps-man/radio/print",
        &[(".proplist", "interface,remote-cap-identity")],
    )
    .await?;
    Ok(capsman_radio_rows(&CapsmanRows {
        interfaces: &interfaces,
        actual: &actual,
        radios: &radios,
    }))
}

/// The rows `capsman_radios` reads.
pub(crate) struct CapsmanRows<'r> {
    pub interfaces: &'r [raw::Row],
    pub actual: &'r [raw::Row],
    pub radios: &'r [raw::Row],
}

pub(crate) fn capsman_radio_rows(rows: &CapsmanRows) -> Vec<WirelessRadio> {
    fn by_name(table: &[raw::Row]) -> HashMap<&str, &raw::Row> {
        table
            .iter()
            .filter_map(|row| Some((raw::field(row, "name")?, row)))
            .collect()
    }
    /// `field` of `interface`'s row, unless empty.
    fn of<'r>(
        table: &HashMap<&str, &'r raw::Row>,
        interface: &str,
        field: &str,
    ) -> Option<&'r str> {
        table
            .get(interface)
            .and_then(|row| raw::field(row, field))
            .filter(|value| !value.is_empty())
    }
    let actual = by_name(rows.actual);
    let masters = by_name(rows.interfaces)
        .into_iter()
        .filter_map(|(name, row)| Some((name, raw::field(row, "master-interface")?)))
        .filter(|(_, master)| *master != "none")
        .collect::<HashMap<_, _>>();
    let caps = rows
        .radios
        .iter()
        .filter_map(|row| {
            Some((
                raw::field(row, "interface")?,
                raw::field(row, "remote-cap-identity")?,
            ))
        })
        .collect::<HashMap<_, _>>();
    rows.interfaces
        .iter()
        .filter(|row| raw::field(row, "running") == Some("true"))
        .filter_map(|row| {
            let name = raw::field(row, "name")?;
            let radio = masters.get(name).copied().unwrap_or(name);
            Some(WirelessRadio {
                name: name.into(),
                ssid: of(&actual, name, "configuration.ssid")
                    .unwrap_or_default()
                    .into(),
                ap: caps.get(radio).copied().unwrap_or_default().into(),
                capsman: Capsman::Legacy,
                band: of(&actual, name, "channel.band")
                    .or_else(|| of(&actual, radio, "channel.band"))
                    .unwrap_or_default()
                    .into(),
                frequency_mhz: raw::field(row, "current-channel").and_then(channel_frequency),
                noise_floor: None,
            })
        })
        .collect()
}

/// The link of every Ethernet port, from one `monitor` of all of them.
async fn ethernet_links(api: &RawDevice<SimpleResult>) -> Result<Box<[EthernetLink]>, Error> {
    let ports = raw::rows(api, "/interface/ethernet/print").await?;
    let names = ports
        .iter()
        .filter_map(|row| raw::field(row, "name"))
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

/// The counters of a row of `/interface/print`; a row without a name is left out.
pub(crate) fn interface_counters(row: &raw::Row) -> Option<InterfaceCounters> {
    let number = |name: &str| raw::field(row, name).and_then(|value| value.parse().ok());
    Some(InterfaceCounters {
        name: raw::field(row, "name")?.into(),
        kind: raw::field(row, "type").unwrap_or_default().into(),
        running: raw::field(row, "running") == Some("true"),
        rx_bytes: number("rx-byte"),
        tx_bytes: number("tx-byte"),
        rx_packets: number("rx-packet"),
        tx_packets: number("tx-packet"),
        rx_errors: number("rx-error"),
        tx_errors: number("tx-error"),
        rx_drops: number("rx-drop"),
        tx_drops: number("tx-drop"),
        link_downs: number("link-downs"),
    })
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
