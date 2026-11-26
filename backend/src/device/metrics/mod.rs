//! Scope: What `/metrics` reads of a device beyond the network map's reading: its WLAN clients and
//! radios, interface counters, Ethernet links and DHCP leases (the clients and leases also for a
//! resident's view, `graphql::resident::live`); not their rendering as metrics
//! (`crate::metrics`).
//!
//! What the mikrotik-metrik-exporter read (C14), each table as a fact (`access::Reader::at`), over
//! the same way as everything else (`DeviceAccess::connect`).
//! Health, resources and PoE come from the network map's reading
//! (`network_state::device_data`), which the map's viewers share.
//!
//! - **WLAN clients:** the registration tables of `/interface/wifi` and of the legacy CAPsMAN
//!   (`/caps-man`), with the CAP each client is on: a client of a virtual interface is on the
//!   radio of its master interface, the radio names the CAP (`identity@address`, and
//!   `remote-cap-identity` in the legacy CAPsMAN); and of `/interface/wireless` for its own
//!   radios outside a CAPsMAN (a radio link, a standalone AP), with their signal to noise.
//!   Their band: `/interface/wifi` says it per client (`5ghz-ax`); for the others it is the
//!   frequency their radio is on (`current-channel`, `monitor`), without the standard.
//! - **Radios** of `/interface/wireless`: SSID and band, noise floor and frequency (`monitor`,
//!   its `channel` `5500/20-Ceee/ac/…`), for each running one; and at a controller of the
//!   legacy CAPsMAN its CAPs' radios (`/caps-man/interface`).
//! - **Interfaces:** `/interface`, which carries the counters without `stats`.
//! - **Ethernet links:** `/interface/ethernet/monitor` of every `ether` interface, link and rate.
//!
//! Every table is read with its mikrotik-model type; the monitors are actions of mikrotik-model's
//! catalog.
//! - **Leases:** the bound ones of `/ip/dhcp-server/lease`, to name a WLAN client by its DHCP
//!   hostname wherever its lease is (on the router, not the AP).
//! - **Bridge hosts:** the MACs `/interface/bridge/host` learned at a port (not the bridge's
//!   own), with the port's default name from `/interface`: which device is behind a resident's
//!   port.
//!
//! A table the device doesn't have (no `wifi` package, no legacy CAPsMAN) has no rows
//! (`access::collect_if_present`).

use super::{
    AccessibleDevice,
    access::{Reader, Row, collect_if_present, field, observe},
};
use crate::{Error, facts::Freshness};
use async_graphql::{Enum, SimpleObject};
use mikrotik_model::value::RosValue;
use mikrotik_model::{
    MacAddress,
    action::{EthernetMonitorOnce, WirelessMonitorOnce},
    ascii::AsciiString,
    model::{
        CapsManActualInterfaceConfiguration, CapsManInterface, CapsManRadioState,
        CapsManRegistrationTableState, InterfaceBridgeHostState, InterfaceEthernetState,
        InterfaceState, InterfaceWifi, InterfaceWifiRadioState,
        InterfaceWifiRegistrationTableState, InterfaceWireless,
        InterfaceWirelessRegistrationTableState, IpDhcpServerLease, SystemClockState,
    },
    resource::{SingleResource, collect_resource},
    value::{ClockDate, LogTime, TimeOfDay},
};
use std::{collections::HashMap, net::IpAddr, time::Duration};

#[derive(Debug, Clone, Default)]
pub struct DeviceMetrics {
    pub wlan_clients: Box<[WlanClient]>,
    pub radios: Box<[WirelessRadio]>,
    pub interfaces: Box<[InterfaceCounters]>,
    pub ethernet: Box<[EthernetLink]>,
    pub ethernet_counters: Box<[EthernetCounters]>,
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
    /// none where the registration or its radio doesn't tell
    pub band: Option<WlanBand>,
}

