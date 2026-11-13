//! Scope: Planning updates across the network: which devices, in what order, with which packages;
//! not carrying the plan out (`rollout::run`).
//!
//! Updates across the network (`docs/concepts/network-update.md`): a rollout brings the
//! RouterOS devices of the sites chosen to the newest release of a channel, one ordinary update
//! job per device. This part plans it, reading only: per device what it runs, what an update
//! would install, whether it has a way back and room for the packages, and where it comes in
//! the order (a pilot per architecture first, then from the outside in).
use crate::{
    Error,
    device::{
        dependency::{Paths, consumer_devices, supplier_devices},
        leftovers, partitions,
        ros::progress,
        update::{self, Channel, Version, newest, next_step, package_size},
        update_fallback,
    },
    error_info::ErrorInfo,
    graphql::scalars::ScalarU64,
    job::Log,
    topology::{
        access::{AccessTopology, DeviceTarget, device::DeviceAccess, site::SiteAccess},
        natural::naturalize,
    },
};
use async_graphql::{Enum, SimpleObject};
use mikrotik_model::{
    model::{File, SystemPackageState, SystemResourceState, SystemRouterboardState},
    resource::{SingleResource, collect_resource},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex},
};
use tokio::{sync::Semaphore, task::JoinSet};

/// How many devices are read at once: they share the way through the management proxy.
const AT_ONCE: usize = 4;

/// What a rollout would do.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutPlan {
    pub channel: Channel,
    /// the newest release of the channel
    pub target: Box<str>,
    /// the devices to update in their order, then those left out
    pub devices: Box<[PlannedDevice]>,
}

/// One RouterOS device of the sites chosen.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PlannedDevice {
    pub id: u32,
    pub name: Box<str>,
    pub site_id: u32,
    pub site: Box<str>,
    /// what it runs; `null` when it couldn't be read
    pub version: Option<Box<str>>,
    pub architecture: Option<Box<str>>,
    /// the package files the update installs (only the packages where it stays at its
    /// version)
    pub files: Box<[Box<str>]>,
    /// the version of those files: the update's first step (the newest v6 for a v6 device)
    pub step: Option<Box<str>>,
    /// their size, from MikroTik's server
    pub download: Option<ScalarU64>,
    /// where the packages are uploaded to: `free-hdd-space`, or `free-memory` where the root
    /// directory is in RAM (`in_ram`)
    pub free_space: Option<ScalarU64>,
    /// the root directory is in RAM (16 MiB of flash): the packages take memory
    pub in_ram: bool,
    pub fallback: RolloutFallback,
    /// why it is left out; `null`: it is updated
    pub skip: Option<RolloutSkip>,
    /// why it couldn't be read
    pub error: Option<ErrorInfo>,
    /// first of its architecture: the rollout stops after the pilots
    pub pilot: bool,
    /// hops from the nearest management root; `null` off NetBox's ways
    pub depth: Option<u32>,
    /// the devices of the plan it doesn't update at the same time as (ids): those behind it on
    /// the ways, those it powers and those powering it
    pub related: Box<[u32]>,
    /// the devices of the plan whose reboot takes it offline (ids): those it is behind on the
    /// ways and those powering it
    pub cut_by: Box<[u32]>,
}

/// How the device comes back from an update that fails.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RolloutFallback {
    /// it boots the partition from before (`update_fallback`)
    Partition,
    /// it stays reachable through a neighbor (`DeviceAccess.updateNeighbors`)
    Neighbor,
    /// neither: the rollout leaves it to the device page
    None,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RolloutSkip {
    /// runs the release with the packages it needs
    UpToDate,
    /// couldn't be read
    Unreachable,
    /// a fallback of an apply or an update left on it (`DeviceAccess.leftovers`)
    Leftovers,
    /// no way back (`RolloutFallback::None`)
    NoFallback,
    /// less free space than the packages take
    NoSpace,
}

