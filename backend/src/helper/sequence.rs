//! Scope: Several helpers' plans as one: each computed on NetBox as the ones before it leave
//! it, its steps behind theirs; not what a helper plans (its module).
use super::{Helper, NetboxPlan};
use crate::{Error, topology::Topology};
use std::sync::Arc;

/// NetBox as `plan` leaves it, or as it is without one.
pub fn after(plan: Option<&NetboxPlan>, topology: &Arc<Topology>) -> Result<Arc<Topology>, Error> {
    match plan {
        Some(plan) => plan.preview_topology(),
        None => Ok(topology.clone()),
    }
}

/// `plan` followed by `part`'s plan, computed on `now` (NetBox as `plan` leaves it, `after`);
/// a part with nothing to change leaves it as it is.
pub async fn then(
    plan: Option<NetboxPlan>,
    now: &Arc<Topology>,
    part: &Helper,
) -> Result<Option<NetboxPlan>, Error> {
    let next = match part.plan(now).await {
        Ok(next) => next,
        Err(Error::NothingToChange) => return Ok(plan),
        Err(error) => return Err(error),
    };
    Ok(Some(match plan {
        Some(mut plan) => {
            plan.append(next)?;
            plan
        }
        None => next,
    }))
}

/// The plan of `parts`, one after the other.
pub async fn plan(parts: &[Helper], topology: &Arc<Topology>) -> Result<NetboxPlan, Error> {
    let mut plan = None;
    for part in parts {
        let now = after(plan.as_ref(), topology)?;
        plan = then(plan, &now, part).await?;
    }
    plan.ok_or(Error::NothingToChange)
}
