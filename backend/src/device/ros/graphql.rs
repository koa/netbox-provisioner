//! Scope: What a reached device answers over GraphQL: its config, state and plans; not the work
//! behind them.
//!
//! What a reached device answers over GraphQL (`AccessibleDevice`, `DeviceCfg`): its config,
//! the generated script, RouterOS versions, partitions, the live checks, each as a fact.
use super::cfg::{DeviceCfg, DeviceStats, GeneratedCfg, render_script};
use crate::facts::{self, FactKey, FactKind, Freshness};
use crate::topology::access::device::DeviceAccess;
use crate::{
    Error,
    config::CONFIG,
    device::{
        AccessibleDevice, Password, PingResult,
        access::Reader,
        device_mode, neighbors, partitions,
        ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, fallback, progress},
        update,
    },
    error_info::ApiError,
};
use async_graphql::{Context, Object, SimpleObject};
use mikrotik_model::{
    hwconfig::DeviceType,
    model::{LogState, LogTopic, SystemIdentityCfg, SystemRouterboardState},
    resource::collect_resource,
    value::RosValue,
};
use std::{future::Future, sync::Arc, time::Duration};

pub struct GraphqlDeviceType(DeviceType);
impl From<DeviceType> for GraphqlDeviceType {
    fn from(value: DeviceType) -> Self {
        GraphqlDeviceType(value)
    }
}
#[Object]
impl GraphqlDeviceType {
    async fn name(&self) -> &str {
        self.0.device_type_name()
    }
}

struct GraphqlSystemIdentity<'a>(&'a SystemIdentityCfg);
#[Object]
impl GraphqlSystemIdentity<'_> {
    async fn name(&self) -> String {
        self.0.name.to_string()
    }
}

#[Object]
impl DeviceCfg {
    async fn current(&self) -> &BaseDeviceDataCurrent {
        &self.base_current
    }
    async fn target(&self) -> &BaseDeviceDataTarget {
        &self.base_target
    }
}

#[Object]
impl BaseDeviceDataCurrent {
    async fn identity(&self) -> GraphqlSystemIdentity<'_> {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl BaseDeviceDataTarget {
    async fn identity(&self) -> GraphqlSystemIdentity<'_> {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl AccessibleDevice {
    async fn ping(&self, count: Option<u8>) -> Result<Box<[PingResult]>, ApiError> {
        Ok(self
            .simple_ping(count.unwrap_or(1))
            .await
            .map_err(Error::from)?)
    }

    /// The device the connection actually reached, to tell it from another one answering at
    /// the same address.
    async fn connected(&self) -> crate::device::Connected {
        self.connected_device()
    }

    async fn device_stats(&self) -> Result<DeviceStats, ApiError> {
        let reader = self.reading(ROUTEROS_AGE);
        Ok(DeviceStats::fetch(&reader, &self.device_config).await?)
    }

