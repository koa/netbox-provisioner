//! Scope: The words of an apply's warnings; nothing else.
use crate::graphql::authenticated::types::{ApplyWarning as Warning, OspfAuthDoubt as Doubt};
use ::i18n::device::warning::{self as w, OspfDoubt};

/// A warning of `generateCfg`, as the operator reads it before applying (also on the device
/// page).
pub fn describe(warning: &Warning) -> String {
    match warning {
        Warning::PoeCut(cut) => w::poe_cut(&cut.port, &cut.default_name),
        Warning::CapOffOutsideWlanGroup(cap) => w::cap_off_outside_wlan_group(&cap.package),
        Warning::WatchTargetSilent(silent) => {
            w::watch_target_silent(&silent.address, silent.port.as_deref(), &silent.device)
        }
        Warning::ApiClientsOutside(outside) => w::api_clients_outside(&outside.clients),
        Warning::OspfAuthKept(kept) => {
            let doubt = match &kept.doubt {
                Doubt::OspfEndsOutside(outside) => OspfDoubt::EndsOutside(&outside.devices),
                Doubt::OspfEndNotOspf(end) => OspfDoubt::EndNotOspf {
                    address: &end.address,
                    device: end.device.as_deref(),
                },
                Doubt::OspfForeignNeighbor(foreign) => OspfDoubt::ForeignNeighbor {
                    device: &foreign.device,
                    router_id: &foreign.router_id,
                },
                Doubt::OspfNeighborMissing(missing) => OspfDoubt::NeighborMissing {
                    device: &missing.device,
                    missing: &missing.missing,
                },
                Doubt::OspfCredentialsDiffer(differ) => {
                    OspfDoubt::CredentialsDiffer(&differ.devices)
                }
                Doubt::Unknown => OspfDoubt::Unknown,
            };
            w::ospf_auth_kept(&kept.port, &kept.network, doubt)
        }
        Warning::Unknown => w::UNKNOWN.into(),
    }
}
