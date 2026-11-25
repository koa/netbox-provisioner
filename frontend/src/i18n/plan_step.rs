//! Scope: The words of a step of a NetBox plan (`PlanStep`); nothing else.
use crate::graphql::authenticated::{
    helper::InterfaceTag,
    netbox_plan::{LocationRef, PlanStep as Step},
    types::{InterfaceRef, IpVersion, PoeMode, RadioRole, VlanRef},
};
use ::i18n::helper as t;

/// The step in one line, as the preview lists it.
pub fn describe(step: &Step) -> String {
    match step {
        Step::CreateDevice(create) => t::create_device(
            &create.name,
            &create.device_type,
            &create.role,
            &create.site,
            create.wlan_group.as_deref(),
        ),
        Step::CreateInterface(create) => t::create_interface(
            &create.device.name,
            &create.name,
            &create.kind,
            &create
                .tags
                .iter()
                .map(InterfaceTag::slug)
                .collect::<Vec<_>>(),
            create.mac.as_deref(),
        ),
        Step::CreateAddress(create) => t::create_address(
            &create.address,
            &create.device.name,
            &create.interface,
            &create.prefix,
        ),
        Step::CreateCable(create) => t::create_cable(port(&create.a), port(&create.b)),
        Step::CreatePrefix(create) => t::create_prefix(
            &create.prefix,
            &create.pool,
            &create.description,
            create.dhcp_lease_time,
        ),
        Step::CreateIpRange(create) => {
            t::create_ip_range(&create.start, &create.end, &create.description)
        }
        Step::UpdateInterface(update) => t::update_interface(
            port(&update.interface),
            update.label.as_deref().map(|label| t::Change {
                to: label,
                before: update.label_before.as_deref(),
            }),
            update.mtu.map(|mtu| t::Change {
                to: mtu,
                before: update.mtu_before,
            }),
            &update
                .tags_added
                .iter()
                .map(InterfaceTag::slug)
                .collect::<Vec<_>>(),
            &update
                .tags_removed
                .iter()
                .map(InterfaceTag::slug)
                .collect::<Vec<_>>(),
            match (&update.poe, update.poe_removed) {
                (Some(poe), _) => Some(Some(plan_poe_mode(poe))),
                (None, true) => Some(None),
                (None, false) => None,
            }
            .map(|to| t::Change {
                to,
                before: Some(update.poe_before.as_ref().map(plan_poe_mode)),
            }),
        ),
        Step::DeleteCable(delete) => {
            t::delete_cable(port(&delete.port), delete.far.as_ref().map(port))
        }
        Step::DeleteAddress(delete) => t::delete_address(
            &delete.address,
            &delete.interface.device.name,
            &delete.interface.name,
        ),
        Step::DeletePrefix(delete) => t::delete_prefix(&delete.prefix),
        Step::UpdatePort(update) => t::update_port(
            port(&update.interface),
            update.name.as_deref(),
            update.rf_role.as_ref().map(|role| t::Change {
                to: rf_role(role),
                before: update.rf_role_before.as_ref().map(rf_role),
            }),
            update.bridge.as_deref(),
            update.leaves_bridge.as_deref(),
        ),
        Step::CreateWirelessLink(create) => {
            t::create_wireless_link(port(&create.a), port(&create.b), &create.ssid)
        }
        Step::UpdatePortVlan(update) => {
            fn vlan(vlan: &VlanRef) -> t::Vlan<'_> {
                t::Vlan {
                    vid: vlan.vid,
                    name: &vlan.name,
                }
            }
            t::update_port_vlan(
                port(&update.interface),
                update.vlan.as_ref().map(vlan),
                update.vlan_before.as_ref().map(vlan),
                &update.tagged_before.iter().map(vlan).collect::<Vec<_>>(),
            )
        }
        Step::DeleteInterface(delete) => t::delete_interface(port(&delete.interface)),
        Step::DeleteWirelessLink(delete) => {
            t::delete_wireless_link(port(&delete.a), port(&delete.b))
        }
        Step::UpdateWirelessLan(update) => t::update_wireless_lan(
            &update.ssid_before,
            update.ssid.as_deref(),
            update.psk_set,
            update.enabled,
        ),
        Step::UpdateWirelessLink(update) => t::update_wireless_link(
            port(&update.a),
            port(&update.b),
            update.ssid.as_deref().map(|ssid| t::Change {
                to: ssid,
                before: Some(&update.ssid_before),
            }),
            update.psk_set,
        ),
        Step::SetPrimaryIp(set) => t::set_primary(
            match set.version {
                IpVersion::V4 => "IPv4",
                IpVersion::V6 => "IPv6",
                IpVersion::Other => "IP",
            },
            &set.device.name,
            &set.address,
            set.before.as_deref(),
        ),
        Step::UpdateDeviceLocation(update) => {
            let path = |location: &LocationRef| location.path.join(" > ");
            t::update_device_location(
                &update.device.name,
                update.location.as_ref().map(path).as_deref(),
                update.location_before.as_ref().map(path).as_deref(),
            )
        }
        Step::Unknown => t::UNKNOWN_STEP.into(),
    }
}

fn port(interface: &InterfaceRef) -> t::Port<'_> {
    t::Port {
        device: &interface.device.name,
        name: &interface.name,
    }
}

fn rf_role(role: &RadioRole) -> &str {
    match role {
        RadioRole::Ap => "AP",
        RadioRole::Station => "Station",
        RadioRole::Other(other) => other,
    }
}

fn plan_poe_mode(mode: &PoeMode) -> &str {
    match mode {
        PoeMode::Pse => "PSE",
        PoeMode::Pd => "PD",
        PoeMode::Other(other) => other,
    }
}
