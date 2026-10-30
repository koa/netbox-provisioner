//! The perimeter firewall (`docs/firewall.md`): the rules on every device with an interface to
//! the internet (NetBox tag `internet`, or `nat`, which needs it), for IPv4 and IPv6 alike,
//! all on `input` and `forward` from the interface list `uplink`. Only what comes from there is
//! filtered: the rest of the network routes as before, without a rule.
//!
//! The rules come first, before those set up by hand; the drop that ends each chain comes after
//! them (`OWNED_LAST`), so what a hand-made rule lets in (router-faesigrund-keller's web servers
//! and VPNs) still gets in.
//!
//! The NetBox services with tag `forward` (`topology::forwards`) open ports through it: an
//! IPv4 one by a port forwarding on its site's router with `nat`, which `forward dstnat` lets
//! in, an IPv6 one by an accept on `forward` before its drop.
use super::{BaseDeviceDataTarget, UPLINK_LIST, firewall_comment};
use crate::topology::{
    ServicePort,
    access::device::DeviceAccess,
    forwards::PortForward,
};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        IpFirewallFilterAction, IpFirewallFilterCfg, IpFirewallFilterCfgBuilder, IpFirewallNatAction,
        IpFirewallNatCfg, Ipv6FirewallFilterAction, Ipv6FirewallFilterCfg,
        Ipv6FirewallFilterCfgBuilder,
    },
    resource::{DeserializeRosBuilder, OWNED_LAST},
};
use std::{collections::BTreeSet, net::Ipv6Addr, time::Duration};

/// The address lists of the SSH bruteforce protection, each stage adding the source to the
/// next (MikroTik's guide); what reaches the last is dropped.
const SSH_STAGES: [&str; 4] = ["ssh_stage1", "ssh_stage2", "ssh_stage3", "ssh_blacklist"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    V4,
    V6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Accept,
    Drop,
    /// the source into the list, for that long; the packet goes on to the next rule
    AddSource(&'static str, Duration),
}

/// One rule of the perimeter, the same for both families but where it says otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Rule {
    /// what it is, in its comment: unique in its chain
    name: String,
    chain: &'static str,
    /// the drop ending the chain, after the rules of others
    last: bool,
    action: Option<Action>,
    protocol: Option<&'static str>,
    dst_port: Option<String>,
    connection_state: Option<&'static str>,
    connection_nat_state: Option<&'static str>,
    src_address: Option<IpNet>,
    src_address_list: Option<&'static str>,
    dst_address: Option<IpNet>,
}

impl Rule {
    fn new(chain: &'static str, name: &str, action: Action) -> Self {
        Rule {
            name: name.into(),
            chain,
            action: Some(action),
            ..Default::default()
        }
    }
    fn comment(&self) -> AsciiString {
        let last = String::from_utf8_lossy(OWNED_LAST);
        let last = last.trim_end();
        if self.last {
            firewall_comment(&[last, self.chain, &self.name])
        } else {
            firewall_comment(&[self.chain, &self.name])
        }
    }
    fn v4(&self) -> IpFirewallFilterCfg {
        let (action, address_list, timeout) = match self.action.unwrap_or(Action::Accept) {
            Action::Accept => (IpFirewallFilterAction::Accept, None, None),
            Action::Drop => (IpFirewallFilterAction::Drop, None, None),
            Action::AddSource(list, timeout) => {
                (IpFirewallFilterAction::AddSrcToAddressList, Some(list.into()), Some(timeout))
            }
        };
        IpFirewallFilterCfgBuilder {
            action: Some(action),
            address_list,
            address_list_timeout: timeout,
            chain: Some(self.chain.into()),
            comment: Some(self.comment()),
            connection_nat_state: self.connection_nat_state.map(Into::into),
            connection_state: self.connection_state.map(Into::into),
            dst_address: self.dst_address.map(ros_address),
            dst_port: self.dst_port.as_deref().map(Into::into),
            in_interface_list: Some(UPLINK_LIST.as_slice().into()),
            protocol: self.protocol.map(Into::into),
            src_address: self.src_address.map(ros_address),
            src_address_list: self.src_address_list.map(Into::into),
            ..Default::default()
        }
        .build()
        .expect("a perimeter rule has its chain and action")
    }
    fn v6(&self) -> Ipv6FirewallFilterCfg {
        let (action, address_list, timeout) = match self.action.unwrap_or(Action::Accept) {
            Action::Accept => (Ipv6FirewallFilterAction::Accept, None, None),
            Action::Drop => (Ipv6FirewallFilterAction::Drop, None, None),
            Action::AddSource(list, timeout) => {
                (Ipv6FirewallFilterAction::AddSrcToAddressList, Some(list.into()), Some(timeout))
            }
        };
        Ipv6FirewallFilterCfgBuilder {
            action: Some(action),
            address_list,
            address_list_timeout: timeout,
            chain: Some(self.chain.into()),
            comment: Some(self.comment()),
            connection_nat_state: self.connection_nat_state.map(Into::into),
            connection_state: self.connection_state.map(Into::into),
            dst_address: self.dst_address.map(ros_address),
            dst_port: self.dst_port.as_deref().map(Into::into),
            in_interface_list: Some(UPLINK_LIST.as_slice().into()),
            protocol: self.protocol.map(Into::into),
            src_address: self.src_address.map(ros_address),
            src_address_list: self.src_address_list.map(Into::into),
            ..Default::default()
        }
        .build()
        .expect("a perimeter rule has its chain and action")
    }
}

