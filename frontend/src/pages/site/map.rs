//! Scope: The page of a site's network map; not the map (`components::network_map`).
//!
//! A site's network map (`/site/<id>/map`).
use crate::components::network_map::NetworkMap;
use yew::{Html, Properties, function_component, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct SiteMapProps {
    pub site: u32,
}

#[function_component(SiteMap)]
pub fn site_map(props: &SiteMapProps) -> Html {
    html!(<NetworkMap site={Some(props.site)}/>)
}
