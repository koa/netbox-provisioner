//! Scope: The base config's step for OSPF; nothing else.
//!
//! OSPF on the interfaces tagged `ospf`, v2 and v3, and where each port's credentials go: the
//! shared template of an area, or one of its own per segment (A10,
//! `docs/concepts/ospf-auth.md`). Which segments get new credentials is decided outside
//! (`device::ospf_auth`); here they are only placed.
use super::{BaseDeviceDataCurrent, BaseDeviceDataTarget};
use crate::{
    device::ros::l2::{L2Setup, NameGenerator},
    topology::access::device::DeviceAccess,
};
use ipnet::{IpNet, Ipv4Net};
use mikrotik_model::ascii::AsciiString;
use mikrotik_model::model::{
    RoutingOriginateDefault, RoutingOspfInstanceByName, RoutingOspfInstanceCfg,
    RoutingOspfInstanceVersion, RoutingOspfInterfaceTemplateAuth, RoutingOspfInterfaceTemplateCfg,
    RoutingRedistribute,
};
use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation};
use std::collections::{BTreeMap, BTreeSet};

/// The areas the shared templates are in, v2 and v3.
pub const OSPF_AREAS: [&[u8]; 2] = [b"backbone-v2", b"backbone-v3"];

/// What the comment of a segment's own template starts with; its IPv4 network follows, which
/// finds the template again whatever its port is named now.
pub const OWN_TEMPLATE: &[u8] = b"netbox-provisioner: ospf ";

/// The credentials of an interface template: `auth`, `auth-id`, `auth-key`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OspfCredentials {
    pub auth: Option<RoutingOspfInterfaceTemplateAuth>,
    pub id: Option<u8>,
    pub key: Option<AsciiString>,
}

impl OspfCredentials {
    pub fn of(template: &RoutingOspfInterfaceTemplateCfg) -> Self {
        Self {
            auth: template.auth,
            id: template.auth_id,
            key: template.auth_key.clone(),
        }
    }
    fn set_on(&self, template: &mut RoutingOspfInterfaceTemplateCfg) {
        template.auth = self.auth;
        template.auth_id = self.id;
        template.auth_key = self.key.clone();
    }
    /// The credentials of the shared template on the device, none without one.
    fn of_shared(template: Option<&RoutingOspfInterfaceTemplateCfg>) -> Self {
        template.map(Self::of).unwrap_or_default()
    }
    /// Whether the template authenticates nothing.
    pub fn is_none(&self) -> bool {
        self.auth.is_none()
    }
}

/// One OSPF port with a segment, and the credentials it has on the device in v2 and v3.
#[derive(Debug, Clone, PartialEq)]
pub struct OspfPort {
    /// RouterOS' name, as `/routing/ospf/neighbor` reports it
    pub port: AsciiString,
    pub network: Ipv4Net,
    pub v2: OspfCredentials,
    pub v3: OspfCredentials,
}

/// The segments an apply moves to new credentials, by IPv4 network: only those whose every end
/// is in the apply (`device::ospf_auth::decide`). Empty, every template keeps its credentials.
pub type OspfSwitch = BTreeMap<Ipv4Net, OspfCredentials>;

/// The segment of each OSPF port: its one IPv4 network. A port with none or several has no
/// segment, and its credentials stay as they are.
pub fn ospf_segments<N: NameGenerator>(l2: &L2Setup<N>) -> BTreeMap<AsciiString, Ipv4Net> {
    l2.logical_interfaces()
        .filter(|port| port.use_ospf)
        .filter_map(|port| {
            let networks = port
                .addresses
                .iter()
                .filter_map(|address| match address.net()? {
                    IpNet::V4(net) => Some(net.trunc()),
                    IpNet::V6(_) => None,
                })
                .collect::<BTreeSet<_>>();
            let mut networks = networks.into_iter();
            match (networks.next(), networks.next()) {
                (Some(network), None) => Some((port.if_name.clone(), network)),
                _ => None,
            }
        })
        .collect()
}

