//! Scope: What a reached device answers over GraphQL: its config, state and plans; not the work
//! behind them.
//!
//! What a reached device answers over GraphQL (`AccessibleDevice`, `DeviceCfg`): its config,
//! the generated script, RouterOS versions, partitions, the live checks, each as a fact.
use super::cfg::{DeviceCfg, DeviceStats, GeneratedCfg, render_script};
use crate::facts::{self, FactKey, FactKind, Freshness};
use crate::{
    Error,
    config::CONFIG,
    device::{
        AccessibleDevice, PingResult, device_mode, neighbors, partitions, raw,
        ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, fallback, progress},
        update,
    },
    error_info::ApiError,
};
use async_graphql::{Context, Object, SimpleObject};
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::{hwconfig::DeviceType, model::SystemIdentityCfg};
use std::{sync::Arc, time::Duration};

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
        Ok(DeviceStats::fetch(&self.client).await?)
    }

    async fn config(&self) -> Result<DeviceCfg, ApiError> {
        Ok(DeviceCfg::fetch(&self.client, &self.device_config).await?)
    }
    /// The script that would bring the device in line with NetBox, and what to know before
    /// applying it; `null` with the error in the response's errors when that fails, so a
    /// device the generator fails on still shows the rest (an update may be what it needs).
    async fn generate_cfg(&self, ctx: &Context<'_>) -> Option<Arc<GeneratedCfg>> {
        nullable(
            ctx,
            self.generated_cfg(self.freshness(CONFIG_AGE))
                .await
                .result(),
        )
    }
    /// The device mode and the features the provisioner needs that are off in it; `null`
    /// before RouterOS 7.17, which has no device mode.
    async fn device_mode(&self, ctx: &Context<'_>) -> Option<device_mode::DeviceMode> {
        let device = self.duplicate();
        let mode = async move {
            device_mode::read(&device.connect_again().await?, &device.device_config).await
        };
        let fact = facts::get(
            self.fact_key(FactKind::DeviceMode),
            self.freshness(DEVICE_MODE_AGE),
            mode,
        );
        nullable(ctx, fact.await.result()).and_then(|mode| (*mode).clone())
    }
    /// What the device sees at its ports against NetBox's cables there, the worst first: the
    /// device's answer kept (`neighborsAge`), against NetBox as it is now.
    async fn neighbor_findings(&self) -> Result<Vec<neighbors::NeighborFinding>, ApiError> {
        let table = self.neighbor_table().await.result()?;
        Ok(neighbors::evaluate(&self.device_config, &table).findings)
    }
    /// seconds since the device was read for `neighborFindings`
    async fn neighbors_age(&self) -> u64 {
        self.neighbor_table().await.age().as_secs()
    }
    /// The live checks of the interface view (`DeviceAccess.interfaceView`) by row: neighbors,
    /// OSPF, the bridge devices of radio links answering (`interfaceLiveAge`).
    async fn interface_live(
        &self,
    ) -> Result<crate::device::interface_live::InterfaceLive, ApiError> {
        Ok((*self.interface_live_fact().await.result()?).clone())
    }
    /// seconds since the device was read for `interfaceLive`
    async fn interface_live_age(&self) -> u64 {
        self.interface_live_fact().await.age().as_secs()
    }
    /// The ports the device reports feeding something over PoE, and what is at their other
    /// end (helper „PoE“).
    async fn poe_observed(&self) -> Result<Vec<crate::helper::poe::PoeObserved>, ApiError> {
        Ok(crate::helper::poe::observed(&self.connect_again().await?, &self.device_config).await?)
    }
    /// The partitions of the flash; `null` on a device without (16 MiB of flash, a CHR).
    async fn partitions(&self, ctx: &Context<'_>) -> Option<partitions::Partitions> {
        let device = self.duplicate();
        let read = async move { partitions::read(&device.connect_again().await?).await };
        let fact = facts::get(
            self.fact_key(FactKind::Partitions),
            self.freshness(PARTITIONS_AGE),
            read,
        );
        nullable(ctx, fact.await.result()).and_then(|partitions| (*partitions).clone())
    }
    /// The running RouterOS and its packages, and what an update in each channel would do.
    async fn routeros(&self, ctx: &Context<'_>) -> Option<Arc<RouterOsVersions>> {
        let device = self.duplicate();
        let versions = async move {
            let as_of = tokio::time::Instant::now();
            let state = update::read_state(&device.connect_again().await?).await?;
            let wanted = device
                .wanted_packages_for(state.model.as_deref().map(str::as_bytes))
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
            // an update of this run brought the running one: the way back to the one before
            let previous =
                crate::job::version_before_update(device.device_config.id()).filter(|previous| {
                    update::Version::parse(previous).is_some_and(|v| v < state.version)
                });
            Ok::<_, Error>(RouterOsVersions {
                as_of,
                previous,
                adjust_packages,
                version: state.version.to_string().into(),
                packages: state.packages,
                wanted: ["routeros".into()].into_iter().chain(wanted).collect(),
                default_channel: CONFIG.routeros_channel.unwrap_or_default(),
                channels,
            })
        };
        let fact = facts::get(
            self.fact_key(FactKind::RouterosState),
            self.freshness(ROUTEROS_AGE),
            versions,
        );
        nullable(ctx, fact.await.result())
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
        let device = self.duplicate();
        let read = async move {
            let raw = device.connect_again().await?;
            let left = crate::device::leftovers::read(&raw).await?;
            if crate::journal::enabled() {
                // what no job of this provisioner explains goes into the journal (Z31)
                let uptime = fallback::uptime(&raw).await.ok().flatten();
                // where the apply fallback is gone, it may have fired: the marks of its script
                let marks = match left.apply_fallback {
                    Some(_) => Vec::new(),
                    None => progress::contents(&raw).await.unwrap_or_default(),
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
        let fact = facts::get(
            self.fact_key(FactKind::Leftovers),
            self.freshness(LEFTOVERS_AGE),
            read,
        );
        let mut left = (*nullable(ctx, fact.await.result())?).clone();
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
            let rows = raw::rows(&self.connect_again().await?, "/log/print").await?;
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|row| {
                    !raw::field(row, "topics")
                        .unwrap_or_default()
                        .split(',')
                        .any(|t| t == "account")
                })
                .collect();
            let skip = rows.len().saturating_sub(last as usize);
            let text =
                |row: &raw::Row, name| Box::<str>::from(raw::field(row, name).unwrap_or_default());
            Ok::<_, Error>(
                rows[skip..]
                    .iter()
                    .map(|row| LogEntry {
                        time: text(row, "time"),
                        topics: text(row, "topics"),
                        message: text(row, "message"),
                    })
                    .collect(),
            )
        };
        nullable(ctx, read.await)
    }
}

