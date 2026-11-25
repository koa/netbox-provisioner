//! Scope: The words of a WLAN client's band (`WlanBand`); nothing else.
use crate::graphql::resident::live::{WlanBand, WlanFrequency, WlanStandard};
use ::i18n::resident as t;

/// „5 GHz (ax)“; empty for a band of a newer server.
pub fn band(band: &WlanBand) -> String {
    let ghz = match band.frequency {
        WlanFrequency::Ghz2 => "2,4",
        WlanFrequency::Ghz5 => "5",
        WlanFrequency::Ghz6 => "6",
        WlanFrequency::Other => return String::new(),
    };
    let standard = band.standard.and_then(|standard| match standard {
        WlanStandard::A => Some("a"),
        WlanStandard::B => Some("b"),
        WlanStandard::G => Some("g"),
        WlanStandard::N => Some("n"),
        WlanStandard::Ac => Some("ac"),
        WlanStandard::Ax => Some("ax"),
        WlanStandard::Be => Some("be"),
        WlanStandard::Other => None,
    });
    t::wlan_band(ghz, standard)
}
