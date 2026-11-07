//! Scope: The base config's step for OSPF; nothing else.
//!
//! OSPF on the interfaces tagged `ospf`, v2 and v3.
use super::BaseDeviceDataTarget;
use crate::{
    device::ros::l2::{L2Setup, NameGenerator},
    topology::access::device::DeviceAccess,
};
use mikrotik_model::model::{
    RoutingOriginateDefault, RoutingOspfInstanceByName, RoutingOspfInstanceCfg,
    RoutingOspfInstanceVersion, RoutingRedistribute,
};
use std::collections::BTreeSet;

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
                    .entry((b"backbone-v2".into(),))
                    .or_default();
                v2_backbone.interfaces = ports.clone();
                v2_backbone.use_bfd = Some(false);
                if bridge_device {
                    v2_backbone.priority = 0;
                }
                let v3_backbone = self
                    .ospf_interface
                    .entry((b"backbone-v3".into(),))
                    .or_default();
                v3_backbone.interfaces = ports;
                v3_backbone.use_bfd = Some(false);
                if bridge_device {
                    v3_backbone.priority = 0;
                }
            }
        }
    }
}