/// The band a WLAN client is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SimpleObject)]
pub struct WlanBand {
    pub frequency: WlanFrequency,
    /// only `/interface/wifi` reports the client's
    pub standard: Option<WlanStandard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum WlanFrequency {
    Ghz2,
    Ghz5,
    Ghz6,
}

impl WlanFrequency {
    /// The band of a channel at `mhz`.
    pub(crate) fn of_mhz(mhz: u32) -> Option<Self> {
        match mhz {
            2400..=2500 => Some(WlanFrequency::Ghz2),
            4900..=5924 => Some(WlanFrequency::Ghz5),
            5925..=7125 => Some(WlanFrequency::Ghz6),
            _ => None,
        }
    }
}

/// IEEE 802.11 by its letters: `ax` is Wi-Fi 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum WlanStandard {
    A,
    B,
    G,
    N,
    Ac,
    Ax,
    Be,
}

/// `band` of `/interface/wifi/registration-table`: `5ghz-ax`, `2ghz-n`; a standard not known
/// here leaves it out.
pub(crate) fn wifi_band(band: &str) -> Option<WlanBand> {
    let (frequency, standard) = band.split_once('-')?;
    let frequency = match frequency {
        "2ghz" => WlanFrequency::Ghz2,
        "5ghz" => WlanFrequency::Ghz5,
        "6ghz" => WlanFrequency::Ghz6,
        _ => return None,
    };
    let standard = match standard {
        "a" => Some(WlanStandard::A),
        "b" => Some(WlanStandard::B),
        "g" => Some(WlanStandard::G),
        "n" => Some(WlanStandard::N),
        "ac" => Some(WlanStandard::Ac),
        "ax" => Some(WlanStandard::Ax),
        "be" => Some(WlanStandard::Be),
        _ => None,
    };
    Some(WlanBand {
        frequency,
        standard,
    })
}

