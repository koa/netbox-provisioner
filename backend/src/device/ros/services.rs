//! Scope: What the device offers and asks: `/ip/service`, the management proxy's SOCKS and DNS; not
//! the firewall in front (`firewall`).
//!
//! What the device offers and asks: `/ip/service`, the management proxy's `/ip socks` and
//! DNS.
use super::{BaseDeviceDataTarget, firewall, firewall_comment};
use crate::{
    device::ros::warning::{ApiClientsOutside, ApplyWarning},
    topology::access::device::DeviceAccess,
};
use ipnet::IpNet;
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        IpServiceByName, IpServiceCfg, IpSocksAccessAction, IpSocksAccessCfg, IpSocksAuthMethod,
        IpSocksUsersByName, IpSocksUsersCfg, Ipv6NdByInterface, Ipv6NdCfg,
    },
    resource::OWNED_LAST,
};
use std::{
    collections::{BTreeSet, HashSet},
    net::IpAddr,
};

impl BaseDeviceDataTarget {
    /// Makes this device the provisioner's way into the network: `/ip socks` as SOCKS5, only
    /// for this user, since the proxy is reachable from wherever the device is, and with
    /// `api_networks` only from there (A3): an allow for each, then a deny for every other
    /// source, after the rules set up by hand (`OWNED_LAST`). RouterOS lets in a source no
    /// rule names, and judges the rules at the CONNECT, not at the greeting (CHR 7.24.5).
    /// Without `api_networks`, the proxy takes every source, as the API does.
    pub fn setup_management_proxy(&mut self, user: &str, password: &str, api_networks: &[IpNet]) {
        if !api_networks.is_empty() {
            let rule = |action, src_address: Option<&IpNet>, comment: &[&str]| IpSocksAccessCfg {
                action,
                comment: Some(firewall_comment(comment)),
                disabled: false,
                dst_address: None,
                dst_port: None,
                src_address: src_address.map(|net| firewall::ros_address(net.trunc())),
                src_port: None,
            };
            let last = String::from_utf8_lossy(OWNED_LAST);
            self.socks_access = api_networks
                .iter()
                .map(|net| {
                    rule(
                        IpSocksAccessAction::Allow,
                        Some(net),
                        &["provisioner", &net.trunc().to_string()],
                    )
                })
                .chain([rule(
                    IpSocksAccessAction::Deny,
                    None,
                    &[last.trim_end(), "rest"],
                )])
                .collect();
        }
        self.socks.enabled = true;
        self.socks.version = 5;
        self.socks.auth_method = IpSocksAuthMethod::Password;
        self.socks_users.insert(
            user.into(),
            IpSocksUsersByName(IpSocksUsersCfg {
                name: user.into(),
                password: password.into(),
                only_one: false,
                disabled: false,
            }),
        );
    }

    /// `/ip/service`: RouterOS's services as the device has them (`current`), with telnet and ftp
    /// off (A2), www (WebFig) on and, with `api_networks`, the plain API and www only from those. Written as
    /// `address`, which RouterOS 7.24 still takes for `available-from`, unless the device
    /// reports `available-from` already: an open service reports neither.
    pub fn setup_services(&mut self, current: &[IpServiceByName], api_networks: Option<&[IpNet]>) {
        let api_networks = network_list(api_networks);
        self.ip_service = current
            .iter()
            .map(|service| (service.name.clone(), service.data.clone()))
            .collect();
        for (name, service) in &mut self.ip_service {
            let restrict = |service: &mut IpServiceCfg| {
                if let Some(networks) = &api_networks {
                    if service.available_from.is_some() {
                        service.available_from = Some(networks.clone());
                    } else {
                        service.address = Some(networks.clone());
                    }
                }
            };
            match &**name {
                b"telnet" | b"ftp" => service.disabled = true,
                // WebFig over plain HTTP: what's left in the field where Winbox isn't at hand,
                // so on, from the same networks as the API, until the device has a certificate
                // for www-ssl (`use_www_ssl`)
                b"www" => {
                    service.disabled = false;
                    restrict(service);
                }
                b"api" => restrict(service),
                _ => {}
            }
        }
    }

    /// WebFig over HTTPS only (A7), once the device has a certificate of the CA attached to
    /// `www-ssl` (`device::www_certificate`): `www-ssl` on, from `api_networks` like `www` was,
    /// and `www` off. The certificate itself isn't in the model, the diff leaves it.
    pub fn use_www_ssl(&mut self, current: &[IpServiceByName], api_networks: Option<&[IpNet]>) {
        let api_networks = network_list(api_networks);
        let www_ssl = AsciiString::from("www-ssl");
        if let Some(current) = current.iter().find(|service| service.name == www_ssl) {
            let mut service = current.data.clone();
            service.disabled = false;
            if let Some(networks) = api_networks {
                if service.available_from.is_some() {
                    service.available_from = Some(networks);
                } else {
                    service.address = Some(networks);
                }
            }
            self.ip_service.insert(www_ssl, service);
        }
        if let Some(www) = self.ip_service.get_mut(&AsciiString::from("www")) {
            www.disabled = true;
        }
    }

