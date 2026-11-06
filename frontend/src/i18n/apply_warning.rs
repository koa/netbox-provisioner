use crate::graphql::authenticated::types::ApplyWarning as Warning;
use ::i18n::device::warning as w;

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
        Warning::Unknown => w::UNKNOWN.into(),
    }
}