    async fn config(&self) -> Result<DeviceCfg, ApiError> {
        let reader = self.reading(CONFIG_AGE);
        Ok(DeviceCfg::fetch(&reader, &self.device_config).await?)
    }
    /// The script that would bring the device in line with NetBox, and what to know before
    /// applying it; `null` with the error in the response's errors when that fails, so a
    /// device the generator fails on still shows the rest (an update may be what it needs).
    async fn generate_cfg(&self, ctx: &Context<'_>) -> Option<Arc<GeneratedCfg>> {
        nullable(
            ctx,
            self.generated_cfg(self.freshness(CONFIG_AGE))
                .await
                .map(Arc::new),
        )
    }
    /// The device mode and the features the provisioner needs that are off in it; `null`
    /// before RouterOS 7.17, which has no device mode.
    async fn device_mode(&self, ctx: &Context<'_>) -> Option<device_mode::DeviceMode> {
        let reader = self.reading(DEVICE_MODE_AGE);
        nullable(ctx, device_mode::read(&reader, &self.device_config).await).flatten()
    }
    /// What the device sees at its ports against NetBox's cables there, the worst first: the
    /// device's answer kept (`neighborsAge`), against NetBox as it is now.
    async fn neighbor_findings(&self) -> Result<Vec<neighbors::NeighborFinding>, ApiError> {
        let table = neighbors::read_table(&self.reading(NEIGHBORS_AGE)).await?;
        Ok(neighbors::evaluate(&self.device_config, &table).findings)
    }
    /// seconds since the device was read for `neighborFindings`
    async fn neighbors_age(&self) -> u64 {
        self.age_of(NEIGHBORS_AGE, |reader| async move {
            neighbors::read_table(&reader).await.map(|_| reader)
        })
        .await
    }
    /// The live checks of the interface view (`DeviceAccess.interfaceView`) by row: neighbors,
    /// OSPF, the bridge devices of radio links answering (`interfaceLiveAge`).
    async fn interface_live(
        &self,
    ) -> Result<crate::device::interface_live::InterfaceLive, ApiError> {
        let reader = self.reading(INTERFACE_LIVE_AGE);
        Ok(crate::device::interface_live::read(&reader, &self.device_config).await?)
    }
    /// seconds since the device was read for `interfaceLive`
    async fn interface_live_age(&self) -> u64 {
        let device = &self.device_config;
        self.age_of(INTERFACE_LIVE_AGE, |reader| async move {
            crate::device::interface_live::read(&reader, device)
                .await
                .map(|_| reader)
        })
        .await
    }
    /// The ports the device reports feeding something over PoE, and what is at their other
    /// end (helper „PoE“).
    async fn poe_observed(&self) -> Result<Vec<crate::device::poe::PoeObserved>, ApiError> {
        Ok(crate::device::poe::observed(self.reader(), &self.device_config).await?)
    }
    /// The partitions of the flash; `null` on a device without (16 MiB of flash, a CHR).
    async fn partitions(&self, ctx: &Context<'_>) -> Option<partitions::Partitions> {
        let reader = self.reading(PARTITIONS_AGE);
        nullable(ctx, partitions::read(&reader).await).flatten()
    }
    /// The running RouterOS and its packages, and what an update in each channel would do.
    async fn routeros(&self, ctx: &Context<'_>) -> Option<RouterOsVersions> {
        let reader = self.reading(ROUTEROS_AGE);
        let device = self;
        let versions = async {
            let state = update::read_state(&reader).await?;
            let wanted = device
                .wanted_packages_for(&reader, state.model.as_deref().map(str::as_bytes))
                .await?;
            let newest_v6 = if state.version.major() < 7 {
                Some(newest(6, update::Channel::LongTerm).await?)
            } else {
                None
            };
            let mut channels = Vec::new();
            for channel in [
                update::Channel::LongTerm,
                update::Channel::Stable,
                update::Channel::Testing,
                update::Channel::Development,
            ] {
                let newest = newest(7, channel).await?;
                let step = update::next_step(
                    &state,
                    &newest,
                    newest_v6.as_ref().unwrap_or(&newest),
                    &wanted,
                );
                channels.push(ChannelUpdate {
                    channel,
                    newest: newest.to_string().into(),
                    upgrade: newest > state.version,
                    update: step.is_some(),
                });
            }
            // RouterOS 6 goes to 7 for its packages (`next_step`)
            let adjust_packages = state.version.major() >= 7
                && update::next_step(&state, &state.version, &state.version, &wanted).is_some();
            // an update of this run brought the running one: the way back to the one before,
            // unless a downgrade can't go there (from RouterOS 6, lab-crs226)
            let previous =
                crate::job::version_before_update(device.device_config.id()).filter(|previous| {
                    update::Version::parse(previous)
                        .is_some_and(|v| update::downgradable(&state.version, &v))
                });
            Ok::<_, Error>(RouterOsVersions {
                as_of: reader.as_of(),
                previous,
                adjust_packages,
                version: state.version.to_string().into(),
                packages: state.packages,
                wanted: ["routeros".into()].into_iter().chain(wanted).collect(),
                default_channel: CONFIG.routeros_channel.unwrap_or_default(),
                channels: channels.into(),
            })
        };
        nullable(ctx, versions.await)
    }
    /// What a job that didn't see its work to the end left on the device, with what can be
    /// done about it; `null` with nothing left, and while a job of this provisioner runs on
    /// the device (what it set up isn't left over).
    async fn leftovers(&self, ctx: &Context<'_>) -> Option<crate::device::leftovers::Leftovers> {
        let subject = crate::job::JobSubject::Device(self.device_config.id());
        if crate::job::list(Some(subject))
            .iter()
            .any(|job| job.is_running())
        {
            return None;
        }
        let reader = self.reading(LEFTOVERS_AGE);
        let device = self;
        let read = async {
            let raw = &reader;
            let left = crate::device::leftovers::read(raw).await?;
            if crate::journal::enabled() {
                // what no job of this provisioner explains goes into the journal (Z31)
                let uptime = fallback::uptime(raw).await.ok().flatten();
                // where the apply fallback is gone, it may have fired: the marks of its script
                let marks = match left.apply_fallback {
                    Some(_) => Vec::new(),
                    None => progress::contents(raw).await.unwrap_or_default(),
                };
                tokio::spawn(crate::journal::observe(
                    device.device_config.id(),
                    left.journal_left(),
                    uptime,
                    marks,
                ));
            }
            Ok::<_, Error>(left)
        };
        let mut left = nullable(ctx, read.await)?;
        // which job holds them changes without the device knowing
        if let Some(watchdogs) = &mut left.watchdogs {
            watchdogs.held_by = crate::job::holding_watchdogs(self.device_config.name());
        }
        Some(left).filter(|left| !left.is_empty())
    }
    /// The newest `last` entries of the device's log (`/log`, kept in its memory until a
    /// reboot), oldest first: what happened, after an update that went wrong. Without the
    /// logins (topic `account`): the provisioner's own, several per page, drowned the rest.
    async fn log(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 30)] last: u32,
    ) -> Option<Vec<LogEntry>> {
        let read = async {
            let entries: Vec<_> = collect_resource::<LogState>(self.reader())
                .await?
                .into_iter()
                .filter(|entry| !entry.topics.contains(&LogTopic::Account))
                .collect();
            let skip = entries.len().saturating_sub(last as usize);
            Ok::<_, Error>(
                entries[skip..]
                    .iter()
                    .map(|entry| LogEntry {
                        time: entry.time.to_string().into(),
                        topics: String::from_utf8_lossy(&entry.topics.encode_ros()).into(),
                        message: entry.message.to_string().into(),
                    })
                    .collect(),
            )
        };
        nullable(ctx, read.await)
    }
}

