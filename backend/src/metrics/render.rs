//! Scope: A round of the collector as Prometheus metric families; not the reading.
//!
//! Names after Prometheus' conventions: the unit in the name, `_total` for a counter of the
//! device (RouterOS counts, Prometheus takes the rate). Every metric has `device` and `site`
//! from NetBox. A value the device doesn't report is left out, not given as 0. A WLAN client
//! and a radio also have `wlan_group`: the NetBox Wireless LAN Group of the device (as
//! controller or AP) with a WLAN of that SSID and, for a CAP's client, that CAP among its APs;
//! empty where none or several fit (a radio link).

use crate::{
    device::metrics::{
        Capsman, DeviceMetrics, EthernetLink, InterfaceCounters, Lease, WirelessRadio, WlanClient,
    },
    network_state::device_data::{DeviceMapData, HealthRow, Resources},
    topology::access::device::DeviceAccess,
};
use mikrotik_model::{MacAddress, model::InterfaceEthernetPoeMonitor, value::RosValue};
use prometheus::{CounterVec, GaugeVec, Opts, core::Collector, proto::MetricFamily};
use std::collections::HashMap;

/// One round of the collector.
#[derive(Debug, Clone, Default)]
pub(crate) struct Snapshot {
    pub devices: Box<[DeviceSample]>,
}

/// What one device answered in a round.
#[derive(Debug, Clone)]
pub(crate) struct DeviceSample {
    /// NetBox's name
    pub device: Box<str>,
    /// NetBox's site, empty without one
    pub site: Box<str>,
    /// it answered the reading
    pub up: bool,
    /// health, resources and PoE (the network map's reading)
    pub map: Option<DeviceMapData>,
    pub metrics: Option<DeviceMetrics>,
    /// the Wireless LAN Groups the device is the controller or an AP of
    pub wlan_groups: Box<[WlanGroupSample]>,
}

/// What names a WLAN client's or radio's group: its WLANs and APs.
#[derive(Debug, Clone)]
pub(crate) struct WlanGroupSample {
    pub name: Box<str>,
    pub ssids: Box<[Box<str>]>,
    /// NetBox's names, which the provisioner gives the CAPs as identity
    pub aps: Box<[Box<str>]>,
}

/// The one group of `groups` with a WLAN `ssid` and, with an `ap`, that AP; empty if none or
/// several fit.
pub(crate) fn wlan_group_of<'g>(groups: &'g [WlanGroupSample], ssid: &str, ap: &str) -> &'g str {
    let mut fitting = groups.iter().filter(|group| {
        group.ssids.iter().any(|s| &**s == ssid)
            && (ap.is_empty() || group.aps.iter().any(|a| &**a == ap))
    });
    match (fitting.next(), fitting.next()) {
        (Some(group), None) => &group.name,
        _ => "",
    }
}