/// The band of a client whose radio is on `mhz`: no standard.
fn band_at(mhz: Option<u32>) -> Option<WlanBand> {
    Some(WlanBand {
        frequency: WlanFrequency::of_mhz(mhz?)?,
        standard: None,
    })
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

/// What an Ethernet port's switch or driver counts (`/interface/ethernet`): the drops and errors
/// `/interface` lacks on a switch chip's ports. Each board reports some of them, a counter it
/// doesn't is `None`: `rx-drop`/`tx-drop` (hEX, SXT), `rx-overflow`/`tx-drop-packet` (CRS1xx/2xx,
/// CRS318), `tx-collision` or `tx-total-collision` (SXT), `rx-fcs-error` every board of the lab.
#[derive(Debug, Clone, PartialEq)]
pub struct EthernetCounters {
    /// the port's name (`/interface`), by its `default-name`
    pub name: Box<str>,
    /// frames with a wrong checksum: a bad cable or connector
    pub rx_fcs_errors: Option<u64>,
    /// frames received, dropped for want of room
    pub rx_overflows: Option<u64>,
    pub rx_drops: Option<u64>,
    /// packets to send, dropped (`tx-drop`, else `tx-drop-packet`)
    pub tx_drops: Option<u64>,
    /// `tx-collision`, else `tx-total-collision`: a duplex mismatch
    pub collisions: Option<u64>,
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

/// A MAC the device's bridges learned at one of its ports.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeHost {
    pub mac_address: MacAddress,
    /// the port's name
    pub port: Box<str>,
    /// the port's default name, where it has one (`ether3`)
    pub default_name: Option<Box<str>>,
}

/// An Ethernet port of the device, enabled, and whether it has a link (`running`).
#[derive(Debug, Clone, PartialEq)]
pub struct PortLink {
    /// the port's name
    pub name: Box<str>,
    /// the port's default name, where it has one (`ether3`)
    pub default_name: Option<Box<str>>,
    pub up: bool,
    /// down, how long since its link went, by the device's clock; none where it had none since
    /// the boot
    pub down_for: Option<Duration>,
}

impl AccessibleDevice {
    /// The device's [`DeviceMetrics`], as fresh as `freshness` asks (a scrape: begun since it).
    pub async fn metrics(&self, freshness: Freshness) -> Result<DeviceMetrics, Error> {
        let metrics = read(&self.reader().at(freshness)).await?;
        self.note_clients(&metrics.wlan_clients);
        Ok(metrics)
    }
    /// The bound leases of the device's DHCP servers.
    pub async fn leases(&self, freshness: Freshness) -> Result<Box<[Lease]>, Error> {
        let client = self.reader().at(freshness);
        Ok(collect_if_present::<IpDhcpServerLease>(&client)
            .await?
            .into_iter()
            .filter_map(bound_lease)
            .collect())
    }
    /// The MACs the device's bridges learned at its ports, without the bridges' own.
    pub async fn bridge_hosts(&self, freshness: Freshness) -> Result<Box<[BridgeHost]>, Error> {
        let client = self.reader().at(freshness);
        let interfaces = collect_resource::<InterfaceState>(&client).await?;
        let hosts = collect_resource::<InterfaceBridgeHostState>(&client).await?;
        Ok(learned(&interfaces, hosts))
    }
    /// Whether its enabled Ethernet ports have a link, and since when one is down.
    pub async fn port_links(&self, freshness: Freshness) -> Result<Box<[PortLink]>, Error> {
        let client = self.reader().at(freshness);
        let interfaces = collect_resource::<InterfaceState>(&client).await?;
        let clock = SystemClockState::fetch(&client).await?;
        Ok(links(&interfaces, clock.as_ref()))
    }
    /// The WLAN clients the device's CAPsMANs and its own radios have registered.
    pub async fn wlan_clients(&self, freshness: Freshness) -> Result<Box<[WlanClient]>, Error> {
        let client = self.reader().at(freshness);
        let capsman = LegacyCapsman::read(&client).await?;
        let mut clients = wifi_clients(&client).await?;
        clients.extend(legacy_clients(&client, &capsman).await?);
        clients.extend(wireless(&client).await?.clients);
        self.note_clients(&clients);
        Ok(clients.into())
    }
    /// Keeps which clients each access point has (`wlan_seen`).
    fn note_clients(&self, clients: &[WlanClient]) {
        let device = &self.device_config;
        super::wlan_seen::note(device.id(), device.name(), clients);
    }
}

async fn read(client: &Reader) -> Result<DeviceMetrics, Error> {
    let capsman = LegacyCapsman::read(client).await?;
    let mut wlan_clients = wifi_clients(client).await?;
    wlan_clients.extend(legacy_clients(client, &capsman).await?);
    let Wireless {
        clients: standalone,
        mut radios,
    } = wireless(client).await?;
    wlan_clients.extend(standalone);
    radios.extend(capsman_radios(client, &capsman).await?);
    let interfaces = collect_resource::<InterfaceState>(client).await?;
    let ethernet = ethernet_links(client, &interfaces).await?;
    let ethernet_counters = collect_resource::<InterfaceEthernetState>(client)
        .await?
        .iter()
        .filter_map(|port| ethernet_counters(port, &interfaces))
        .collect();
    let interfaces = interfaces.iter().map(interface_counters).collect();
    let leases = collect_if_present::<IpDhcpServerLease>(client)
        .await?
        .into_iter()
        .filter_map(bound_lease)
        .collect();
    Ok(DeviceMetrics {
        wlan_clients: wlan_clients.into(),
        radios: radios.into(),
        interfaces,
        ethernet,
        ethernet_counters,
        leases,
    })
}

/// The rows of `R`'s table; none where the device hasn't the table (its package missing).
async fn wifi_clients(client: &Reader) -> Result<Vec<WlanClient>, Error> {
    let registrations = collect_if_present::<InterfaceWifiRegistrationTableState>(client).await?;
    if registrations.is_empty() {
        return Ok(Vec::new());
    }
    let masters = collect_if_present::<InterfaceWifi>(client)
        .await?
        .into_iter()
        .filter_map(|wifi| Some((wifi.cfg.name.clone(), wifi.cfg.master_interface.clone()?)))
        .collect::<HashMap<_, _>>();
    let caps = collect_if_present::<InterfaceWifiRadioState>(client)
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
                band: wifi_band(&client.band.to_string()),
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
    async fn read(client: &Reader) -> Result<Self, Error> {
        let interfaces = collect_if_present::<CapsManInterface>(client).await?;
        let radios = match interfaces.is_empty() {
            true => Box::new([]) as Box<[_]>,
            false => collect_if_present::<CapsManRadioState>(client).await?,
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

    /// The frequency (MHz) `interface` sends on, from its `current-channel` or its master's.
    fn frequency_of(&self, interface: &AsciiString) -> Option<u32> {
        [interface, self.radio_of(interface)]
            .into_iter()
            .find_map(|name| {
                self.interfaces
                    .iter()
                    .find(|i| i.cfg.name == *name)?
                    .status
                    .current_channel
                    .as_ref()
            })
            .and_then(|channel| channel_frequency(&channel.to_string()))
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
    client: &Reader,
    capsman: &LegacyCapsman,
) -> Result<Vec<WlanClient>, Error> {
    if capsman.interfaces.is_empty() {
        return Ok(Vec::new());
    }
    let registrations = collect_if_present::<CapsManRegistrationTableState>(client).await?;
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
            band: band_at(capsman.frequency_of(&client.interface)),
        })
        .collect())
}

/// What [`wireless`] found.
struct Wireless {
    clients: Box<[WlanClient]>,
    /// the radios running
    radios: Vec<WirelessRadio>,
}

/// The clients and running radios of `/interface/wireless`: its registration table, the
/// radios for SSID and band, `monitor` for noise floor and frequency.
///
/// A radio a CAPsMAN manages has no SSID of its own there (ap-klr-solar, a CAP of the legacy
/// CAPsMAN): its SSID and its clients are the controller's (`legacy_clients`,
/// `capsman_radios`), so its clients are left out here.
async fn wireless(client: &Reader) -> Result<Wireless, Error> {
    let radios = collect_if_present::<InterfaceWireless>(client).await?;
    let running = radios
        .iter()
        .filter(|radio| radio.status.running)
        .collect::<Vec<_>>();
    if running.is_empty() {
        return Ok(Wireless {
            clients: Box::default(),
            radios: Vec::new(),
        });
    }
    let ssids = radios
        .iter()
        .filter_map(|radio| Some((&radio.cfg.name, radio.cfg.ssid.as_ref()?)))
        .filter(|(_, ssid)| !ssid.0.is_empty())
        .collect::<HashMap<_, _>>();
    let registrations = collect_resource::<InterfaceWirelessRegistrationTableState>(client).await?;
    let names = running
        .iter()
        .map(|radio| radio.cfg.name.to_string())
        .collect::<Vec<_>>();
    // answered in the order of `numbers`
    let numbers = names.join(",");
    let monitor = observe(client, &WirelessMonitorOnce { numbers: &numbers }).await?;
    let running_radios = running
        .iter()
        .zip(&monitor)
        .map(|(radio, monitor)| wireless_radio(radio, monitor))
        .collect::<Vec<_>>();
    // a client's band is the frequency its radio is on
    let frequencies = running_radios
        .iter()
        .filter_map(|radio| Some((&*radio.name, radio.frequency_mhz?)))
        .collect::<HashMap<_, _>>();
    let clients = registrations
        .iter()
        .filter_map(|registration| standalone_client(registration, &ssids))
        .map(|client| WlanClient {
            band: band_at(frequencies.get(&*client.interface).copied()),
            ..client
        })
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
        // the frequency of its radio, from `monitor` (`wireless`)
        band: None,
    })
}