/// How old the generated config may be on the device page: what it showed a minute ago.
const CONFIG_AGE: Duration = Duration::from_secs(60);
/// How old RouterOS version and packages may be: only jobs change them, and those drop them.
const ROUTEROS_AGE: Duration = Duration::from_secs(10 * 60);
const PARTITIONS_AGE: Duration = Duration::from_secs(10 * 60);
/// How old the live checks of the interface view may be: shown with their time, „Erneut
/// prüfen“ reads anew (`refreshInterfaceLive`), jobs on the device and NetBox changing drop them.
const INTERFACE_LIVE_AGE: Duration = Duration::from_secs(60);
/// How old the neighbors a device sees may be: a cable seldom moves, „Nochmals prüfen“ reads
/// anew (`refreshNeighbors`).
const NEIGHBORS_AGE: Duration = Duration::from_secs(10 * 60);
const DEVICE_MODE_AGE: Duration = Duration::from_secs(10 * 60);
/// How old what jobs left may be: a fallback's next run moves on.
const LEFTOVERS_AGE: Duration = Duration::from_secs(60);
/// How old MikroTik's newest version in a channel may be: it publishes seldom.
const NEWEST_AGE: Duration = Duration::from_secs(15 * 60);

/// The newest RouterOS `major` in `channel` (`update::newest`), as a fact for every device.
async fn newest(major: u32, channel: update::Channel) -> Result<update::Version, Error> {
    let key = FactKey::new(
        FactKind::NewestVersion,
        None,
        format!("{major} {}", channel.name()),
    );
    let fact = facts::get(
        key,
        Freshness::MaxAge(NEWEST_AGE),
        update::newest(major, channel),
    )
    .await;
    Ok((*fact.result()?).clone())
}

