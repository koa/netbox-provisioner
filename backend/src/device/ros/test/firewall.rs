//! The perimeter firewall (`device::ros::firewall`, `docs/firewall.md`).
use super::empty_current;
use crate::{
    device::ros::{BaseDeviceDataTarget, l2::KeepNameGenerator},
    topology::{Device, Interface, PhysicalPortId, TopologyHolder, builder::TopologyBuilder},
};
use mikrotik_model::{
    generator::Generator,
    model::{IpFirewallFilterAction, IpFirewallFilterById, IpFirewallFilterCfgBuilder},
    resource::DeserializeRosBuilder,
    value::Id,
};
use std::time::Duration;

/// A router with `ether1` tagged as given (`internet`, `nat`) and `ether2` without tags.
async fn router(internet: bool, nat: bool, stage: Option<Duration>) -> BaseDeviceDataTarget {
    let mut builder = TopologyBuilder::default();
    let core = builder.devices.next_id();
    builder.devices.insert(
        core,
        Device {
            name: "core".into(),
            has_routeros: true,
            ssh_stage_time: stage,
            ..Default::default()
        },
    );
    for (number, tagged) in [(1, true), (2, false)] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{number}").into(),
                device: core,
                external: Some(PhysicalPortId::Ethernet(number)),
                enable_dhcp_client: true,
                internet: tagged && internet,
                nat: tagged && nat,
                ..Default::default()
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    let core = topology.devices_by_id(core).await.unwrap();
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    target.generate_from(&core, KeepNameGenerator).unwrap();
    target
}

fn v4_comments(target: &BaseDeviceDataTarget) -> Vec<String> {
    target
        .ip_firewall_filter
        .iter()
        .map(|rule| rule.comment.as_ref().unwrap().to_string())
        .collect()
}
fn v6_comments(target: &BaseDeviceDataTarget) -> Vec<String> {
    target
        .ipv_6_firewall_filter
        .iter()
        .map(|rule| rule.comment.as_ref().unwrap().to_string())
        .collect()
}

/// An interface to the internet gets the rules, all from the list `uplink`, the drop ending
/// each chain marked to go last; IPv6 lets ICMPv6 and the DHCPv6 client's answers in, IPv4
/// what a port forwarding lets in.
#[tokio::test]
async fn an_internet_uplink_gets_the_perimeter() {
    let target = router(true, false, None).await;
    let prefixed = |names: &[&str]| {
        names.iter().map(|name| format!("netbox-provisioner: {name}")).collect::<Vec<_>>()
    };
    let ssh = [
        "input ssh blacklist",
        "input ssh stage3",
        "input ssh stage2",
        "input ssh stage1",
        "input ssh",
    ];
    let v4 = [
        &["input established", "input invalid", "input ssh blacklisted", "input icmp"][..],
        &ssh,
        &["last input drop", "forward established", "forward invalid", "forward dstnat", "last forward drop"],
    ]
    .concat();
    assert_eq!(v4_comments(&target), prefixed(&v4));
    let v6 = [
        &["input established", "input invalid", "input ssh blacklisted", "input icmpv6", "input dhcpv6 client"][..],
        &ssh,
        &["last input drop", "forward established", "forward invalid", "last forward drop"],
    ]
    .concat();
    assert_eq!(v6_comments(&target), prefixed(&v6));
    assert!(target
        .ip_firewall_filter
        .iter()
        .all(|rule| rule.in_interface_list.as_ref().map(|l| l.to_string()).as_deref() == Some("uplink")));
    // the stages from the end: the blacklist after the third, 10 days, the others a minute
    let blacklist = &target.ip_firewall_filter[4];
    assert_eq!(blacklist.action, IpFirewallFilterAction::AddSrcToAddressList);
    assert_eq!(blacklist.src_address_list.as_ref().map(|l| l.to_string()).as_deref(), Some("ssh_stage3"));
    assert_eq!(blacklist.address_list.as_ref().map(|l| l.to_string()).as_deref(), Some("ssh_blacklist"));
    assert_eq!(blacklist.address_list_timeout, Some(Duration::from_secs(10 * 24 * 3600)));
    let first = &target.ip_firewall_filter[7];
    assert_eq!(first.src_address_list, None);
    assert_eq!(first.address_list_timeout, Some(Duration::from_secs(60)));
    // `internet` alone masquerades nothing
    assert!(target.ip_firewall_nat.is_empty());
}