/// A running radio and its row of `monitor`; one without SSID is managed by a CAPsMAN.
pub(crate) fn wireless_radio(radio: &InterfaceWireless, monitor: &Row) -> WirelessRadio {
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
        frequency_mhz: field(monitor, "channel").and_then(channel_frequency),
        noise_floor: field(monitor, "noise-floor").and_then(|value| value.parse().ok()),
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
    client: &Reader,
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
    client: &Reader,
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
    let numbers = names.join(",");
    Ok(observe(client, &EthernetMonitorOnce { numbers: &numbers })
        .await?
        .iter()
        .filter_map(ethernet_link)
        .collect())
}

/// A row of `/interface/ethernet/monitor`.
pub(crate) fn ethernet_link(row: &Row) -> Option<EthernetLink> {
    let up = field(row, "status") == Some("link-ok");
    Some(EthernetLink {
        name: field(row, "name")?.into(),
        up,
        rate_bits_per_second: field(row, "rate").filter(|_| up).and_then(bits_per_second),
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

/// The counters of an Ethernet port, named as `/interface` names the port with its
/// `default-name`; none for a port `/interface` doesn't list.
pub(crate) fn ethernet_counters(
    port: &InterfaceEthernetState,
    interfaces: &[InterfaceState],
) -> Option<EthernetCounters> {
    let interface = interfaces
        .iter()
        .find(|interface| interface.default_name.as_ref() == Some(&port.default_name))?;
    Some(EthernetCounters {
        name: interface.name.to_string().into(),
        rx_fcs_errors: port.rx_fcs_error,
        rx_overflows: port.rx_overflow,
        rx_drops: port.rx_drop,
        tx_drops: port.tx_drop.or(port.tx_drop_packet),
        collisions: port.tx_collision.or(port.tx_total_collision),
    })
}

/// The lease while it is bound to a client: its address and MAC as the client has them.
/// The `hosts` learned at a port, with the port's default name from `interfaces`.
fn learned(
    interfaces: &[InterfaceState],
    hosts: Box<[InterfaceBridgeHostState]>,
) -> Box<[BridgeHost]> {
    let default_names = interfaces
        .iter()
        .filter_map(|interface| {
            Some((
                interface.name.to_string(),
                interface.default_name.as_ref()?.to_string(),
            ))
        })
        .collect::<HashMap<_, _>>();
    hosts
        .into_vec()
        .into_iter()
        .filter(|host| host.local != Some(true))
        .filter_map(|host| {
            let port = host.on_interface.or(host.interface)?.to_string();
            Some(BridgeHost {
                mac_address: host.mac_address,
                default_name: default_names.get(&port).map(|name| name.as_str().into()),
                port: port.into(),
            })
        })
        .collect()
}

/// The enabled Ethernet ports among `interfaces`, each with whether it is running: a disabled
/// one has no link by choice, nothing to judge.
fn links(interfaces: &[InterfaceState], clock: Option<&SystemClockState>) -> Box<[PortLink]> {
    interfaces
        .iter()
        .filter(|interface| interface._type.0.as_ref() == b"ether")
        .filter(|interface| interface.disabled != Some(true))
        .map(|interface| PortLink {
            name: interface.name.to_string().into(),
            default_name: interface
                .default_name
                .as_ref()
                .map(|name| name.to_string().into()),
            up: interface.running,
            down_for: match interface.running {
                true => None,
                false => interface
                    .last_link_down_time
                    .zip(clock)
                    .and_then(|(down, clock)| since(down, clock.date, clock.time)),
            },
        })
        .collect()
}

/// The time from `then` to the device's date and time now, both its local time; none for a
/// time without its date (RouterOS 6) or after now.
pub(crate) fn since(then: LogTime, date: ClockDate, time: TimeOfDay) -> Option<Duration> {
    let LogTime::At {
        year,
        month,
        day,
        time: then_time,
    } = then
    else {
        return None;
    };
    let moment = |year: u16, month: u8, day: u8, time: TimeOfDay| {
        let date =
            time::Date::from_calendar_date(year.into(), time::Month::try_from(month).ok()?, day)
                .ok()?;
        let time = time::Time::from_hms(time.hour, time.minute, time.second.min(59)).ok()?;
        Some(time::PrimitiveDateTime::new(date, time))
    };
    let then = moment(year, month, day, then_time)?;
    let now = moment(date.year, date.month, date.day, time)?;
    (now - then).try_into().ok()
}

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