impl AccessibleDevice {
    /// The planned changes, from what the device answered as `freshness` takes it: shared by
    /// the device page and `diffSite`.
    pub async fn generated_cfg(&self, freshness: Freshness) -> Result<GeneratedCfg, Error> {
        let reader = self.reader().at(freshness);
        generate(&reader, &self.device_config, self.password()).await
    }
    /// A reader of this login taking what it reads as old as `own` (or the younger age the
    /// request asked for).
    fn reading(&self, own: Duration) -> Reader {
        self.reader().at(self.freshness(own))
    }
    /// Seconds since the oldest of what `work` read (with a reader as old as `own`) was asked;
    /// 0 when it failed.
    async fn age_of<F: Future<Output = Result<Reader, Error>>>(
        &self,
        own: Duration,
        work: impl FnOnce(Reader) -> F,
    ) -> u64 {
        match work(self.reading(own)).await {
            Ok(reader) => reader.as_of().elapsed().as_secs(),
            Err(_) => 0,
        }
    }
    /// `own`, or the younger age the request asked for (`access(maxAge:)`).
    fn freshness(&self, own: Duration) -> Freshness {
        Freshness::MaxAge(own).at_most(self.max_age)
    }
}

/// The planned changes of `device` from what `reader` reads, logged in with `password`.
pub(crate) async fn generate(
    reader: &Reader,
    device: &DeviceAccess,
    password: Password,
) -> Result<GeneratedCfg, Error> {
    let mut device_cfg = DeviceCfg::fetch(reader, device).await?;
    device_cfg.generate_live(device, reader).await?;
    let (script, mut menus) = {
        let mutations = device_cfg.sorted_mutations()?;
        (
            render_script(&mutations)?,
            crate::device::site_diff::menu_changes(&mutations),
        )
    };
    let previous_password = password == Password::Previous;
    if previous_password {
        crate::device::site_diff::add_change(&mut menus, "user");
    }
    let www_certificate = device_cfg.www_certificate();
    if www_certificate == crate::device::www_certificate::WwwCertificate::Needed {
        crate::device::site_diff::add_change(&mut menus, "certificate");
    }
    let warnings = device_cfg.warnings(reader).await;
    Ok(GeneratedCfg {
        script,
        menus: menus.into(),
        previous_password,
        www_certificate,
        warnings,
        as_of: reader.as_of(),
    })
}

/// An entry of the device's log, as RouterOS reports it.
#[derive(SimpleObject)]
pub struct LogEntry {
    /// `10:04:12`, or with the date for an older one
    time: Box<str>,
    /// `system,info`
    topics: Box<str>,
    message: Box<str>,
}

/// A field error that leaves the field `null` instead of its parent: async-graphql passes an
/// `Err` of a resolver on to the parent even for a nullable field.
pub(crate) fn nullable<T>(ctx: &Context<'_>, result: Result<T, Error>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            ctx.add_error(ctx.set_error_path(
                async_graphql::Error::from(ApiError(error)).into_server_error(ctx.item.pos),
            ));
            None
        }
    }
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct RouterOsVersions {
    /// when the device was read for it
    #[graphql(skip)]
    as_of: tokio::time::Instant,
    version: Box<str>,
    /// installed packages
    packages: Box<[Box<str>]>,
    /// the packages an update leaves installed
    wanted: Box<[Box<str>]>,
    /// the channel preselected for updates (`routeros_channel` of the config)
    default_channel: update::Channel,
    channels: Box<[ChannelUpdate]>,
    /// whether the packages differ from `wanted`, which an update without channel fixes at
    /// the running version
    adjust_packages: bool,
    /// the older version the device ran before an update of this run of the provisioner, where
    /// `downgradeRouteros` can take it back there (not to RouterOS 6)
    previous: Option<Box<str>>,
}

#[async_graphql::ComplexObject]
impl RouterOsVersions {
    /// seconds since the device was read for it (`facts`)
    async fn age(&self) -> u64 {
        self.as_of.elapsed().as_secs()
    }
}

#[derive(SimpleObject)]
pub struct ChannelUpdate {
    channel: update::Channel,
    /// the newest RouterOS 7 of the channel
    newest: Box<str>,
    /// whether the device runs an older version
    upgrade: bool,
    /// whether an update changes anything (version or packages)
    update: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct GraphqlSystemRouterboard(pub(crate) SystemRouterboardState);

#[Object]
impl GraphqlSystemRouterboard {
    async fn device_type(&self) -> String {
        self.0.model.to_string()
    }
    async fn serial_number(&self) -> String {
        self.0.serial_number.to_string()
    }
    async fn firmware_type(&self) -> String {
        self.0.firmware_type.to_string()
    }
}
