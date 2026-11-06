//! The device's addresses: from NetBox on its L3 interfaces and loopback, the DHCP clients,
//! and the hand-made ones that stay.
use super::{
    BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError,
    dhcp::{dhcp_pool_v4, pool_range, static_leases},
};
use crate::{
    device::ros::l2::{L2Setup, NameGenerator},
    topology::access::AccessTopology,
};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        IpAddressByAddress, IpAddressCfg, IpDhcpClientCfg, IpDhcpClientDhcpOptions,
        IpDhcpServerLeaseCfg, Ipv6AddressByAddress, Ipv6AddressCfg, Ipv6DhcpClientByInterface,
        Ipv6DhcpClientCfg, Ipv6DhcpClientRequest, YesNo,
    },
    value,
};
use std::{collections::HashSet, net::IpAddr, time::Duration};

impl BaseDeviceDataTarget {
    /// Keeps the addresses the device has on an interface the provisioner doesn't manage: a
    /// tunnel (`gre`, `ipip`, …), a PPPoE client or any other kind set up by hand, as BGP is.
    /// Managed are the Ethernet ports, bridges, VLANs, VXLANs, the radios (`radios`) and `lo`;
    /// an address on one of them, or on a removed interface (`*35`), goes with NetBox.
    pub(super) fn keep_hand_made_addresses(
        &mut self,
        current: &BaseDeviceDataCurrent,
        radios: &[AsciiString],
    ) {
        let managed = current
            .ethernet
            .iter()
            .map(|port| &port.data.name)
            .chain(self.ethernet.values().map(|port| &port.name))
            .chain(current.bridge.iter().map(|bridge| &bridge.0.name))
            .chain(current.vlan.iter().map(|vlan| &vlan.0.name))
            .chain(current.vxlan.iter().map(|vxlan| &vxlan.0.name))
            .chain(radios)
            .map(|name| name.0.as_ref())
            .collect::<HashSet<&[u8]>>();
        let hand_made = |interface: &AsciiString| {
            let name: &[u8] = interface.0.as_ref();
            !name.is_empty() && !name.starts_with(b"*") && name != b"lo" && !managed.contains(name)
        };
        for address in current
            .ipv_4_address
            .iter()
            .filter(|a| hand_made(&a.0.interface))
        {
            self.ipv_4_address
                .entry(address.0.address)
                .or_insert_with(|| address.clone());
        }
        for address in current
            .ipv_6_address
            .iter()
            .filter(|a| hand_made(&a.0.interface))
        {
            self.ipv_6_address
                .entry(address.0.address)
                .or_insert_with(|| address.clone());
        }
    }

    pub(super) fn set_ip_address(&mut self, ip: IpNet, if_name: impl Into<AsciiString>) {
        match ip {
            IpNet::V4(v4_net) => {
                self.ipv_4_address
                    .entry(v4_net)
                    .or_insert(IpAddressByAddress(IpAddressCfg {
                        address: Default::default(),
                        interface: Default::default(),
                        comment: None,
                    }))
                    .0
                    .interface = if_name.into();
            }
            IpNet::V6(v6_net) => {
                self.ipv_6_address
                    .entry(v6_net)
                    .or_insert(Ipv6AddressByAddress(Ipv6AddressCfg {
                        address: Default::default(),
                        advertise: false,
                        // RouterOS only accepts `auto-link-local=no` on a link-local
                        // address (it then replaces the generated one) and rejects it
                        // on any other with "selected address is not link local".
                        auto_link_local: !v6_net.addr().is_unicast_link_local(),
                        comment: None,
                        disabled: false,
                        eui_64: false,
                        from_pool: None,
                        interface: Default::default(),
                        no_dad: false,
                    }))
                    .0
                    .interface = if_name.into();
            }
        }
    }

    pub(super) fn set_loopback_ip(&mut self, loopback_ip: IpAddr) {
        match loopback_ip {
            IpAddr::V4(loopback_ip) => {
                self.ipv_4_address.insert(
                    loopback_ip.into(),
                    IpAddressByAddress(IpAddressCfg {
                        interface: b"lo".into(),
                        ..IpAddressCfg::default()
                    }),
                );
            }
            IpAddr::V6(loopback_ip) => {
                self.ipv_6_address.insert(
                    loopback_ip.into(),
                    Ipv6AddressByAddress(Ipv6AddressCfg {
                        interface: b"lo".into(),
                        ..Ipv6AddressCfg::default()
                    }),
                );
            }
        }
    }

