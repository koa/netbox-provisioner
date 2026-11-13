//! Scope: The network as OSPF sees it, for the network map; not the map's drawing (frontend).
//!
//! The network as OSPF sees it, for the network map (`docs/network-map.md`): the
//! link-state database read from one router (`lsa`) and the state of NetBox's edges from it
//! (`edges`).

pub mod device_data;
pub mod edges;
pub mod graphql;
pub mod lsa;
pub mod read;