/// Plans a rollout of `channel` over `sites`.
pub async fn plan(sites: &[SiteAccess], channel: Channel) -> Result<RolloutPlan, Error> {
    let target = newest(7, channel).await?;
    let mut devices = sites
        .iter()
        .flat_map(SiteAccess::devices)
        .filter(DeviceAccess::has_routeros)
        .collect::<Vec<_>>();
    devices.sort_by_cached_key(|device| naturalize(device.name()));
    let paths = match devices.first() {
        Some(device) => Paths::of(device).await,
        None => Paths::default(),
    };
    let sizes = Arc::new(Mutex::new(HashMap::new()));
    let permits = Arc::new(Semaphore::new(AT_ONCE));
    let mut reads = JoinSet::new();
    for (index, device) in devices.iter().cloned().enumerate() {
        let (permits, target, sizes) = (permits.clone(), target.clone(), sizes.clone());
        reads.spawn(async move {
            let _permit = permits.acquire_owned().await;
            let read = crate::job::run(read(device.clone(), target, sizes)).await;
            (index, read)
        });
    }
    let mut read = BTreeMap::new();
    while let Some(done) = reads.join_next().await {
        if let Ok((index, result)) = done {
            read.insert(index, result);
        }
    }
    let depths = paths.depths();
    let ids = devices
        .iter()
        .map(|device| (Box::<str>::from(device.name()), device.id().0))
        .collect::<HashMap<_, _>>();
    // what is behind each, worked out once
    let behind = devices
        .iter()
        .map(|device| (Box::<str>::from(device.name()), paths.behind(device.name())))
        .collect::<HashMap<_, _>>();
    let mut planned = devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let result = read.remove(&index).unwrap_or(Err(Error::Cancelled));
            let mut planned = planned_device(device, result, depths.get(device.name()).copied());
            planned.related = related(device, &paths, &ids).into();
            planned.cut_by = cut_by(device, &behind, &ids).into();
            planned
        })
        .collect::<Vec<_>>();
    let candidates = devices
        .iter()
        .zip(&planned)
        .map(|(device, planned)| Candidate {
            name: device.name().into(),
            architecture: planned.architecture.clone().unwrap_or_default(),
            depth: planned.depth,
            leaf: paths.behind(device.name()).is_empty() && consumer_devices(device).is_empty(),
            caps: caps_of(device),
            update: planned.skip.is_none(),
        })
        .collect::<Vec<_>>();
    let order = order(&candidates);
    for in_order in &order {
        planned[in_order.index].pilot = in_order.pilot;
    }
    let positions = order
        .iter()
        .enumerate()
        .map(|(position, in_order)| (in_order.index, position))
        .collect::<HashMap<_, _>>();
    let mut sorted = planned.into_iter().enumerate().collect::<Vec<_>>();
    sorted.sort_by_key(|(index, _)| positions.get(index).copied().unwrap_or(usize::MAX));
    Ok(RolloutPlan {
        channel,
        target: target.to_string().into(),
        devices: sorted.into_iter().map(|(_, device)| device).collect(),
    })
}

/// Makes `pilots` (device ids) the pilots, those of them to be updated, in place of the
/// proposed ones, first in their order.
pub fn choose_pilots(plan: &mut RolloutPlan, pilots: &[u32]) {
    for device in &mut plan.devices {
        device.pilot = device.skip.is_none() && pilots.contains(&device.id);
    }
    // stable: the pilots keep their order, so do the rest
    plan.devices
        .sort_by_key(|device| match (device.pilot, device.skip.is_some()) {
            (true, _) => 0,
            (false, false) => 1,
            (false, true) => 2,
        });
}

/// The devices of the plan (`ids`, by name) `device` doesn't update at the same time as: those
/// behind it on the ways, those it powers, those powering it. Updating them together, the one
/// in front is away while the other uploads, and both suspend the same watchdogs (Berg 925,
/// 2026-11-07: switch-mimi with the three APs behind it, two of them powered by it).
fn related(device: &DeviceAccess, paths: &Paths, ids: &HashMap<Box<str>, u32>) -> Vec<u32> {
    let names = paths
        .behind(device.name())
        .into_iter()
        .chain(consumer_devices(device).iter().map(|d| d.name().into()))
        .chain(supplier_devices(device).iter().map(|d| d.name().into()))
        .collect::<BTreeSet<Box<str>>>();
    let mut related = names
        .iter()
        .filter(|name| &***name != device.name())
        .filter_map(|name| ids.get(name).copied())
        .collect::<Vec<_>>();
    related.sort_unstable();
    related
}

/// The devices of `devices` whose reboot takes `device` offline, by their ids: it is behind
/// them, or they power it.
fn cut_by(
    device: &DeviceAccess,
    behind: &HashMap<Box<str>, BTreeSet<Box<str>>>,
    ids: &HashMap<Box<str>, u32>,
) -> Vec<u32> {
    let mut cut_by = behind
        .iter()
        .filter(|(other, _)| &***other != device.name())
        .filter(|(_, behind)| behind.contains(device.name()))
        .map(|(other, _)| other.clone())
        .chain(supplier_devices(device).iter().map(|d| d.name().into()))
        .collect::<BTreeSet<Box<str>>>()
        .iter()
        .filter_map(|name| ids.get(name).copied())
        .collect::<Vec<_>>();
    cut_by.sort_unstable();
    cut_by
}