/// `sorted` with every change and removal of an interface template that comes before the last
/// template added moved behind it: a port moving into a template of its own is in one all
/// the time, and its adjacency stays (CHR 7.24.6: added first, then taken out of the shared
/// one, it stayed `Full`; the sort put the shared one first, and it went down at once). Nothing
/// depends on a template's change, so nothing else moves.
pub fn templates_added_first(sorted: Vec<ResourceMutation<'_>>) -> Vec<ResourceMutation<'_>> {
    let template = |m: &ResourceMutation| *m.resource == *b"routing/ospf/interface-template";
    let added =
        |m: &ResourceMutation| template(m) && matches!(m.operation, ResourceMutationOperation::Add);
    let Some(last) = sorted.iter().rposition(added) else {
        return sorted;
    };
    let mut before = Vec::with_capacity(sorted.len());
    let mut later = Vec::new();
    let mut rest = sorted.into_iter();
    for mutation in rest.by_ref().take(last) {
        if template(&mutation) && !added(&mutation) {
            later.push(mutation);
        } else {
            before.push(mutation);
        }
    }
    before.extend(rest.next());
    before.extend(later);
    before.extend(rest);
    before
}

/// The comment of the own template of the segment `network`.
pub fn own_template_comment(network: Ipv4Net) -> AsciiString {
    let mut comment = OWN_TEMPLATE.to_vec();
    comment.extend_from_slice(network.to_string().as_bytes());
    AsciiString::from(comment.as_slice())
}

/// The segment a template is the own one of, by its comment.
pub fn own_template_network(template: &RoutingOspfInterfaceTemplateCfg) -> Option<Ipv4Net> {
    let comment = template.comment.as_ref()?;
    let network = comment.0.strip_prefix(OWN_TEMPLATE)?;
    std::str::from_utf8(network).ok()?.parse().ok()
}

/// The credentials a port has on the device in `area`: its segment's own template, else the
/// shared one it is in (the templates without the provisioner's comment holding it).
pub(super) fn current_credentials(
    current: &BaseDeviceDataCurrent,
    area: &[u8],
    port: &AsciiString,
    network: Ipv4Net,
) -> OspfCredentials {
    let in_area = || {
        current
            .ospf_interface
            .iter()
            .map(|template| &template.data)
            .filter(move |template| *template.area.0 == *area)
    };
    in_area()
        .find(|template| own_template_network(template) == Some(network))
        .or_else(|| {
            in_area()
                .filter(|template| own_template_network(template).is_none())
                .find(|template| template.interfaces.contains(port))
        })
        .map(OspfCredentials::of)
        .unwrap_or_default()
}

/// What OSPF announces besides its interfaces' networks: connected and static routes, and BGP's,
/// which is configured by hand (on klr-sw-3, `examples/` 01 to 03) and reaches the rest of the
/// network only this way. Where there is no BGP it brings nothing. A default route doesn't go out
/// this way (CHR 7.24.5: a static `0.0.0.0/0` with `redistribute=static` is no external LSA), only
/// by `originate-default`.
pub(super) fn ospf_redistribute() -> BTreeSet<RoutingRedistribute> {
    [
        RoutingRedistribute::Connected,
        RoutingRedistribute::Static,
        RoutingRedistribute::Bgp,
    ]
    .into_iter()
    .collect()
}

impl BaseDeviceDataTarget {
    pub(super) fn setup_ospf<N: NameGenerator>(&mut self, device: &DeviceAccess, l2: &L2Setup<N>) {
        if let Some(router_id) = device.primary_ip_v4().and_then(|ip| ip.addr()) {
            let ports = l2
                .logical_interfaces()
                .filter(|p| p.use_ospf)
                .map(|p| p.if_name.clone())
                .collect::<BTreeSet<_>>();
            if !ports.is_empty() {
                // only where NetBox says so: a default route a device has from an uplink to
                // another network would otherwise go out into that network too
                let originate_default = Some(if device.propagates_default_route() {
                    RoutingOriginateDefault::IfInstalled
                } else {
                    RoutingOriginateDefault::Never
                });
                let v2_instance = self
                    .ospf_instance
                    .entry(b"default-v2".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v2_instance.0.redistribute = ospf_redistribute();
                v2_instance.0.router_id = Some(router_id.to_string().into());
                v2_instance.0.originate_default = originate_default;
                v2_instance.0.version = RoutingOspfInstanceVersion::_2;
                let v3_instance = self
                    .ospf_instance
                    .entry(b"default-v3".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v3_instance.0.redistribute = ospf_redistribute();
                v3_instance.0.router_id = Some(router_id.to_string().into());
                v3_instance.0.originate_default = originate_default;
                v3_instance.0.version = RoutingOspfInstanceVersion::_3;
                let v2_area = self.ospf_area.entry(b"backbone-v2".into()).or_default();
                v2_area.0.instance = b"default-v2".into();
                let v3_area = self.ospf_area.entry(b"backbone-v3".into()).or_default();
                v3_area.0.instance = b"default-v3".into();
                // a radio link's bridge device is never DR or BDR of its segment: it shares
                // it with the routers at both ends and forwards nothing between them
                let bridge_device = device.is_bridge_device();
                let v2_backbone = self
                    .ospf_interface
                    .entry((b"backbone-v2".into(), None))
                    .or_default();
                v2_backbone.interfaces = ports.clone();
                v2_backbone.use_bfd = Some(false);
                if bridge_device {
                    v2_backbone.priority = 0;
                }
                let v3_backbone = self
                    .ospf_interface
                    .entry((b"backbone-v3".into(), None))
                    .or_default();
                v3_backbone.interfaces = ports;
                v3_backbone.use_bfd = Some(false);
                if bridge_device {
                    v3_backbone.priority = 0;
                }
            }
        }
    }
    /// Places the credentials of every OSPF port (A10): a port whose segment `switch` names
    /// gets those, any other one the credentials it has on the device (`current`). A port with
    /// credentials is in a template of its own, a copy of its area's shared one; the shared
    /// template keeps what it has on the device, and goes when no port is left in it (with
    /// `interfaces=""` it would stay as a template of no port). Without credentials anywhere
    /// nothing changes.
    pub(crate) fn place_ospf_credentials(
        &mut self,
        current: &BaseDeviceDataCurrent,
        segments: &BTreeMap<AsciiString, Ipv4Net>,
        switch: &OspfSwitch,
    ) {
        for area in OSPF_AREAS {
            let shared_key = (AsciiString::from(area), None);
            // placed before with every port in a template of its own: the shared one went, an
            // own one is the copy of it
            let base = self.ospf_interface.get(&shared_key).or_else(|| {
                self.ospf_interface
                    .iter()
                    .find(|((a, comment), _)| *a.0 == *area && comment.is_some())
                    .map(|(_, own)| own)
            });
            let Some(base) = base else {
                continue;
            };
            let mut shared = base.clone();
            shared.comment = None;
            if !self.ospf_interface.contains_key(&shared_key) {
                shared.interfaces.clear();
            }
            // placed again (a decision after the generate): the own templates' ports back first
            let placed = self
                .ospf_interface
                .keys()
                .filter(|(a, comment)| *a.0 == *area && comment.is_some())
                .cloned()
                .collect::<Vec<_>>();
            for key in placed {
                if let Some(own) = self.ospf_interface.remove(&key) {
                    shared.interfaces.extend(own.interfaces);
                }
            }
            // the shared template on the device: the one holding most of the target's ports
            let shared_now = current
                .ospf_interface
                .iter()
                .map(|template| &template.data)
                .filter(|template| *template.area.0 == *area)
                .filter(|template| own_template_network(template).is_none())
                .max_by_key(|template| {
                    template.interfaces.intersection(&shared.interfaces).count()
                });
            OspfCredentials::of_shared(shared_now).set_on(&mut shared);
            let mut own = Vec::new();
            for port in shared.interfaces.clone() {
                let Some(network) = segments.get(&port).copied() else {
                    continue;
                };
                let credentials = match switch.get(&network) {
                    Some(target) => target.clone(),
                    None => current_credentials(current, area, &port, network),
                };
                if credentials.is_none() || credentials == OspfCredentials::of(&shared) {
                    continue;
                }
                shared.interfaces.remove(&port);
                let mut template = shared.clone();
                template.interfaces = [port].into();
                template.comment = Some(own_template_comment(network));
                credentials.set_on(&mut template);
                own.push(template);
            }
            for template in own {
                self.ospf_interface.insert(
                    (AsciiString::from(area), template.comment.clone()),
                    template,
                );
            }
            if shared.interfaces.is_empty() {
                self.ospf_interface.remove(&shared_key);
            } else {
                self.ospf_interface.insert(shared_key, shared);
            }
        }
    }
}