impl DeviceSample {
    /// `device` without an answer.
    pub fn down(device: &DeviceAccess) -> Self {
        DeviceSample {
            device: device.name().into(),
            site: device
                .site()
                .map(|site| site.name().into())
                .unwrap_or_default(),
            up: false,
            map: None,
            metrics: None,
            wlan_groups: device
                .topology()
                .list_wlan_groups()
                .filter(|group| {
                    group.controller().is_some_and(|c| c.id() == device.id())
                        || group.aps().iter().any(|ap| ap.id() == device.id())
                })
                .filter_map(|group| {
                    Some(WlanGroupSample {
                        name: group.name()?.into(),
                        ssids: group
                            .wlan()
                            .filter_map(|w| w.ssid().map(Box::from))
                            .collect(),
                        aps: group.aps().iter().map(|ap| Box::from(ap.name())).collect(),
                    })
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Gauge,
    Counter,
}

/// A metric family under construction: its samples, each with the values of its labels.
struct Family {
    kind: Kind,
    name: &'static str,
    help: &'static str,
    labels: &'static [&'static str],
    samples: Vec<Sample>,
}

struct Sample {
    labels: Box<[Box<str>]>,
    value: f64,
}

impl Family {
    fn new(
        kind: Kind,
        name: &'static str,
        help: &'static str,
        labels: &'static [&'static str],
    ) -> Self {
        Family {
            kind,
            name,
            help,
            labels,
            samples: Vec::new(),
        }
    }
    fn push(&mut self, labels: Box<[Box<str>]>, value: f64) {
        debug_assert_eq!(labels.len(), self.labels.len(), "{}", self.name);
        self.samples.push(Sample { labels, value });
    }
    fn render(self) -> Vec<MetricFamily> {
        fn values(sample: &Sample) -> Vec<&str> {
            sample.labels.iter().map(Box::as_ref).collect()
        }
        let opts = Opts::new(self.name, self.help);
        let rendered = match self.kind {
            Kind::Gauge => GaugeVec::new(opts, self.labels).map(|vec| {
                for sample in &self.samples {
                    vec.with_label_values(&values(sample)).set(sample.value);
                }
                vec.collect()
            }),
            Kind::Counter => CounterVec::new(opts, self.labels).map(|vec| {
                for sample in &self.samples {
                    // a counter of the device: a fresh one carries its value
                    vec.with_label_values(&values(sample))
                        .inc_by(sample.value.max(0.0));
                }
                vec.collect()
            }),
        };
        rendered.unwrap_or_default()
    }
}

fn labels<const N: usize>(values: [&str; N]) -> Box<[Box<str>]> {
    values.into_iter().map(Into::into).collect()
}

const DEVICE: &[&str] = &["device", "site"];
const WITH_INTERFACE: &[&str] = &["device", "site", "interface"];
const WITH_HEALTH: &[&str] = &["device", "site", "name"];
const WLAN_CLIENT: &[&str] = &[
    "device",
    "site",
    "interface",
    "wlan_group",
    "ap",
    "ssid",
    "capsman",
    "mac_address",
    "client_hostname",
    "client_ip",
];

/// Every metric of `snapshot`. A WLAN client takes its hostname and address from a bound lease
/// of any device, by MAC.
pub(crate) fn families(snapshot: &Snapshot) -> Vec<MetricFamily> {
    let leases = snapshot
        .devices
        .iter()
        .filter_map(|device| device.metrics.as_ref())
        .flat_map(|metrics| metrics.leases.iter())
        .map(|lease| (lease.mac_address, lease))
        .collect::<HashMap<MacAddress, &Lease>>();
    let mut out = Families::default();
    for device in &snapshot.devices {
        out.device(device, &leases);
    }
    // a family no device has a value of (no fan anywhere) is refused by the text encoder
    out.render()
        .into_iter()
        .filter(|family| !family.get_metric().is_empty())
        .collect()
}

/// The families of a scrape, by topic.
struct Families {
    up: Family,
    system: SystemFamilies,
    health: HealthFamilies,
    poe: PoeFamilies,
    interface: InterfaceFamilies,
    ethernet: EthernetFamilies,
    radio: RadioFamilies,
    wlan: WlanFamilies,
    lease: Family,
}

struct SystemFamilies {
    info: Family,
    uptime: Family,
    cpu_load: Family,
    memory_free: Family,
    memory_total: Family,
    disk_free: Family,
    disk_total: Family,
}

struct HealthFamilies {
    temperature: Family,
    voltage: Family,
    current: Family,
    power: Family,
    fan: Family,
}

struct PoeFamilies {
    status: Family,
    voltage: Family,
    current: Family,
    power: Family,
}

struct InterfaceFamilies {
    running: Family,
    rx_bytes: Family,
    tx_bytes: Family,
    rx_packets: Family,
    tx_packets: Family,
    rx_errors: Family,
    tx_errors: Family,
    rx_drops: Family,
    tx_drops: Family,
    link_downs: Family,
}

struct EthernetFamilies {
    up: Family,
    rate: Family,
}

struct RadioFamilies {
    info: Family,
    frequency: Family,
    noise_floor: Family,
}

struct WlanFamilies {
    rx_bytes: Family,
    tx_bytes: Family,
    rx_packets: Family,
    tx_packets: Family,
    signal: Family,
    signal_to_noise: Family,
    uptime: Family,
}

impl Default for Families {
    fn default() -> Self {
        use Kind::{Counter, Gauge};
        let interface = |name, help| Family::new(Counter, name, help, WITH_INTERFACE);
        let wlan_counter = |name, help| Family::new(Counter, name, help, WLAN_CLIENT);
        let health = |name, help| Family::new(Gauge, name, help, WITH_HEALTH);
        let poe = |name, help| Family::new(Gauge, name, help, WITH_INTERFACE);
        let system = |name, help| Family::new(Gauge, name, help, DEVICE);
        Families {
            up: Family::new(
                Gauge,
                "mikrotik_up",
                "1 if the device answered the last reading, 0 if not",
                DEVICE,
            ),
            system: SystemFamilies {
                info: Family::new(
                    Gauge,
                    "mikrotik_system_info",
                    "the RouterOS version and the board, always 1",
                    &["device", "site", "version", "board"],
                ),
                uptime: system("mikrotik_system_uptime_seconds", "time since the last boot"),
                cpu_load: system("mikrotik_system_cpu_load_ratio", "CPU load, 0 to 1"),
                memory_free: system("mikrotik_system_memory_free_bytes", "free memory"),
                memory_total: system("mikrotik_system_memory_total_bytes", "total memory"),
                disk_free: system("mikrotik_system_disk_free_bytes", "free storage"),
                disk_total: system("mikrotik_system_disk_total_bytes", "total storage"),
            },
            health: HealthFamilies {
                temperature: health(
                    "mikrotik_health_temperature_celsius",
                    "a temperature sensor",
                ),
                voltage: health("mikrotik_health_voltage_volts", "a voltage sensor"),
                current: health("mikrotik_health_current_amperes", "a current sensor"),
                power: health("mikrotik_health_power_watts", "a power sensor"),
                fan: health("mikrotik_health_fan_rpm", "a fan's speed"),
            },
            poe: PoeFamilies {
                status: Family::new(
                    Gauge,
                    "mikrotik_poe_out_status",
                    "the PoE out state of a port (poe-out-status), always 1",
                    &["device", "site", "interface", "status"],
                ),
                voltage: poe(
                    "mikrotik_poe_out_voltage_volts",
                    "PoE out voltage of a port",
                ),
                current: poe(
                    "mikrotik_poe_out_current_amperes",
                    "PoE out current of a port",
                ),
                power: poe("mikrotik_poe_out_power_watts", "PoE out power of a port"),
            },
            interface: InterfaceFamilies {
                running: Family::new(
                    Gauge,
                    "mikrotik_interface_running",
                    "1 if the interface runs (link up), 0 if not",
                    &["device", "site", "interface", "type"],
                ),
                rx_bytes: interface("mikrotik_interface_rx_bytes_total", "bytes received"),
                tx_bytes: interface("mikrotik_interface_tx_bytes_total", "bytes sent"),
                rx_packets: interface("mikrotik_interface_rx_packets_total", "packets received"),
                tx_packets: interface("mikrotik_interface_tx_packets_total", "packets sent"),
                rx_errors: interface("mikrotik_interface_rx_errors_total", "receive errors"),
                tx_errors: interface("mikrotik_interface_tx_errors_total", "send errors"),
                rx_drops: interface("mikrotik_interface_rx_drops_total", "received, dropped"),
                tx_drops: interface("mikrotik_interface_tx_drops_total", "to send, dropped"),
                link_downs: interface(
                    "mikrotik_interface_link_downs_total",
                    "times the link went down",
                ),
            },
            ethernet: EthernetFamilies {
                up: Family::new(
                    Gauge,
                    "mikrotik_ethernet_link_up",
                    "1 if the Ethernet port has a link (status link-ok), 0 if not",
                    WITH_INTERFACE,
                ),
                rate: Family::new(
                    Gauge,
                    "mikrotik_ethernet_rate_bits_per_second",
                    "the rate the Ethernet port's link runs at",
                    WITH_INTERFACE,
                ),
            },
            radio: RadioFamilies {
                info: Family::new(
                    Gauge,
                    "mikrotik_wireless_radio_info",
                    "a running radio of /interface/wireless or a CAP of /caps-man, always 1",
                    &[
                        "device",
                        "site",
                        "interface",
                        "ssid",
                        "band",
                        "wlan_group",
                        "ap",
                        "capsman",
                    ],
                ),
                frequency: Family::new(
                    Gauge,
                    "mikrotik_wireless_radio_frequency_hertz",
                    "the frequency a radio of /interface/wireless sends on",
                    WITH_INTERFACE,
                ),
                noise_floor: Family::new(
                    Gauge,
                    "mikrotik_wireless_radio_noise_floor_dbm",
                    "the noise floor a radio of /interface/wireless hears",
                    WITH_INTERFACE,
                ),
            },
            wlan: WlanFamilies {
                rx_bytes: wlan_counter(
                    "mikrotik_wlan_client_rx_bytes_total",
                    "bytes from the client",
                ),
                tx_bytes: wlan_counter(
                    "mikrotik_wlan_client_tx_bytes_total",
                    "bytes to the client",
                ),
                rx_packets: wlan_counter(
                    "mikrotik_wlan_client_rx_packets_total",
                    "packets from the client",
                ),
                tx_packets: wlan_counter(
                    "mikrotik_wlan_client_tx_packets_total",
                    "packets to the client",
                ),
                signal: Family::new(
                    Gauge,
                    "mikrotik_wlan_client_signal_dbm",
                    "the client's signal at the AP",
                    WLAN_CLIENT,
                ),
                signal_to_noise: Family::new(
                    Gauge,
                    "mikrotik_wlan_client_signal_to_noise_db",
                    "the client's signal over the noise at the AP (/interface/wireless only)",
                    WLAN_CLIENT,
                ),
                uptime: Family::new(
                    Gauge,
                    "mikrotik_wlan_client_uptime_seconds",
                    "time since the client registered",
                    WLAN_CLIENT,
                ),
            },
            lease: Family::new(
                Gauge,
                "mikrotik_dhcp_lease",
                "a bound DHCP lease, always 1",
                &[
                    "device",
                    "site",
                    "server",
                    "address",
                    "mac_address",
                    "hostname",
                ],
            ),
        }
    }
}

impl Families {
    fn device(&mut self, sample: &DeviceSample, leases: &HashMap<MacAddress, &Lease>) {
        let (device, site) = (&*sample.device, &*sample.site);
        self.up
            .push(labels([device, site]), if sample.up { 1.0 } else { 0.0 });
        if let Some(map) = &sample.map {
            if let Some(resources) = &map.resources {
                self.system.push(device, site, resources);
            }
            for row in &map.health {
                self.health.push(device, site, row);
            }
            for row in &map.poe.monitor {
                self.poe.push(device, site, row);
            }
        }
        if let Some(metrics) = &sample.metrics {
            for counters in &metrics.interfaces {
                self.interface.push(device, site, counters);
            }
            for link in &metrics.ethernet {
                self.ethernet.push(device, site, link);
            }
            for radio in &metrics.radios {
                let group = wlan_group_of(&sample.wlan_groups, &radio.ssid, &radio.ap);
                self.radio.push(device, site, group, radio);
            }
            for client in &metrics.wlan_clients {
                let group = wlan_group_of(&sample.wlan_groups, &client.ssid, &client.ap);
                self.wlan
                    .push(device, site, group, client, leases.get(&client.mac_address));
            }
            for lease in &metrics.leases {
                let address = lease.address.to_string();
                let mac = lease.mac_address.to_string();
                self.lease.push(
                    labels([
                        device,
                        site,
                        &lease.server,
                        &address,
                        &mac,
                        lease.hostname.as_deref().unwrap_or_default(),
                    ]),
                    1.0,
                );
            }
        }
    }
    fn render(self) -> Vec<MetricFamily> {
        let Families {
            up,
            system,
            health,
            poe,
            interface,
            ethernet,
            radio,
            wlan,
            lease,
        } = self;
        [
            up,
            system.info,
            system.uptime,
            system.cpu_load,
            system.memory_free,
            system.memory_total,
            system.disk_free,
            system.disk_total,
            health.temperature,
            health.voltage,
            health.current,
            health.power,
            health.fan,
            poe.status,
            poe.voltage,
            poe.current,
            poe.power,
            interface.running,
            interface.rx_bytes,
            interface.tx_bytes,
            interface.rx_packets,
            interface.tx_packets,
            interface.rx_errors,
            interface.tx_errors,
            interface.rx_drops,
            interface.tx_drops,
            interface.link_downs,
            ethernet.up,
            ethernet.rate,
            radio.info,
            radio.frequency,
            radio.noise_floor,
            wlan.rx_bytes,
            wlan.tx_bytes,
            wlan.rx_packets,
            wlan.tx_packets,
            wlan.signal,
            wlan.signal_to_noise,
            wlan.uptime,
            lease,
        ]
        .into_iter()
        .flat_map(Family::render)
        .collect()
    }
}

impl SystemFamilies {
    fn push(&mut self, device: &str, site: &str, resources: &Resources) {
        let base = || labels([device, site]);
        self.info.push(
            labels([device, site, &resources.version, &resources.board_name]),
            1.0,
        );
        self.uptime.push(base(), resources.uptime.as_secs_f64());
        self.cpu_load
            .push(base(), f64::from(resources.cpu_load) / 100.0);
        self.memory_free.push(base(), resources.free_memory as f64);
        self.memory_total
            .push(base(), resources.total_memory as f64);
        self.disk_free.push(base(), resources.free_hdd_space as f64);
        self.disk_total
            .push(base(), resources.total_hdd_space as f64);
    }
}

impl HealthFamilies {
    /// A sensor by its unit (`type`); a state without one (`state`, `fan-state`) or a unit
    /// not known here is left out.
    fn push(&mut self, device: &str, site: &str, row: &HealthRow) {
        let Ok(value) = row.value.parse::<f64>() else {
            return;
        };
        let family = match row.unit.as_deref() {
            Some("C") => &mut self.temperature,
            Some("V") => &mut self.voltage,
            Some("A") => &mut self.current,
            Some("W") => &mut self.power,
            Some("RPM") => &mut self.fan,
            _ => return,
        };
        family.push(labels([device, site, &row.name]), value);
    }
}

impl PoeFamilies {
    /// A reading of `/interface/ethernet/poe/monitor`; `poe-out-current` is in mA.
    fn push(&mut self, device: &str, site: &str, reading: &InterfaceEthernetPoeMonitor) {
        let interface = reading.name.to_string();
        let interface = interface.as_str();
        // RouterOS's name as the label value
        let status = String::from_utf8_lossy(&reading.poe_out_status.encode_ros()).into_owned();
        self.status
            .push(labels([device, site, interface, &status]), 1.0);
        let port = || labels([device, site, interface]);
        if let Some(voltage) = reading.poe_out_voltage {
            self.voltage.push(port(), voltage);
        }
        if let Some(current) = reading.poe_out_current {
            self.current.push(port(), current / 1000.0);
        }
        if let Some(power) = reading.poe_out_power {
            self.power.push(port(), power);
        }
    }
}

impl InterfaceFamilies {
    fn push(&mut self, device: &str, site: &str, counters: &InterfaceCounters) {
        let name = &*counters.name;
        self.running.push(
            labels([device, site, name, &counters.kind]),
            if counters.running { 1.0 } else { 0.0 },
        );
        let port = || labels([device, site, name]);
        for (family, value) in [
            (&mut self.rx_bytes, counters.rx_bytes),
            (&mut self.tx_bytes, counters.tx_bytes),
            (&mut self.rx_packets, counters.rx_packets),
            (&mut self.tx_packets, counters.tx_packets),
            (&mut self.rx_errors, counters.rx_errors),
            (&mut self.tx_errors, counters.tx_errors),
            (&mut self.rx_drops, counters.rx_drops),
            (&mut self.tx_drops, counters.tx_drops),
            (&mut self.link_downs, counters.link_downs),
        ] {
            if let Some(value) = value {
                family.push(port(), value as f64);
            }
        }
    }
}

impl EthernetFamilies {
    /// A port's link; the rate only while it is up.
    fn push(&mut self, device: &str, site: &str, link: &EthernetLink) {
        let port = || labels([device, site, &link.name]);
        self.up.push(port(), if link.up { 1.0 } else { 0.0 });
        if let Some(rate) = link.rate_bits_per_second {
            self.rate.push(port(), rate as f64);
        }
    }
}

impl RadioFamilies {
    fn push(&mut self, device: &str, site: &str, wlan_group: &str, radio: &WirelessRadio) {
        let port = || labels([device, site, &radio.name]);
        // a CAP's radio its CAPsMAN manages: the controller has its SSID
        if !radio.ssid.is_empty() {
            self.info.push(
                labels([
                    device,
                    site,
                    &radio.name,
                    &radio.ssid,
                    &radio.band,
                    wlan_group,
                    &radio.ap,
                    capsman_label(radio.capsman),
                ]),
                1.0,
            );
        }
        if let Some(frequency) = radio.frequency_mhz {
            self.frequency.push(port(), f64::from(frequency) * 1e6);
        }
        if let Some(noise_floor) = radio.noise_floor {
            self.noise_floor.push(port(), f64::from(noise_floor));
        }
    }
}

fn capsman_label(capsman: Capsman) -> &'static str {
    match capsman {
        Capsman::Wifi => "wifi",
        Capsman::Legacy => "legacy",
        Capsman::Standalone => "none",
    }
}

impl WlanFamilies {
    fn push(
        &mut self,
        device: &str,
        site: &str,
        wlan_group: &str,
        client: &WlanClient,
        lease: Option<&&Lease>,
    ) {
        let mac = client.mac_address.to_string();
        let address = lease
            .map(|lease| lease.address.to_string())
            .unwrap_or_default();
        let hostname = lease
            .and_then(|lease| lease.hostname.as_deref())
            .unwrap_or_default();
        let capsman = capsman_label(client.capsman);
        let client_labels = || {
            labels([
                device,
                site,
                &client.interface,
                wlan_group,
                &client.ap,
                &client.ssid,
                capsman,
                &mac,
                hostname,
                &address,
            ])
        };
        self.rx_bytes.push(client_labels(), client.rx_bytes as f64);
        self.tx_bytes.push(client_labels(), client.tx_bytes as f64);
        self.rx_packets
            .push(client_labels(), client.rx_packets as f64);
        self.tx_packets
            .push(client_labels(), client.tx_packets as f64);
        self.signal.push(client_labels(), f64::from(client.signal));
        if let Some(signal_to_noise) = client.signal_to_noise {
            self.signal_to_noise
                .push(client_labels(), f64::from(signal_to_noise));
        }
        self.uptime
            .push(client_labels(), client.uptime.as_secs_f64());
    }
}