/// Downloads every package the updates of `plan` install, into the [`update::DownloadCache`]
/// its job holds, before the first update starts: an update may cut the way to the internet
/// (its device the uplink router), the others then upload what is here.
pub async fn download_all(plan: &RolloutPlan, log: &Log) -> Result<(), Error> {
    let files = plan
        .devices
        .iter()
        .filter(|device| device.skip.is_none())
        .filter_map(|device| Some((device.step.clone()?, &device.files)))
        .flat_map(|(step, files)| files.iter().map(move |file| (step.clone(), file.clone())))
        .collect::<BTreeSet<_>>();
    for (step, file) in files {
        let version = Version::parse(&step).ok_or_else(|| Error::NoVersion(step.clone()))?;
        let data = log
            .unless_cancelled(update::download(&version, &file))
            .await??;
        log.emit(update::PackageDownloaded {
            file,
            bytes: (data.len() as u64).into(),
        });
    }
    Ok(())
}

/// What one device runs and what an update would need there.
struct Read {
    state: update::DeviceState,
    step: Option<update::UpdateStep>,
    download: Option<u64>,
    free_space: Option<u64>,
    in_ram: bool,
    partition: bool,
    leftovers: bool,
}

async fn read(
    device: DeviceAccess,
    target: Version,
    sizes: Arc<Mutex<HashMap<Box<str>, Option<u64>>>>,
) -> Result<Read, Error> {
    let link = device
        .connect(DeviceTarget::default())
        .await?
        .ok_or(Error::MissingCredentials)?;
    let raw = link.reader();
    // each menu once (`/system/resource` for the state and the free space, `/file` for the
    // root and the leftover packages)
    let resource = SystemResourceState::fetch(raw).await?;
    let packages = collect_resource::<SystemPackageState>(raw).await?;
    // CHR has no RouterBOARD; the command fails there
    let routerboard = SystemRouterboardState::fetch(raw).await.ok().flatten();
    let files = collect_resource::<File>(raw).await?;
    let state = update::state_from(resource.as_ref(), &packages, routerboard.as_ref())?;
    let wanted = link
        .wanted_packages_for(state.model.as_deref().map(str::as_bytes))
        .await?;
    let newest_v6 = match state.version.major() < 7 {
        true => Some(newest(6, Channel::LongTerm).await?),
        false => None,
    };
    let step = next_step(
        &state,
        &target,
        newest_v6.as_ref().unwrap_or(&target),
        &wanted,
    );
    // the uploads go to the root, in RAM on a device with 16 MiB of flash (1og-sw2, a CRS with
    // 2.8 MiB of flash free, takes its 11.7 MiB update there)
    let in_ram = progress::root_in_ram_of(&files);
    let free_space = resource.as_ref().and_then(|resource| match in_ram {
        true => resource.free_memory,
        false => resource.free_hdd_space,
    });
    let partition = partitions::other(&partitions::list(raw).await).is_some();
    let left = leftovers::read_with_files(raw, &files).await?;
    let mut download = Some(0);
    for file in step.iter().flat_map(|step| step.files.iter()) {
        let known = sizes.lock().ok().and_then(|sizes| sizes.get(file).copied());
        let size = match known {
            Some(size) => size,
            None => {
                let version = step.as_ref().map(|step| &step.version).unwrap_or(&target);
                let size = package_size(version, file).await.ok().flatten();
                if let Ok(mut sizes) = sizes.lock() {
                    sizes.insert(file.clone(), size);
                }
                size
            }
        };
        download = download.zip(size).map(|(sum, size)| sum + size);
    }
    Ok(Read {
        state,
        step,
        download,
        free_space,
        in_ram,
        partition,
        leftovers: left.apply_fallback.is_some() || left.update_fallback.is_some(),
    })
}