/// A network as RouterOS reports it in a rule: an IPv4 host without `/32` (CHR 7.24.5; an
/// IPv6 host keeps its `/128`).
fn ros_address(net: IpNet) -> AsciiString {
    match net {
        IpNet::V4(v4) if v4.prefix_len() == 32 => v4.addr().to_string(),
        net => net.to_string(),
    }
    .as_str()
    .into()
}

/// The rules of `family` in their order, the SSH stages lasting `stage`, the blacklist
/// `blacklist`.
fn perimeter(family: Family, stage: Duration, blacklist: Duration) -> Vec<Rule> {
    let mut rules = Vec::new();
    for chain in ["input", "forward"] {
        rules.push(Rule {
            connection_state: Some("established,related"),
            ..Rule::new(chain, "established", Action::Accept)
        });
        rules.push(Rule {
            connection_state: Some("invalid"),
            ..Rule::new(chain, "invalid", Action::Drop)
        });
        if chain == "input" {
            let [.., blacklisted] = SSH_STAGES;
            rules.push(Rule {
                src_address_list: Some(blacklisted),
                ..Rule::new(chain, "ssh blacklisted", Action::Drop)
            });
            rules.push(match family {
                Family::V4 => Rule {
                    protocol: Some("icmp"),
                    ..Rule::new(chain, "icmp", Action::Accept)
                },
                // ND and path MTU need it
                Family::V6 => Rule {
                    protocol: Some("icmpv6"),
                    ..Rule::new(chain, "icmpv6", Action::Accept)
                },
            });
            if family == Family::V6 {
                // the answers to the DHCPv6 client of the uplink (prefix delegation)
                rules.push(Rule {
                    protocol: Some("udp"),
                    dst_port: Some("546".into()),
                    src_address: "fe80::/10".parse().ok(),
                    ..Rule::new(chain, "dhcpv6 client", Action::Accept)
                });
            }
            // each new connection puts its source a stage further, the last one on the
            // blacklist: the stages from the end, so one connection takes one step
            for (index, list) in SSH_STAGES.iter().enumerate().rev() {
                let time = if index + 1 == SSH_STAGES.len() { blacklist } else { stage };
                rules.push(Rule {
                    protocol: Some("tcp"),
                    dst_port: Some("22".into()),
                    connection_state: Some("new"),
                    src_address_list: index.checked_sub(1).map(|before| SSH_STAGES[before]),
                    // the list without `ssh_`: RouterOS's script escapes `_` in a comment
                    ..Rule::new(
                        chain,
                        &format!("ssh {}", list.trim_start_matches("ssh_")),
                        Action::AddSource(list, time),
                    )
                });
            }
            rules.push(Rule {
                protocol: Some("tcp"),
                dst_port: Some("22".into()),
                ..Rule::new(chain, "ssh", Action::Accept)
            });
        }
        if chain == "forward" && family == Family::V4 {
            // what a port forwarding lets in
            rules.push(Rule {
                connection_nat_state: Some("dstnat"),
                ..Rule::new(chain, "dstnat", Action::Accept)
            });
        }
        rules.push(Rule {
            last: true,
            ..Rule::new(chain, "drop", Action::Drop)
        });
    }
    rules
}

