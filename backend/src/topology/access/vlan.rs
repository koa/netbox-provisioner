use crate::topology::{
    InterfaceId, Topology, VlanData, VlanGroupId, VlanId,
    access::{AccessTopology, device::DeviceAccess, vxlan::VxlanAccess, wlan::WlanAccess},
};
use async_graphql::Object;
use std::{
    collections::BTreeSet,
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct VlanAccess {
    topology: Arc<Topology>,
    pub id: VlanId,
}

impl AccessTopology for VlanAccess {
    type Id = VlanId;
    type Data = VlanData;

    fn topology(&self) -> Arc<Topology> {
        self.topology.clone()
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn data(&self) -> Option<&VlanData> {
        self.topology.vlans.get(&self.id)
    }

    fn create(topology: Arc<Topology>, id: Self::Id) -> Self {
        VlanAccess { topology, id }
    }
}

impl VlanAccess {
    pub fn name(&self) -> Option<&str> {
        self.data().map(|v| v.name.as_ref())
    }
    pub fn vlan_id(&self) -> Option<u16> {
        self.data().map(|v| v.vlan_id)
    }
    /// The VXLAN of this VLAN's group, if the group has a VNI - whether or not
    /// this VLAN actually needs it, see `tunnel`.
    pub fn vxlan(&self) -> Option<VxlanAccess> {
        self.data().and_then(|v| v.vxlan).map(self.create_access())
    }
    /// The VXLAN this VLAN is tunneled over: its group has a VNI and the VLAN is
    /// used on more than one device. A VLAN used on a single device stays local.
    pub fn tunnel(&self) -> Option<VxlanAccess> {
        self.vxlan().filter(|_| self.devices().len() > 1)
    }
    /// Its VLAN group, by id and name.
    pub fn group(&self) -> Option<(VlanGroupId, &str)> {
        let group = self.data()?.group;
        let data = self.topology.vlan_groups.get(&group)?;
        Some((group, data.name.as_ref()))
    }
    /// Every device this VLAN is used on: carried by one of its interfaces, or a
    /// WLAN or management VLAN of the WLAN group it is an AP or the controller of.
    pub fn devices(&self) -> Box<[DeviceAccess]> {
        self.devices_but(None)
    }
    /// [`Self::devices`] as if `interface` didn't carry it (a port about to get another VLAN).
    pub fn devices_but(&self, interface: Option<InterfaceId>) -> Box<[DeviceAccess]> {
        let topology = &self.topology;
        let mut devices = topology
            .interfaces
            .iter()
            .filter(|(id, _)| Some(**id) != interface)
            .map(|(_, i)| i)
            .filter(|i| i.vlan == Some(self.id) || i.tagged_vlans.contains(&self.id))
            .map(|i| i.device)
            .collect::<BTreeSet<_>>();
        for group in topology.wlan_groups.values() {
            let in_group = group.mgmt_vlan == Some(self.id)
                || group.wlans.iter().any(|wlan| {
                    topology
                        .wlans
                        .get(wlan)
                        .is_some_and(|wlan| wlan.vlan == Some(self.id))
                });
            if in_group {
                devices.insert(group.controller);
                devices.extend(group.aps.iter().copied());
            }
        }
        devices.into_iter().map(self.create_access()).collect()
    }
    pub fn wlan<'s>(&'s self) -> impl Iterator<Item = WlanAccess> + use<'s> {
        self.data()
            .into_iter()
            .flat_map(|d| d.wlans.iter().cloned())
            .map(self.create_access())
    }
}

impl Debug for VlanAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VlanAccess({};{}({}))",
            self.id.0,
            self.name().unwrap_or_default(),
            self.vlan_id().unwrap_or_default()
        )
    }
}

impl Hash for VlanAccess {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[Object]
impl VlanAccess {
    #[graphql(name = "id")]
    async fn api_id(&self) -> u32 {
        self.id.0
    }
    #[graphql(name = "name")]
    async fn api_name(&self) -> &str {
        self.name().unwrap_or_default()
    }
    async fn api_vlan_id(&self) -> u16 {
        self.vlan_id().expect("vlan_id not set")
    }
}