/// How old the generated config may be on the device page: what it showed a minute ago.
const CONFIG_AGE: Duration = Duration::from_secs(60);
/// Marks the facts of a helper's plan (`generating_for`) in their key: not the device's own.
const PLAN: &str = " plan";

/// Whether `key` is about a device as it is, not as a helper's plan would make it.
pub(crate) fn of_device_as_is(key: &FactKey) -> bool {
    !key.detail.contains(PLAN)
}
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
    /// The planned changes as `freshness` takes them (`FactKind::Config`): shared by the
    /// device page and `diffSite`.
    pub async fn generated_cfg(&self, freshness: Freshness) -> facts::Fact<GeneratedCfg> {
        let device = self.duplicate();
        let generated = async move {
            let as_of = tokio::time::Instant::now();
            let mut device_cfg = DeviceCfg::fetch(&device.client, &device.device_config).await?;
            device_cfg
                .generate_live(&device.device_config, &device.connect_again().await?)
                .await?;
            let (script, menus) = {
                let mutations = device_cfg.sorted_mutations()?;
                (
                    render_script(&mutations)?,
                    crate::device::site_diff::menu_changes(&mutations),
                )
            };
            Ok(GeneratedCfg {
                script,
                menus,
                warnings: device_cfg
                    .warnings(async || device.connect_again::<SimpleResult>().await)
                    .await,
                as_of,
            })
        };
        facts::get(self.fact_key(FactKind::Config), freshness, generated).await
    }
    /// The live checks (`FactKind::InterfaceLive`): NetBox changing drops them, they compare
    /// with it.
    async fn interface_live_fact(
        &self,
    ) -> facts::Fact<crate::device::interface_live::InterfaceLive> {
        let device = self.duplicate();
        let read = async move {
            crate::device::interface_live::read(
                &device.connect_again().await?,
                &device.device_config,
            )
            .await
        };
        facts::get(
            self.fact_key(FactKind::InterfaceLive),
            self.freshness(INTERFACE_LIVE_AGE),
            read,
        )
        .await
    }
    /// What the device answers for the neighbor check (`FactKind::Neighbors`).
    async fn neighbor_table(&self) -> facts::Fact<neighbors::NeighborTable> {
        let device = self.duplicate();
        let read = async move { neighbors::read_table(&device.connect_again().await?).await };
        facts::get(
            self.fact_key(FactKind::Neighbors),
            self.freshness(NEIGHBORS_AGE),
            read,
        )
        .await
    }
    /// The key of a fact about the device as this connection reads it: the credentials go in,
    /// what typed-in ones read stays with them.
    fn fact_key(&self, kind: FactKind) -> FactKey {
        FactKey::new(
            kind,
            Some(self.device_config.id()),
            format!(
                "{} {}{}",
                self.address,
                self.credentials.fact_key(),
                // a helper's plan: of that topology only
                if self.preview {
                    format!("{PLAN} {:p}", Arc::as_ptr(self.device_config.topology()))
                } else {
                    String::new()
                }
            ),
        )
        .via(self.path.label())
    }
    /// `own`, or the younger age the request asked for (`access(maxAge:)`).
    fn freshness(&self, own: Duration) -> Freshness {
        Freshness::MaxAge(own).at_most(self.max_age)
    }
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
    channels: Vec<ChannelUpdate>,
    /// whether the packages differ from `wanted`, which an update without channel fixes at
    /// the running version
    adjust_packages: bool,
    /// the older version the device ran before an update of this run of the provisioner
    /// (`downgradeRouteros` takes it back there)
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
