//! The network as OSPF sees it, for the network map (`docs/concepts/network-map.md`): the
//! link-state database read from one router (`lsa`) and the state of NetBox's edges from it
//! (`edges`).

pub mod edges;
pub mod graphql;
pub mod lsa;
pub mod read;
