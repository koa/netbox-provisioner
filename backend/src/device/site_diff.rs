//! Scope: How far each device of a site is from NetBox, as a job of the site; not the diff of one
//! device (`ros::cfg`).
//!
//! How far each device of a site is from NetBox (`diffSite`, „Abweichung von NetBox“ in
//! `docs/config-helper.md`): a job of the site reads its RouterOS devices, a few at
//! a time, and reports for each, as it is read, what an apply would change by table, or why it
//! couldn't be read. It only reads, so its devices stay free for jobs of their own.
use crate::{
    Error,
    device::{
        Credentials,
        access::Reader,
        known_password,
        ros::{cfg::in_script, graphql::generate},
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
    pub devices: Box<[DiffedDevice]>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DiffedDevice {
    pub id: u32,
    pub name: Box<str>,
}

/// What an apply would do in one table (`ip/address`).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct TableChanges {
    pub table: Box<str>,
    pub added: u32,
    pub changed: u32,
    pub removed: u32,
}

/// One device read: its changes by table (none: it is as NetBox has it), or why it couldn't be
/// read.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct DeviceDiffed {
    pub device: DiffedDevice,
    pub tables: Box<[TableChanges]>,
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
            let tables = job::run(changes_of(device.clone(), started)).await;
            (device, tables)
        });
    }
    // cancelled: the reads still going end with `reads`
    while let Some(read) = log.unless_cancelled(reads.join_next()).await? {
        let Ok((device, tables)) = read else { continue };
        let (tables, error) = match tables {
            Ok(tables) => (tables, None),
            Err(error) => (Vec::new(), Some(ErrorInfo::from(&error))),
        };
        log.emit(DeviceDiffed {
            device: diffed_device(&device),
            tables: tables.into(),
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

/// Read anew for the job (the device page shares what it reads).
async fn changes_of(
    device: DeviceAccess,
    started: tokio::time::Instant,
) -> Result<Vec<TableChanges>, Error> {
    let generated = device
        .connect(DeviceTarget::default())
        .await?
        .ok_or(Error::MissingCredentials)?
        .generated_cfg(Freshness::Since(started))
        .await?;
    Ok(generated.table_changes().to_vec())
}

/// What the provisioner knows of how far a device is from NetBox, without asking it: the
/// planned changes from what anyone read of it last (`diffSite`, the device page: the facts it
/// left), and since when what was read was dropped.
#[derive(Debug, SimpleObject)]
pub struct KnownDiff {
    /// the changes by table (none: it is as NetBox has it); `null` when what they come from
    /// isn't known (anymore)
    pub tables: Option<Vec<TableChanges>>,
    /// why the device couldn't be reached last, when it couldn't
    pub error: Option<ErrorInfo>,
    /// seconds since the oldest of what they come from was read
    pub age: Option<u64>,
    /// seconds since what was read of it was dropped: a job on the device ended, it was read
    /// anew
    pub changed_age: Option<u64>,
}

/// Reads `device`'s diff anew right after its apply ended: the job dropped what was read, and
/// the site's overview, which only looks at what is kept ([`known`]), would show it changed
/// until someone read it. Only reads; a failure is logged and nothing else.
pub async fn read_after_apply(device: DeviceAccess) {
    // one without credentials, the tests' jobs, has nothing to read with
    if device.credentials().is_none() {
        return;
    }
    let since = Freshness::Since(tokio::time::Instant::now());
    let read = async {
        let link = device
            .connect(DeviceTarget::default())
            .await?
            .ok_or(Error::MissingCredentials)?;
        link.generated_cfg(since).await
    };
    match read.await {
        Ok(_) => log::debug!("{}: diff read after the apply", device.name()),
        Err(error) => log::info!(
            "{}: cannot read the diff after the apply: {error}",
            device.name()
        ),
    }
}

pub async fn known(device: &DeviceAccess) -> KnownDiff {
    let changed_age = facts::dropped(FactKind::Table, device.id()).map(|at| at.elapsed().as_secs());
    let Some(credentials) = device.credentials() else {
        return KnownDiff {
            tables: None,
            error: None,
            age: None,
            changed_age,
        };
    };
    let scope = Credentials::Named(credentials.into()).fact_key();
    let reader = Reader::stored(device.id(), &scope);
    match generate(&reader, device, known_password(device.id())).await {
        Ok(generated) => KnownDiff {
            tables: Some(generated.table_changes().to_vec()),
            error: None,
            age: Some(reader.as_of().elapsed().as_secs()),
            changed_age,
        },
        Err(_) => KnownDiff {
            tables: None,
            error: device.unreached().map(|error| ErrorInfo::from(&*error)),
            age: None,
            changed_age,
        },
    }
}

/// The mutations counted by table and kind, the tables in alphabetical order. Like the script,
/// it leaves out what has no fields but a removal or a move: the diff has an update for every
/// entry, most of them without a change. A move of the firewall rules counts as one change.
pub fn table_changes(mutations: &[ResourceMutation<'_>]) -> Vec<TableChanges> {
    let mut tables = BTreeMap::<&[u8], (u32, u32, u32)>::new();
    for mutation in mutations.iter().filter(|mutation| in_script(mutation)) {
        let (added, changed, removed) = tables.entry(mutation.resource).or_default();
        match mutation.operation {
            ResourceMutationOperation::Add | ResourceMutationOperation::AddThenSet { .. } => {
                *added += 1
            }
            ResourceMutationOperation::RemoveByKey(_) => *removed += 1,
            ResourceMutationOperation::UpdateSingle
            | ResourceMutationOperation::UpdateByKey(_)
            | ResourceMutationOperation::Move { .. } => *changed += 1,
        }
    }
    tables
        .into_iter()
        .map(|(table, (added, changed, removed))| TableChanges {
            table: String::from_utf8_lossy(table).into_owned().into(),
            added,
            changed,
            removed,
        })
        .collect()
}

/// A change an apply makes outside the script, counted in `table`: the current password where
/// the login went with the previous one (A9, `user`; the password stays out of the script),
/// WebFig's certificate (A7, `certificate`).
pub(crate) fn add_change(tables: &mut Vec<TableChanges>, name: &str) {
    let at = tables.partition_point(|table| &*table.table < name);
    match tables.get_mut(at) {
        Some(table) if table.table == name.into() => table.changed += 1,
        _ => tables.insert(
            at,
            TableChanges {
                table: name.into(),
                added: 0,
                changed: 1,
                removed: 0,
            },
        ),
    }
}

#[cfg(test)]
mod test {
    use super::{TableChanges, add_change, table_changes};
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
    fn a_password_change_goes_in_order() {
        let table = |table: &str| TableChanges {
            table: table.into(),
            added: 1,
            changed: 0,
            removed: 0,
        };
        let mut tables = vec![table("system/identity"), table("zzz")];
        add_change(&mut tables, "user");
        let names: Vec<_> = tables.iter().map(|table| &*table.table).collect();
        assert_eq!(names, ["system/identity", "user", "zzz"]);
        assert_eq!(tables[1].changed, 1);
    }

    #[test]
    fn counts_by_table_and_kind() {
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
        let table = |table: &str, added, changed, removed| TableChanges {
            table: table.into(),
            added,
            changed,
            removed,
        };
        assert_eq!(
            table_changes(&mutations),
            [
                table("interface/ethernet", 0, 1, 0),
                table("ip/address", 1, 0, 2),
                table("system/identity", 0, 1, 0),
            ]
        );
        assert!(table_changes(&[]).is_empty());
    }
}
