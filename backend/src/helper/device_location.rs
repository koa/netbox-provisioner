//! Scope: The helper „Raum eines Geräts“: the NetBox location of a device; not who may
//! (`graphql::resident::change`).
//!
//! A device stands in the room of its socket, unless a patch cable leads through the wall: its
//! location is then another. A RouterOS device without a tenant of its own takes its
//! credentials from its location's tenant; a location giving it others is refused, the
//! provisioner would lose it.
use super::{LocationRef, NetboxPlan, PlanStep, UpdateDeviceLocation};
use crate::{
    Error,
    topology::{DeviceId, Topology, access::device::DeviceAccess, findings::device_ref},
};
use std::sync::Arc;

/// A device and the location it is put in, none for the site only.
#[derive(Debug, Clone, PartialEq, Eq, async_graphql::InputObject)]
pub struct DeviceLocationInput {
    pub device: u32,
    pub location: Option<u32>,
}

pub fn plan(topology: &Arc<Topology>, input: &DeviceLocationInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    if device.location_id() == input.location {
        return Err(Error::NothingToChange);
    }
    let location = |id: u32| {
        let data = topology
            .locations()
            .get(&id)
            .ok_or(Error::UnknownLocation(id))?;
        Ok::<_, Error>(LocationRef {
            id,
            path: data.path.clone(),
        })
    };
    let mut plan = NetboxPlan::new(&device, format!("Raum von {}", device.name()));
    plan.steps
        .push(PlanStep::UpdateDeviceLocation(UpdateDeviceLocation {
            device: device_ref(&device),
            location: input.location.map(location).transpose()?,
            location_before: device.location_id().map(location).transpose()?,
        }));
    if device.has_routeros()
        && credentials_after(&plan, &device)?.as_deref() != device.credentials()
    {
        return Err(Error::OtherCredentials(device.name().into()));
    }
    Ok(plan)
}

/// The credentials `device` has once `plan` is written.
fn credentials_after(plan: &NetboxPlan, device: &DeviceAccess) -> Result<Option<Box<str>>, Error> {
    let after = plan.preview_topology()?;
    Ok(after
        .get_device_by_id(&device.id())
        .and_then(|device| device.credentials().map(Box::from)))
}

#[cfg(test)]
mod test;
