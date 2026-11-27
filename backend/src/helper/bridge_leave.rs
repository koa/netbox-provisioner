//! Scope: The helper „Aus Bridge lösen“; nothing else.
//!
//! Helper „Aus Bridge lösen“: takes a port out of its bridge in NetBox, with the VLANs of the
//! port itself, so it is free again (`docs/interface-view.md`). Its cable stays. The
//! bridge stays too: its last port gone, the plan names it (`NetboxPlan.emptied_bridge`), its
//! network then reaches nobody; „Kunden-Anschluss abräumen“ on its row takes that out.
use super::{NetboxPlan, PlanId, PlanStep, UpdatePort, UpdatePortVlan, plan_interface, port_name};
use crate::{
    Error,
    topology::{DeviceId, InterfaceId, Topology, findings::vlan_ref},
};
use async_graphql::InputObject;
use i18n::changelog::Plan;
use std::sync::Arc;

/// The port, for the device whose page asks (its job writes it).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct BridgeLeaveInput {
    pub device: u32,
    pub port: u32,
}

pub fn plan(topology: &Arc<Topology>, input: &BridgeLeaveInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let port = topology
        .get_interface_by_id(InterfaceId(input.port))
        .ok_or(Error::UnknownInterface(input.port))?;
    let bridge = port
        .bridge()
        .ok_or_else(|| Error::NoBridge(port_name(&port)))?;
    let owner = port.device().ok_or(Error::UnknownInterface(input.port))?;
    let mut plan = NetboxPlan::new(
        &device,
        Plan::BridgeLeave {
            port: super::changelog_port(&owner, &port),
            bridge: bridge.name().into(),
        },
    );
    plan.steps.push(PlanStep::UpdatePort(UpdatePort {
        interface: plan_interface(&port),
        name: None,
        rf_role: None,
        rf_role_before: None,
        bridge: None,
        bridge_id: None,
        bridge_before: Some(bridge.plan_id(bridge.id().0)),
        leaves_bridge: Some(bridge.name().into()),
    }));
    let untagged = port.untagged_vlan();
    let tagged = port.tagged_vlans().collect::<Vec<_>>();
    if untagged.is_some() || !tagged.is_empty() {
        plan.steps.push(PlanStep::UpdatePortVlan(UpdatePortVlan {
            interface: plan_interface(&port),
            vlan: None,
            vlan_before: untagged.as_ref().and_then(vlan_ref),
            tagged_before: tagged.iter().filter_map(vlan_ref).collect(),
        }));
    }
    let others = owner
        .interfaces()
        .iter()
        .filter(|i| i.id() != port.id() && i.bridge().is_some_and(|b| b.id() == bridge.id()))
        .count();
    if others == 0 {
        plan.emptied_bridge = Some(crate::topology::findings::interface_ref(&bridge));
    }
    Ok(plan)
}

#[cfg(test)]
mod test;
