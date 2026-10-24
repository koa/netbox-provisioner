//! Helper „Kabel übernehmen“: the device sees another router at a port than NetBox has there
//! (`docs/concepts/interface-view.md`); NetBox takes over what the device sees. One plan of
//! „Verbindung abräumen“ for each end whose link goes elsewhere in NetBox, then „Verbindung“
//! between the two, computed on NetBox as the clearing leaves it. A cable over a patch panel is
//! refused: the new cable is a direct one, the panels in NetBox document the building.
use super::{
    NetboxPlan,
    connection::{self, ConnectionInput},
    disconnect::{self, DisconnectInput},
    port_name,
};
use crate::{
    Error,
    topology::{
        InterfaceId, Topology,
        access::{cable::CablePortAccess, interface::InterfaceAccess},
    },
};
use async_graphql::InputObject;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct CableTakeoverInput {
    /// the port on the device the plan is for
    pub port: u32,
    /// the port the device sees at it
    pub peer: u32,
}

pub async fn plan(topology: &Arc<Topology>, input: &CableTakeoverInput) -> Result<NetboxPlan, Error> {
    let cleared = clear(topology, input)?;
    let after = match &cleared {
        Some(cleared) => cleared.preview_topology()?,
        None => topology.clone(),
    };
    let connect = ConnectionInput {
        interface: input.port,
        peer: input.peer,
        mtu: None,
    };
    let connection = connection::plan(&after, &connect).await?;
    let Some(mut plan) = cleared else {
        return Ok(connection);
    };
    let offset = plan.steps.len();
    plan.steps.extend(connection.steps.into_iter().map(|mut step| {
        step.shift(offset);
        step
    }));
    plan.description = format!("Kabel übernehmen: {}", connection.description).into();
    plan.checked_with = connection.checked_with;
    plan.undocumented.extend(connection.undocumented);
    Ok(plan)
}

/// „Verbindung abräumen“ at each end whose cable or transfer addresses go elsewhere, one after
/// the other on NetBox as the one before leaves it; none if NetBox has no such link.
fn clear(topology: &Arc<Topology>, input: &CableTakeoverInput) -> Result<Option<NetboxPlan>, Error> {
    let end = |id| {
        topology
            .get_interface_by_id(InterfaceId(id))
            .ok_or(Error::UnknownInterface(id))
    };
    let (port, peer) = (end(input.port)?, end(input.peer)?);
    let device = port.device().ok_or(Error::UnknownInterface(input.port))?;
    let cabled_together = port.connected_interfaces().iter().any(|i| i.id() == peer.id());
    let mut plan: Option<NetboxPlan> = None;
    for end in [&port, &peer] {
        if patched(end) {
            return Err(Error::PortPatched(port_name(end)));
        }
    }
    for end in [&port, &peer] {
        if cabled_together || !has_link(end) {
            continue;
        }
        let now = match &plan {
            Some(plan) => plan.preview_topology()?,
            None => topology.clone(),
        };
        let input = DisconnectInput {
            device: end.device().ok_or(Error::UnknownInterface(end.id().0))?.id().0,
            interface: end.id().0,
        };
        let cleared = match disconnect::plan(&now, &input) {
            Ok(cleared) => cleared,
            Err(Error::NothingToDisconnect(_)) => continue,
            Err(error) => return Err(error),
        };
        let plan = plan.get_or_insert_with(|| NetboxPlan::new(&device, ""));
        // only deletions and changes of existing objects: no step references another
        plan.steps.extend(cleared.steps);
        for cut in cleared.cut_off {
            if !plan.cut_off.iter().any(|known| known.id == cut.id) {
                plan.cut_off.push(cut);
            }
        }
    }
    Ok(plan)
}

/// Whether the cable at `end` goes to a patch panel (or a circuit), not to the far interface.
fn patched(end: &InterfaceAccess) -> bool {
    end.cable().is_some_and(|cable| {
        cable
            .port_a()
            .chain(cable.port_b())
            .any(|port| !matches!(port, CablePortAccess::Interface(_)))
    })
}

/// Whether NetBox has a link at `end`: a cable, or addresses (the transfer pool's go with it).
fn has_link(end: &InterfaceAccess) -> bool {
    end.cable().is_some() || !end.ips().is_empty()
}

#[cfg(test)]
mod test;
