//! Scope: A device's name as a link to its page, wherever a device is named; nothing else.
use crate::routes::AppRoute;
use yew::{AttrValue, Html, Properties, function_component, html};
use yew_nested_router::components::Link;

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DeviceLinkProps {
    /// its site; without one the device has no page here, and its name stands alone
    pub site: Option<u32>,
    pub id: u32,
    pub name: AttrValue,
}

/// The device's name, linking to its page.
#[function_component(DeviceLink)]
pub fn device_link(props: &DeviceLinkProps) -> Html {
    match props.site {
        Some(site) => html! {
            <Link<AppRoute> to={AppRoute::device(site, props.id)}>{props.name.clone()}</Link<AppRoute>>
        },
        None => html!({ props.name.clone() }),
    }
}