/// The IPv6 rules opening `port` of `address` from the internet (a service with `forward`),
/// each once.
fn openings(openings: &BTreeSet<(Ipv6Addr, ServicePort)>) -> Vec<Rule> {
    openings
        .iter()
        .map(|(address, port)| Rule {
            protocol: Some(port.protocol.name()),
            dst_port: Some(port.port.to_string()),
            dst_address: Some(IpNet::from(std::net::IpAddr::V6(*address))),
            ..Rule::new("forward", &format!("{address} {port}"), Action::Accept)
        })
        .collect()
}

/// The port forwarding of `forward`: the same port outside as inside (Z3).
fn dstnat(forward: &PortForward) -> IpFirewallNatCfg {
    IpFirewallNatCfg {
        action: IpFirewallNatAction::DstNat,
        chain: b"dstnat".into(),
        comment: Some(firewall_comment(&["dstnat", &forward.address.to_string(), &forward.port.to_string()])),
        disabled: false,
        dst_address: None,
        dst_port: Some(forward.port.port.to_string().as_str().into()),
        in_interface: None,
        in_interface_list: Some(UPLINK_LIST.as_slice().into()),
        jump_target: None,
        out_interface: None,
        out_interface_list: None,
        protocol: Some(forward.port.protocol.name().into()),
        src_address: None,
        to_addresses: Some(forward.address.to_string().as_str().into()),
        to_ports: None,
    }
}

/// The rules letting the provisioner in from `networks` (its `api_networks`), each of
/// `family`'s.
fn provisioner(family: Family, networks: &[IpNet]) -> Vec<Rule> {
    networks
        .iter()
        .filter(|net| matches!((family, net), (Family::V4, IpNet::V4(_)) | (Family::V6, IpNet::V6(_))))
        .map(|net| Rule {
            src_address: Some(*net),
            ..Rule::new("input", &format!("provisioner {net}"), Action::Accept)
        })
        .collect()
}

impl BaseDeviceDataTarget {
    /// The perimeter firewall, where the device has an interface to the internet (the list
    /// `uplink`, `add_to_uplinks`).
    pub(super) fn setup_perimeter(&mut self, device: &DeviceAccess) {
        if !self.has_uplinks() {
            return;
        }
        let (stage, blacklist) = (device.ssh_stage_time(), device.ssh_blacklist_time());
        let forwards = device.topology().forwards();
        self.ip_firewall_filter
            .extend(perimeter(Family::V4, stage, blacklist).iter().map(Rule::v4));
        let mut v6 = perimeter(Family::V6, stage, blacklist);
        let open = forwards
            .openings
            .iter()
            .map(|opening| (opening.address, opening.port))
            .collect();
        let drop = v6.iter().rposition(|rule| rule.last).unwrap_or(v6.len());
        v6.splice(drop..drop, openings(&open));
        self.ipv_6_firewall_filter.extend(v6.iter().map(Rule::v6));
        if let Some(on_device) = forwards.dstnat.get(&device.id()) {
            self.ip_firewall_nat.extend(on_device.iter().map(dstnat));
        }
    }
    /// Lets the provisioner in from `networks` (`api_networks`) through the perimeter, before
    /// the SSH stages: its uploads and logins never count there. Without them it reaches a
    /// device from the internet only by SSH, and an apply would cut its API off, which the
    /// fallback takes back.
    pub fn allow_provisioner(&mut self, networks: &[IpNet]) {
        if !self.has_uplinks() {
            return;
        }
        let invalid = Some(Rule::new("input", "invalid", Action::Drop).comment());
        if let Some(at) = self.ip_firewall_filter.iter().position(|rule| rule.comment == invalid) {
            let rules = provisioner(Family::V4, networks);
            self.ip_firewall_filter.splice(at + 1..at + 1, rules.iter().map(Rule::v4));
        }
        if let Some(at) = self.ipv_6_firewall_filter.iter().position(|rule| rule.comment == invalid) {
            let rules = provisioner(Family::V6, networks);
            self.ipv_6_firewall_filter.splice(at + 1..at + 1, rules.iter().map(Rule::v6));
        }
    }
    fn has_uplinks(&self) -> bool {
        self.interface_list_member
            .keys()
            .any(|(_, list)| *list.0 == *UPLINK_LIST)
    }
}