    /// The API restriction of [`Self::setup_services`] only where it locks none of `clients`
    /// (the API's open connections) out: otherwise the API stays as in `current`, with a
    /// warning naming those outside `networks`.
    pub fn keep_api_open_for(
        &mut self,
        current: &[IpServiceByName],
        clients: &[IpAddr],
        networks: &[IpNet],
    ) -> Option<ApplyWarning> {
        let outside = clients
            .iter()
            .filter(|client| !networks.iter().any(|net| net.contains(*client)))
            .map(|client| client.to_string().into())
            .collect::<Vec<Box<str>>>();
        if outside.is_empty() {
            return None;
        }
        let name = AsciiString::from("api");
        if let (Some(target), Some(current)) = (
            self.ip_service.get_mut(&name),
            current.iter().find(|service| service.name == name),
        ) {
            *target = current.data.clone();
        }
        Some(ApplyWarning::ApiClientsOutside(ApiClientsOutside {
            clients: outside.into(),
        }))
    }

    /// DNS of the client networks. With DNS servers from the config context they get these:
    /// the IPv4 ones by DHCP, the IPv6 ones by router advertisement (RDNSS,
    /// `setup_router_advertisements`), in their order; the device asks them too and resolves
    /// for nobody. One of them its own (its loopback, router-faesigrund-keller) makes it a
    /// resolver: it asks the others, not itself, and answers its networks. Without, the DHCP networks name the device itself,
    /// so it answers them; one without a DHCP client learns no DNS server and asks the NAT
    /// routers (NetBox tag `nat`). The perimeter firewall keeps the internet out of it.
    pub(super) fn setup_dns(&mut self, device: &DeviceAccess) {
        let servers = device.dns_servers();
        if !servers.is_empty() {
            let v4 = servers
                .iter()
                .filter_map(|ip| match ip {
                    IpAddr::V4(ip) => Some(*ip),
                    IpAddr::V6(_) => None,
                })
                .collect::<Box<[_]>>();
            for network in self.dhcp_v_4_server_network.values_mut() {
                network.0.dns_server = v4.clone();
            }
            let own = device
                .interfaces()
                .iter()
                .flat_map(|interface| interface.ips())
                .filter_map(|ip| ip.addr())
                .chain(device.loopback_ip().and_then(|ip| ip.addr()))
                .collect::<HashSet<_>>();
            let resolver = servers.iter().any(|ip| own.contains(ip));
            self.dns.servers = servers
                .iter()
                .filter(|ip| !own.contains(ip))
                .copied()
                .collect();
            self.dns.allow_remote_requests = resolver;
            return;
        }
        self.dns.allow_remote_requests = !self.dhcp_v_4_server_network.is_empty();
        if !self.dns.allow_remote_requests {
            return;
        }
        let has_uplink = device
            .interfaces()
            .iter()
            .any(|interface| interface.is_enable_dhcp_client());
        if !has_uplink {
            self.dns.servers = device
                .topology()
                .list_devices()
                .filter(|other| other.id() != device.id())
                .filter(|other| other.interfaces().iter().any(|i| i.is_nat()))
                .filter_map(|other| other.loopback_ip().and_then(|ip| ip.addr()))
                .collect();
        }
    }
}

impl BaseDeviceDataTarget {
    /// Router advertisements only on the interfaces with a prefix the device announces
    /// (`advertise`, a client network's /64 or one set up by hand), with the IPv6 DNS servers
    /// of the config context (RDNSS). RouterOS's own entry for `all` made every device a
    /// router on every interface: an AP or switch in a WLAN's VXLAN (1og-sw2, ap-klr-solar)
    /// announced itself as a default router to the clients there, where it has no address.
    /// That entry can't be removed (`reuse`: it takes one of the interfaces); without any it
    /// is turned off. Runs again after the hand-made addresses are kept.
    pub(super) fn setup_router_advertisements(&mut self, device: &DeviceAccess) {
        let dns = device
            .dns_servers()
            .iter()
            .filter_map(|ip| match ip {
                IpAddr::V6(ip) => Some(*ip),
                IpAddr::V4(_) => None,
            })
            .collect::<Box<[_]>>();
        let interfaces = self
            .ipv_6_address
            .values()
            .filter(|address| address.0.advertise)
            .map(|address| address.0.interface.clone())
            .collect::<BTreeSet<_>>();
        self.ipv_6_nd.clear();
        for interface in interfaces {
            let mut nd = Ipv6NdCfg::new(interface.clone(), false);
            if !dns.is_empty() {
                nd.dns = Some(dns.clone());
                nd.advertise_dns = Some(true);
            }
            self.ipv_6_nd.insert(interface, Ipv6NdByInterface(nd));
        }
        if self.ipv_6_nd.is_empty() {
            self.ipv_6_nd.insert(
                b"all".into(),
                Ipv6NdByInterface(Ipv6NdCfg::new(b"all".into(), true)),
            );
        }
    }
}

/// `api_networks` as RouterOS takes them in `address`/`available-from`; `None` for none.
fn network_list(api_networks: Option<&[IpNet]>) -> Option<AsciiString> {
    api_networks
        .filter(|networks| !networks.is_empty())
        .map(|networks| {
            let list = networks
                .iter()
                .map(|net| net.trunc().to_string())
                .collect::<Vec<_>>();
            AsciiString::from(list.join(",").as_str())
        })
}