    pub(super) fn setup_ip_addresses<N: NameGenerator>(
        &mut self,
        l2: &L2Setup<N>,
    ) -> Result<(), SetupError> {
        // A device NetBox gives no address at all and no `dhcp-client` tag is pure L2
        // (a radio bridge): it is only reachable through an address from DHCP, so its
        // bridges get a client - under whatever name the bridge is generated with.
        let pure_l2 = l2
            .logical_interfaces()
            .all(|logical| logical.addresses.is_empty() && !logical.enable_dhcp_client);
        if pure_l2 {
            for bridge in &l2.simple_bridges {
                self.enable_dhcp_client(bridge.name.clone(), false);
            }
        }
        for logical in l2.logical_interfaces() {
            let if_name = logical.if_name.clone();
            let ips = logical
                .addresses
                .iter()
                .filter_map(|ip| ip.net().map(|a| (a, ip.clone())))
                .collect::<Vec<_>>();
            if logical.nat {
                self.masquerade(&if_name);
            }
            // `nat` needs `internet` (finding `NatWithoutInternet`): guarded all the same
            if logical.nat || logical.internet {
                self.add_to_uplinks(&if_name);
            }
            if ips.is_empty() {
                if logical.enable_dhcp_client {
                    // the internet uplink: its provider delegates an IPv6 prefix
                    if logical.nat {
                        self.enable_dhcp_v6_client(if_name.clone(), logical.use_ospf);
                    }
                    self.enable_dhcp_client(if_name, logical.use_ospf);
                }
            } else {
                let dhcp_server = logical.enable_dhcp_server;
                // RouterOS takes one DHCP server per interface ("server or relay with such
                // interface already exists", CHR 7.24.5): the first network with addresses to
                // hand out creates it, a further one adds its ranges to its pool and a network
                // entry of its own
                let mut port_server: Option<AsciiString> = None;
                // the shortest lease time of the port's networks: one server has one
                let mut port_lease_time: Option<Duration> = None;
                for (ip_idx, (ip, ip_address)) in ips.iter().enumerate() {
                    self.set_ip_address(*ip, if_name.clone());
                    // a client network (`dhcp`) gets its IPv6 /64 announced for SLAAC
                    if let (true, IpNet::V6(v6_net)) = (dhcp_server, ip)
                        && v6_net.prefix_len() == 64
                        && let Some(address) = self.ipv_6_address.get_mut(v6_net)
                    {
                        address.0.advertise = true;
                    }
                    // the pool comes from the IPv4 prefix; IPv6 clients take SLAAC
                    if let (true, IpNet::V4(v4)) = (dhcp_server, ip) {
                        let Some(prefix) = ip_address.prefix() else {
                            // a /32 (a loopback) has no address for a client: no DHCP server,
                            // as with an empty pool (finding `DhcpPoolEmpty`)
                            if v4.prefix_len() == 32 {
                                continue;
                            }
                            return Err(SetupError::MissingPrefixOnIpAddress {
                                ip: ip.trunc(),
                                interface: if_name.clone(),
                            });
                        };
                        let IpNet::V4(net) =
                            prefix.prefix().ok_or(SetupError::MissingAddressOnPrefix {
                                prefix: prefix.id(),
                            })?
                        else {
                            continue;
                        };
                        let dhcp_ranges = dhcp_pool_v4(&prefix, net);
                        if dhcp_ranges.is_empty() {
                            continue;
                        }
                        let server_name: AsciiString = match &port_server {
                            Some(name) => name.clone(),
                            None => {
                                // numbered among the IPv4 addresses only: an IPv6 address
                                // before it in NetBox (one created again first by an undo)
                                // doesn't rename the server
                                let v4_idx = ips[..ip_idx]
                                    .iter()
                                    .filter(|(ip, _)| matches!(ip, IpNet::V4(_)))
                                    .count();
                                let suffix = if v4_idx == 0 {
                                    if_name.to_string()
                                } else {
                                    format!("{if_name}-{v4_idx}")
                                };
                                let name: AsciiString = format!("dhcp-{suffix}").into();
                                port_server = Some(name.clone());
                                name
                            }
                        };
                        let server = &mut self
                            .dhcp_v_4_server
                            .entry(server_name.clone())
                            .or_default()
                            .0;
                        server.interface = if_name.clone();
                        server.address_pool = server_name.clone();
                        if let Some(lease_time) = prefix.dhcp_lease_time() {
                            let shortest =
                                port_lease_time.map_or(lease_time, |t| t.min(lease_time));
                            port_lease_time = Some(shortest);
                            server.lease_time = Some(shortest);
                        }
                        let network = &mut self.dhcp_v_4_server_network.entry(net).or_default().0;
                        network.gateway.insert(v4.addr());
                        if !network.dns_server.contains(&v4.addr()) {
                            network.dns_server = network
                                .dns_server
                                .iter()
                                .copied()
                                .chain([v4.addr()])
                                .collect();
                        }
                        for lease in static_leases(&prefix) {
                            self.dhcp_v_4_lease.insert(
                                (IpAddr::V4(lease.address),),
                                IpDhcpServerLeaseCfg {
                                    address: IpAddr::V4(lease.address),
                                    mac_address: Some(lease.mac),
                                    server: Some(server_name.clone()),
                                    comment: Some(lease.name.as_bytes().into()),
                                    // the rest RouterOS's own: a lease taken over has only
                                    // its client, name, MAC and address in NetBox
                                    address_lists: Default::default(),
                                    allow_dual_stack_queue: false,
                                    always_broadcast: false,
                                    block_access: false,
                                    client_id: None,
                                    dhcp_option: None,
                                    dhcp_option_set: None,
                                    disabled: false,
                                    insert_queue_before: None,
                                    lease_time: Duration::ZERO,
                                    parent_queue: value::HasNone::NoneValue,
                                    queue_type: None,
                                    rate_limit: None,
                                    routes: None,
                                    use_src_mac: false,
                                },
                            );
                        }
                        self.ipv_4_pool
                            .entry(server_name)
                            .or_default()
                            .0
                            .ranges
                            .extend(dhcp_ranges.iter().map(|r| pool_range(r).into()));
                    }
                }
            }
        }
        Ok(())
    }

