//! Scope: How far each device of a site is from NetBox, as a job of the site; not the diff of one
//! device (`ros::cfg`).
//!
//! How far each device of a site is from NetBox (`diffSite`, „Abweichung von NetBox“ in
//! `docs/config-helper.md`): a job of the site reads its RouterOS devices, a few at
//! a time, and reports for each, as it is read, what an apply would change by menu, or why it
//! couldn't be read. It only reads, so its devices stay free for jobs of their own.
use crate::{
    Error,
    device::ros::{
        cfg::{GeneratedCfg, in_script},
        graphql::of_device_as_is,
    },
    error_info::ErrorInfo,
    facts::{self, FactKind, Freshness},
    job::{self, Log},
    topology::{
        access::{DeviceTarget, device::DeviceAccess, site::SiteAccess},
        natural::naturalize,
    },
};
use async_graphql::SimpleObject;
use mikrotik_model::resource::{ResourceMutation, ResourceMutationOperation};
use std::{collections::BTreeMap, sync::Arc};
use tokio::{sync::Semaphore, task::JoinSet};

/// How many devices are read at once: they share the way through the management proxy.
const AT_ONCE: usize = 4;

/// The devices the job reads, in NetBox' order.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SiteDiffStarted {
    pub devices: Vec<DiffedDevice>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DiffedDevice {
    pub id: u32,
    pub name: String,
}

/// What an apply would do in one menu (`ip/address`).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct MenuChanges {
    pub menu: String,
    pub added: u32,
    pub changed: u32,
    pub removed: u32,
}

/// One device read: its changes by menu (none: it is as NetBox has it), or why it couldn't be
/// read.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceDiffed {
    pub device: DiffedDevice,
    pub menus: Vec<MenuChanges>,
    pub error: Option<ErrorInfo>,
}

/// The job's work.
pub async fn run(site: SiteAccess, log: Log) -> Result<(), Error> {
    let started = tokio::time::Instant::now();
    let mut devices = site
        .devices()
        .filter(DeviceAccess::has_routeros)
        .collect::<Vec<_>>();
    devices.sort_by_cached_key(|device| naturalize(device.name()));
    log.emit(SiteDiffStarted {
        devices: devices.iter().map(diffed_device).collect(),
    });
    let permits = Arc::new(Semaphore::new(AT_ONCE));
    let mut reads = JoinSet::new();
    for device in devices {
        let permits = permits.clone();
        reads.spawn(async move {
            let _permit = permits.acquire_owned().await;
            let menus = job::run(changes_of(device.clone(), started)).await;
            (device, menus)
        });
    }
    // cancelled: the reads still going end with `reads`
    while let Some(read) = log.unless_cancelled(reads.join_next()).await? {
        let Ok((device, menus)) = read else { continue };
        let (menus, error) = match menus {
            Ok(menus) => (menus, None),
            Err(error) => (Vec::new(), Some(ErrorInfo::from(&error))),
        };
        log.emit(DeviceDiffed {
            device: diffed_device(&device),
            menus,
            error,
        });
    }
    Ok(())
}

fn diffed_device(device: &DeviceAccess) -> DiffedDevice {
    DiffedDevice {
        id: device.id().0,
        name: device.name().into(),
    }
}

/// Read anew for the job (the device page shares it: `FactKind::Config`).
async fn changes_of(
    device: DeviceAccess,
    started: tokio::time::Instant,
) -> Result<Vec<MenuChanges>, Error> {
    let generated = device
        .connect(DeviceTarget::default())
        .await?
        .ok_or(Error::MissingCredentials)?
        .generated_cfg(Freshness::Since(started))
        .await
        .result()?;
    Ok(generated.menu_changes().to_vec())
}

/// What the provisioner knows of how far a device is from NetBox, without asking it: the
/// planned changes read last by anyone (`diffSite`, the device page), and since when what they
/// were read for changed.
#[derive(Debug, SimpleObject)]
pub struct KnownDiff {
    /// the changes read last by menu (none: it is as NetBox has it); `null` when none are
    /// known (anymore)
    pub menus: Option<Vec<MenuChanges>>,
    /// why the device couldn't be read last
    pub error: Option<ErrorInfo>,
    /// seconds since it was read
    pub age: Option<u64>,
    /// seconds since what was read dropped: a job on the device ended, NetBox changed
    pub changed_age: Option<u64>,
}