fn planned_device(
    device: &DeviceAccess,
    read: Result<Read, Error>,
    depth: Option<u32>,
) -> PlannedDevice {
    let neighbor = !update_fallback::neighbors(device).is_empty();
    let mut planned = PlannedDevice {
        id: device.id().0,
        name: device.name().into(),
        site_id: device.site().map(|site| site.id().0).unwrap_or_default(),
        site: device
            .site()
            .map(|site| site.name().into())
            .unwrap_or_default(),
        version: None,
        architecture: None,
        files: Box::default(),
        step: None,
        download: None,
        free_space: None,
        in_ram: false,
        fallback: if neighbor {
            RolloutFallback::Neighbor
        } else {
            RolloutFallback::None
        },
        skip: None,
        error: None,
        pilot: false,
        depth,
        related: Box::default(),
        cut_by: Box::default(),
    };
    let read = match read {
        Ok(read) => read,
        Err(error) => {
            planned.skip = Some(RolloutSkip::Unreachable);
            planned.error = Some(ErrorInfo::from(&error));
            return planned;
        }
    };
    planned.version = Some(read.state.version.to_string().into());
    planned.architecture = Some(update::architecture_name(read.state.architecture).into());
    planned.free_space = read.free_space.map(ScalarU64);
    planned.in_ram = read.in_ram;
    if read.partition {
        planned.fallback = RolloutFallback::Partition;
    }
    let Some(step) = read.step else {
        planned.skip = Some(RolloutSkip::UpToDate);
        return planned;
    };
    planned.step = Some(step.version.to_string().into());
    planned.files = step.files;
    planned.download = read.download.map(ScalarU64);
    planned.skip = if read.leftovers {
        Some(RolloutSkip::Leftovers)
    } else if planned.fallback == RolloutFallback::None {
        Some(RolloutSkip::NoFallback)
    } else if matches!((read.download, read.free_space), (Some(need), Some(free)) if need > free) {
        Some(RolloutSkip::NoSpace)
    } else {
        None
    };
    planned
}

/// The access points of the WLAN groups `device` is the controller of.
fn caps_of(device: &DeviceAccess) -> BTreeSet<Box<str>> {
    device
        .topology()
        .list_wlan_groups()
        .filter(|group| group.controller().is_some_and(|c| c.id() == device.id()))
        .flat_map(|group| group.aps().into_vec())
        .filter(|ap| ap.id() != device.id())
        .map(|ap| ap.name().into())
        .collect()
}

/// A device as the order sees it.
#[derive(Debug, Clone, Default)]
struct Candidate {
    name: Box<str>,
    architecture: Box<str>,
    depth: Option<u32>,
    /// no device is reached only through it, it powers none
    leaf: bool,
    /// the access points it is the CAPsMAN controller of
    caps: BTreeSet<Box<str>>,
    /// it is to be updated
    update: bool,
}

/// A candidate in the [`order`] of the update.
struct InOrder {
    /// its place in the candidates
    index: usize,
    pilot: bool,
}

/// The order of the candidates to update, by index, with whether each is a pilot: one per
/// architecture first (one nothing depends on where there is one), then the rest from the
/// outside in (the farthest from a root first, one off the ways first of all), a controller
/// after its access points.
fn order(candidates: &[Candidate]) -> Vec<InOrder> {
    let mut rest = (0..candidates.len())
        .filter(|index| candidates[*index].update)
        .collect::<Vec<_>>();
    // stable: ties stay in NetBox's order of the names
    rest.sort_by_key(|index| std::cmp::Reverse(candidates[*index].depth.unwrap_or(u32::MAX)));
    let mut pilots = Vec::new();
    let architectures = rest
        .iter()
        .map(|index| candidates[*index].architecture.clone())
        .collect::<BTreeSet<_>>();
    for architecture in architectures {
        let of_it = rest
            .iter()
            .copied()
            .filter(|index| candidates[*index].architecture == architecture)
            .collect::<Vec<_>>();
        let pilot = of_it
            .iter()
            .copied()
            .find(|index| candidates[*index].leaf)
            .unwrap_or(of_it[0]);
        pilots.push(pilot);
    }
    pilots.sort_by_key(|pilot| rest.iter().position(|index| index == pilot));
    rest.retain(|index| !pilots.contains(index));
    // a controller after the last of its access points
    let mut ordered: Vec<usize> = Vec::new();
    let mut waiting = Vec::new();
    for index in rest {
        waiting.push(index);
        loop {
            let ready = waiting.iter().position(|controller| {
                candidates[*controller].caps.iter().all(|cap| {
                    !waiting
                        .iter()
                        .any(|other| other != controller && candidates[*other].name == *cap)
                        && !(0..candidates.len()).any(|other| {
                            candidates[other].update
                                && candidates[other].name == *cap
                                && !ordered.contains(&other)
                                && !waiting.contains(&other)
                                && !pilots.contains(&other)
                        })
                })
            });
            match ready {
                Some(at) => ordered.push(waiting.remove(at)),
                None => break,
            }
        }
    }
    ordered.extend(waiting);
    pilots
        .into_iter()
        .map(|index| InOrder { index, pilot: true })
        .chain(ordered.into_iter().map(|index| InOrder {
            index,
            pilot: false,
        }))
        .collect()
}

pub mod run;
pub mod status;

#[cfg(test)]
mod test;
