use crate::topology::{DeviceId, Topology};
use async_graphql::InputObject;
use device::DeviceAccess;
use std::sync::Arc;

pub mod cable;
pub mod connections;
pub mod device;
pub mod front_port;
pub mod graphql;
pub mod interface;
pub mod ip_addresses;
pub mod ip_prefix;
pub mod ip_ranges;
pub mod rear_port;
pub mod vlan;
pub mod vxlan;
pub mod wlan;
pub mod wlan_group;

trait AccessTopology {
    type Id: Copy;
    type Data;
    fn topology(&self) -> Arc<Topology>;
    fn id(&self) -> Self::Id;
    fn data(&self) -> Option<&Self::Data>;
    fn create(topology: Arc<Topology>, id: Self::Id) -> Self;

    fn create_access<Id, Access>(&self) -> impl Fn(Id) -> Access
    where
        Access: AccessTopology<Id = Id>,
    {
        |id| Access::create(self.topology(), id)
    }
}

impl Topology {
    pub fn list_devices(self: &Arc<Self>) -> impl Iterator<Item = DeviceAccess> {
        let topo = self;
        self.devices
            .keys()
            .copied()
            .map(move |id| DeviceAccess::create(topo.clone(), id))
    }
    pub fn get_device_by_id(self: &Arc<Self>, id: &DeviceId) -> Option<DeviceAccess> {
        if self.devices.contains_key(id) {
            Some(DeviceAccess::create(self.clone(), *id))
        } else {
            None
        }
    }
}

#[derive(InputObject)]
struct AdhocCredentials {
    username: Option<Box<str>>,
    #[graphql(secret)]
    password: Option<Box<str>>,
}