pub fn known(device: &DeviceAccess) -> KnownDiff {
    let fact = facts::peek_newest::<GeneratedCfg>(FactKind::Config, device.id(), of_device_as_is);
    let (menus, error) = match fact.as_ref().map(|fact| fact.result()) {
        Some(Ok(generated)) => (Some(generated.menu_changes().to_vec()), None),
        Some(Err(error)) => (None, Some(ErrorInfo::from(&error))),
        None => (None, None),
    };
    KnownDiff {
        menus,
        error,
        age: fact.map(|fact| fact.age().as_secs()),
        changed_age: facts::dropped(FactKind::Config, device.id()).map(|at| at.elapsed().as_secs()),
    }
}

/// The mutations counted by menu and kind, the menus in alphabetical order. Like the script,
/// it leaves out what has no fields but a removal or a move: the diff has an update for every
/// entry, most of them without a change. A move of the firewall rules counts as one change.
pub fn menu_changes(mutations: &[ResourceMutation<'_>]) -> Vec<MenuChanges> {
    let mut menus = BTreeMap::<&[u8], (u32, u32, u32)>::new();
    for mutation in mutations.iter().filter(|mutation| in_script(mutation)) {
        let (added, changed, removed) = menus.entry(mutation.resource).or_default();
        match mutation.operation {
            ResourceMutationOperation::Add => *added += 1,
            ResourceMutationOperation::RemoveByKey(_) => *removed += 1,
            ResourceMutationOperation::UpdateSingle
            | ResourceMutationOperation::UpdateByKey(_)
            | ResourceMutationOperation::Move { .. } => *changed += 1,
        }
    }
    menus
        .into_iter()
        .map(|(menu, (added, changed, removed))| MenuChanges {
            menu: String::from_utf8_lossy(menu).into_owned(),
            added,
            changed,
            removed,
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::{MenuChanges, menu_changes};
    use mikrotik_model::{
        resource::{ResourceMutation, ResourceMutationOperation},
        value::KeyValuePair,
    };

    fn mutation(
        resource: &'static [u8],
        operation: ResourceMutationOperation<'static>,
    ) -> ResourceMutation<'static> {
        ResourceMutation {
            resource,
            operation,
            fields: Box::new([key()]),
            depends: Box::new([]),
            provides: Box::new([]),
            ordering: Default::default(),
        }
    }

    fn key() -> KeyValuePair<'static> {
        KeyValuePair {
            key: b"address",
            value: b"192.168.1.1/24".as_slice().into(),
        }
    }

    #[test]
    fn counts_by_menu_and_kind() {
        let mutations = [
            mutation(b"ip/address", ResourceMutationOperation::RemoveByKey(key())),
            mutation(b"ip/address", ResourceMutationOperation::RemoveByKey(key())),
            mutation(b"ip/address", ResourceMutationOperation::Add),
            mutation(
                b"interface/ethernet",
                ResourceMutationOperation::UpdateByKey(key()),
            ),
            mutation(b"system/identity", ResourceMutationOperation::UpdateSingle),
            // unchanged: not in the script
            ResourceMutation {
                fields: Box::new([]),
                ..mutation(b"system/watchdog", ResourceMutationOperation::UpdateSingle)
            },
            ResourceMutation {
                fields: Box::new([]),
                ..mutation(b"ip/address", ResourceMutationOperation::UpdateByKey(key()))
            },
        ];
        let menu = |menu: &str, added, changed, removed| MenuChanges {
            menu: menu.into(),
            added,
            changed,
            removed,
        };
        assert_eq!(
            menu_changes(&mutations),
            [
                menu("interface/ethernet", 0, 1, 0),
                menu("ip/address", 1, 0, 2),
                menu("system/identity", 0, 1, 0),
            ]
        );
        assert!(menu_changes(&[]).is_empty());
    }
}