/// `nat` without `internet` is guarded all the same (finding `NatWithoutInternet`); a device
/// without either gets no rule.
#[tokio::test]
async fn nat_alone_gets_it_too_and_no_uplink_none() {
    let nat = router(false, true, None).await;
    assert_eq!(v4_comments(&nat), v4_comments(&router(true, true, None).await));
    assert_eq!(nat.ip_firewall_nat.len(), 1);
    let inside = router(false, false, None).await;
    assert!(inside.ip_firewall_filter.is_empty());
    assert!(inside.ipv_6_firewall_filter.is_empty());
}

/// The config context's `ssh_stage_time` sets the stages' timeout.
#[tokio::test]
async fn the_stage_time_comes_from_the_config_context() {
    let target = router(true, false, Some(Duration::from_secs(120))).await;
    assert_eq!(target.ip_firewall_filter[7].address_list_timeout, Some(Duration::from_secs(120)));
    assert_eq!(target.ip_firewall_filter[4].address_list_timeout, Some(Duration::from_secs(10 * 24 * 3600)));
}

/// The provisioner's networks come after `invalid`, before the SSH stages, each in its
/// family's menu; a device without uplink gets none.
#[tokio::test]
async fn the_provisioner_gets_in_before_the_ssh_stages() {
    let networks = ["192.168.0.0/16".parse().unwrap(), "2a02:16a:fe00:8000::/50".parse().unwrap()];
    let mut target = router(true, false, None).await;
    target.allow_provisioner(&networks);
    assert_eq!(v4_comments(&target)[2], "netbox-provisioner: input provisioner 192.168.0.0/16");
    assert_eq!(v4_comments(&target)[3], "netbox-provisioner: input ssh blacklisted");
    assert_eq!(v6_comments(&target)[2], "netbox-provisioner: input provisioner 2a02:16a:fe00:8000::/50");
    assert_eq!(v4_comments(&target).len(), 15);
    let mut inside = router(false, false, None).await;
    inside.allow_provisioner(&networks);
    assert!(inside.ip_firewall_filter.is_empty());
}

/// A single IPv4 address goes in without `/32`, as RouterOS reports it (CHR 7.24.5: else the
/// diff never got empty); an IPv6 one keeps its `/128`.
#[tokio::test]
async fn a_host_is_written_as_routeros_reports_it() {
    let mut target = router(true, false, None).await;
    target.allow_provisioner(&["100.64.0.1/32".parse().unwrap(), "2001:db8:64::1/128".parse().unwrap()]);
    let v4 = target.ip_firewall_filter[2].src_address.as_ref().map(|a| a.to_string());
    assert_eq!(v4.as_deref(), Some("100.64.0.1"));
    let v6 = target.ipv_6_firewall_filter[2].src_address.as_ref().map(|a| a.to_string());
    assert_eq!(v6.as_deref(), Some("2001:db8:64::1/128"));
}

/// Against a rule set up by hand the rules go before it, the drops after it: what it lets in
/// still gets in.
#[tokio::test]
async fn the_drops_go_after_a_rule_set_up_by_hand() {
    let target = router(true, false, None).await;
    let mut current = empty_current(&target);
    let by_hand = IpFirewallFilterCfgBuilder {
        action: Some(IpFirewallFilterAction::Accept),
        chain: Some(b"forward".into()),
        comment: Some(b"http ingress".into()),
        dst_port: Some(b"443".into()),
        protocol: Some(b"tcp".into()),
        ..Default::default()
    }
    .build()
    .unwrap();
    current.ip_firewall_filter = Box::new([IpFirewallFilterById { id: Id(1), data: by_hand }]);
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in target.generate_mutations(&current).unwrap() {
        if mutation.resource == b"ip/firewall/filter".as_slice() {
            generator.append_mutation(&mutation).unwrap();
        }
    }
    let moves = script.lines().filter(|line| line.starts_with("move")).collect::<Vec<_>>();
    assert_eq!(moves.len(), 14, "{script}");
    assert!(moves[..12].iter().all(|line| line.ends_with("destination=*1")), "{script}");
    assert_eq!(
        moves[12..],
        [
            "move [find comment=\"netbox-provisioner: last input drop\" dynamic=no]",
            "move [find comment=\"netbox-provisioner: last forward drop\" dynamic=no]",
        ]
    );
    assert!(!script.contains("remove"), "{script}");
}