    /// `use_ospf`: the default route comes over OSPF then; one from DHCP would be
    /// redistributed to the whole network, although it only works from this device.
    pub(super) fn enable_dhcp_client(&mut self, if_name: AsciiString, use_ospf: bool) {
        self.dhcp_v_4_client
            .entry((if_name,))
            .or_insert(IpDhcpClientCfg {
                interface: Default::default(),
                add_default_route: if use_ospf { YesNo::No } else { YesNo::Yes },
                comment: None,
                // RouterOS reports the distance only with a default route, can't clear it and
                // reads the hidden one as its default 1 (mikrotik-model `default=1`)
                default_route_distance: if use_ospf { 1 } else { 10 },
                // RouterOS's default: without the client ID the DHCP server may see a new
                // client and hand out a different address, cutting off the management path
                dhcp_options: [
                    IpDhcpClientDhcpOptions::Hostname,
                    IpDhcpClientDhcpOptions::Clientid,
                ]
                .into(),
                script: None,
                use_peer_dns: true,
                use_peer_ntp: false,
            });
    }

    /// The IPv6 client of an internet uplink: an address and a delegated prefix, which goes
    /// into the pool `pd-<interface>` for now (no network takes from it), and like the IPv4
    /// client a default route unless OSPF brings one. `keep_dhcp_v6_clients` merges a client the
    /// device has there already.
    pub(super) fn enable_dhcp_v6_client(&mut self, if_name: AsciiString, use_ospf: bool) {
        let pool = format!("pd-{if_name}");
        self.dhcp_v_6_client.insert(
            if_name.clone(),
            Ipv6DhcpClientByInterface(Ipv6DhcpClientCfg {
                interface: if_name,
                request: [
                    Ipv6DhcpClientRequest::Address,
                    Ipv6DhcpClientRequest::Prefix,
                ]
                .into(),
                accept_prefix_without_address: true,
                add_default_route: !use_ospf,
                // RouterOS reports the distance only with a default route
                default_route_distance: (!use_ospf).then_some(10),
                default_route_tables: Some(b"default".into()),
                allow_reconfigure: false,
                check_gateway: Some(b"none".into()),
                comment: None,
                dhcp_options: None,
                disabled: false,
                pool_name: Some(pool.as_str().into()),
                pool_prefix_length: Some(64),
                prefix_address_lists: None,
                prefix_hint: Some(b"::/0".into()),
                script: None,
                use_peer_dns: true,
                validate_server_duid: true,
            }),
        );
    }

    /// The IPv6 clients of `current` (as after the renames): one where the target has its own
    /// keeps what the device set by hand but the prefix delegation, the default route and an
    /// existing pool's name, which other settings may refer to; any other stays as it is.
    pub(super) fn keep_dhcp_v6_clients(&mut self, current: &BaseDeviceDataCurrent) {
        for client in current.dhcp_v_6_client.iter() {
            match self.dhcp_v_6_client.get_mut(&client.0.interface) {
                Some(own) => {
                    let target = &own.0;
                    own.0 = Ipv6DhcpClientCfg {
                        request: target.request.clone(),
                        add_default_route: target.add_default_route,
                        default_route_distance: target.default_route_distance,
                        pool_name: client
                            .0
                            .pool_name
                            .clone()
                            .or_else(|| target.pool_name.clone()),
                        pool_prefix_length: client
                            .0
                            .pool_prefix_length
                            .or(target.pool_prefix_length),
                        disabled: false,
                        ..client.0.clone()
                    };
                }
                None => {
                    self.dhcp_v_6_client
                        .insert(client.0.interface.clone(), client.clone());
                }
            }
        }
    }
}
