//! Scope: Tests of `graphql::resident::live`; nothing else.
use super::{behind, joined, plugged_in};
use crate::device::metrics::{BridgeHost, Capsman, Lease, WlanClient};
use crate::{
    device::neighbors::SeenNeighbor,
    topology::findings::{DeviceRef, InterfaceRef},
};
use std::{str::FromStr, time::Duration};

fn mac(text: &str) -> mikrotik_model::MacAddress {
    mikrotik_model::MacAddress::from_str(text).expect("a MAC")
}

fn client(address: &str, ssid: &str) -> WlanClient {
    WlanClient {
        mac_address: mac(address),
        interface: "wifi1".into(),
        ssid: ssid.into(),
        ap: "lab-wap".into(),
        capsman: Capsman::Wifi,
        rx_bytes: 0,
        tx_bytes: 0,
        rx_packets: 0,
        tx_packets: 0,
        signal: -60,
        signal_to_noise: None,
        uptime: Duration::ZERO,
        band: None,
    }
}

fn lease(address: &str, mac_text: &str, hostname: Option<&str>) -> Lease {
    Lease {
        server: "dhcp-vlan104".into(),
        address: address.parse().expect("an address"),
        mac_address: mac(mac_text),
        hostname: hostname.map(Box::from),
    }
}

/// A lease and a WLAN client with one MAC are one device; a client without lease comes last,
/// the leases by address; a host name padded with NULs ends at them.
#[test]
fn leases_and_clients_join_by_mac() {
    let devices = joined(
        &[
            lease("192.168.104.20", "02:00:00:00:00:02", Some("tablet\0\0")),
            lease("192.168.104.3", "02:00:00:00:00:01", None),
        ],
        &[
            client("02:00:00:00:00:02", "lab-wlan"),
            client("02:00:00:00:00:09", "lab-wlan"),
        ],
    );
    let addresses = devices
        .iter()
        .map(|device| device.address.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        addresses,
        [Some("192.168.104.3"), Some("192.168.104.20"), None]
    );
    assert_eq!(devices[1].hostname.as_deref(), Some("tablet"));
    assert!(devices[1].wlan.is_some());
    assert!(devices[0].wlan.is_none());
    assert!(devices[2].wlan.is_some());
}

fn seen(device: u32, port: Option<u32>) -> SeenNeighbor {
    let device_ref = DeviceRef {
        id: device,
        name: format!("device {device}").into(),
        site: Some(1),
    };
    SeenNeighbor {
        identity: device_ref.name.clone(),
        interface_name: "ether2".into(),
        board: "cAP ax".into(),
        device: Some(device_ref.clone()),
        interface: port.map(|id| InterfaceRef {
            id,
            name: "ether2".into(),
            device: device_ref,
        }),
    }
}

/// The router seeing the device at another NetBox port than its Anschluss names that port;
/// at its Anschluss, at a port NetBox doesn't have, or only another device: nothing.
#[test]
fn the_port_the_router_sees_the_device_at() {
    let other = seen(7, Some(71));
    assert_eq!(
        plugged_in(6, 61, &[other.clone(), seen(6, Some(62))]).map(|port| port.id),
        Some(62)
    );
    assert_eq!(plugged_in(6, 61, &[seen(6, Some(61))]), None);
    assert_eq!(plugged_in(6, 61, &[seen(6, None)]), None);
    assert_eq!(plugged_in(6, 61, &[other]), None);
}

fn host(mac_text: &str, port: &str, default_name: Option<&str>) -> BridgeHost {
    BridgeHost {
        mac_address: mac(mac_text),
        port: port.into(),
        default_name: default_name.map(Box::from),
    }
}

/// Behind a port are the leases whose MAC the bridge learned there, by the port's own or its
/// default name (NetBox's); a MAC learned elsewhere or without lease isn't.
#[test]
fn the_leases_behind_a_port() {
    let hosts = [
        host("02:00:00:00:00:01", "e03-wohnung", Some("ether3")),
        host("02:00:00:00:00:02", "e04-buero", Some("ether4")),
        host("02:00:00:00:00:03", "e03-wohnung", Some("ether3")),
    ];
    let leases = [
        lease("192.168.104.20", "02:00:00:00:00:01", Some("laptop")),
        lease("192.168.104.21", "02:00:00:00:00:02", None),
    ];
    let devices = behind("ether3", &hosts, &leases);
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].address.as_deref(), Some("192.168.104.20"));
    assert_eq!(devices[0].hostname.as_deref(), Some("laptop"));
    assert_eq!(behind("e04-buero", &hosts, &leases).len(), 1);
    assert!(behind("ether5", &hosts, &leases).is_empty());
}
