//! Scope: One device in one of a resident's networks, wherever it is listed (the devices of a
//! network, the devices behind a port); not reading them (`network_devices`, `components::resident`).
use crate::graphql::resident::live::{NetworkDevice, WlanBand, WlanFrequency, WlanStandard};
use i18n::resident as t;
use yew::{Html, Properties, function_component, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct NetworkDeviceProps {
    pub device: NetworkDevice,
}

/// A device as a list item: the name its DHCP client sent, its address with the MAC as tooltip
/// (saying whether the device made the MAC up), and how it is connected.
#[function_component(NetworkDeviceItem)]
pub fn network_device_item(props: &NetworkDeviceProps) -> Html {
    let device = &props.device;
    let name = device.hostname.as_deref().unwrap_or(t::NO_HOSTNAME);
    let address = device.address.as_deref().unwrap_or(&device.mac_address);
    let random = u8::from_str_radix(device.mac_address.get(..2).unwrap_or_default(), 16)
        .is_ok_and(|first| first & 2 != 0);
    let wlan = device
        .wlan
        .as_ref()
        .map(|wlan| {
            let band = wlan.band.as_ref().map(band).unwrap_or_default();
            t::wlan_connection(&wlan.access_point, &band, wlan.signal)
        })
        .unwrap_or_default();
    html! {
        <li>
            {name}{" "}
            <code title={t::mac_title(&device.mac_address, random)} class="sock-address">
                {address}
            </code>
            {" "}<small>{wlan}</small>
        </li>
    }
}

/// „5 GHz (ax)“; empty for a band of a newer server.
fn band(band: &WlanBand) -> String {
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
