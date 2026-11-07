//! Scope: The page of the network map of every site; not the map (`components::network_map`).
//!
//! The network map of every site (`/network-map`).
use crate::components::network_map::NetworkMap;
use yew::{Html, function_component, html};

#[function_component(AllSitesMap)]
pub fn all_sites_map() -> Html {
    html!(<NetworkMap site={None}/>)
}
