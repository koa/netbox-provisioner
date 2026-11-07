//! Scope: Tests of the addresses on the interfaces; not the DHCP pool (`dhcp`).
use crate::device::ros::BaseDeviceDataTarget;
use ipnet::{IpNet, Ipv6Net};

/// RouterOS rejects `auto-link-local=no` on anything but a link-local address.
#[test]
pub fn test_auto_link_local_only_off_for_link_local() {
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    let global: Ipv6Net = "2a02:16a:fe00:102::1/64".parse().unwrap();
    let link_local: Ipv6Net = "fe80::1/64".parse().unwrap();
    target.set_ip_address(IpNet::V6(global), "ether2");
    target.set_ip_address(IpNet::V6(link_local), "ether2");
    assert!(target.ipv_6_address[&global].0.auto_link_local);
    assert!(!target.ipv_6_address[&link_local].0.auto_link_local);
}
