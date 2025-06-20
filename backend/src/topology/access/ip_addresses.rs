use crate::topology::{
    IpAddressData, IpAddressId, Topology,
    access::{
        AccessTopology, graphql::IpNetGraphql, interface::InterfaceAccess,
        ip_prefix::IpPrefixAccess,
    },
};
use async_graphql::Object;
use ipnet::IpNet;
use std::{net::IpAddr, sync::Arc};

#[derive(Clone, PartialEq, Eq)]
pub struct IpAddressAccess {
    topology: Arc<Topology>,
    id: IpAddressId,
}
impl AccessTopology for IpAddressAccess {
    type Id = IpAddressId;
    type Data = IpAddressData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&Self::Data> {
        self.topology.ip_addresses.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        IpAddressAccess { topology, id }
    }
}

impl IpAddressAccess {
    pub fn addr(&self) -> Option<IpAddr> {
        self.data().map(|a| a.ip.addr())
    }
    pub fn net(&self) -> Option<IpNet> {
        self.data().map(|a| a.ip)
    }
    pub fn prefix(&self) -> Option<IpPrefixAccess> {
        self.data().and_then(|a| a.prefix).map(self.create_access())
    }
    pub fn interface(&self) -> Option<InterfaceAccess> {
        self.data()
            .and_then(|a| a.interface)
            .map(self.create_access())
    }
}

#[Object]
impl IpAddressAccess {
    async fn address(&self) -> Option<IpNetGraphql> {
        self.data().map(|d| d.ip.into())
    }
}
