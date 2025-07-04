use crate::{
    Error,
    device::{
        AccessibleDevice, GraphqlSystemRouterboard, PingResult,
        ros::{
            BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, WirelessDeviceDataCurrent,
            WirelessDeviceDataTarget,
        },
    },
    topology::access::device::DeviceAccess,
};
use async_graphql::{Object, SimpleObject};
use log::info;
use mikrotik_model::{
    MikrotikDevice,
    generator::Generator,
    hwconfig::DeviceType,
    model::{ReferenceType, SystemIdentityCfg, SystemPackageState, SystemRouterboardState},
    resource::{ResourceMutation, SingleResource, collect_resource},
};
use std::collections::HashSet;
use surge_ping::SurgeError;

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
#[derive(Clone, Debug)]
pub struct DeviceCfg {
    base_current: BaseDeviceDataCurrent,
    base_target: BaseDeviceDataTarget,
    wireless_current: Option<WirelessDeviceDataCurrent>,
    wireless_target: Option<WirelessDeviceDataTarget>,
}

impl DeviceCfg {
    fn generate_from(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        self.base_target.generate_from(device)?;
        if let Some(wireless_target) = self.wireless_target.as_mut() {
            wireless_target.generate_from(device);
        }
        Ok(())
    }
    fn generate_mutations(&self) -> Result<Box<[ResourceMutation]>, Error> {
        let mutations = self.base_target.generate_mutations(&self.base_current)?;
        let mutations = if let (Some(wireless_target), Some(wireless_current)) =
            (&self.wireless_target, &self.wireless_current)
        {
            let wireless_mutations = wireless_target.generate_mutations(wireless_current)?;
            mutations.into_iter().chain(wireless_mutations).collect()
        } else {
            mutations
        };

        Ok(mutations)
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

impl AccessibleDevice {
    pub async fn fetch_config(&self, client: &MikrotikDevice) -> Result<DeviceCfg, SetupError> {
        let installed_packages = collect_resource::<SystemPackageState>(client)
            .await?
            .into_iter()
            .filter(|p| !p.disabled)
            .map(|p| p.name.0)
            .collect::<HashSet<_>>();

        let current = BaseDeviceDataCurrent::fetch(client).await?;
        let target = BaseDeviceDataTarget::detect_device(client).await?;
        let (wireless_current, wireless_target) =
            if installed_packages.contains(b"wireless".as_ref()) {
                let current_wireless = WirelessDeviceDataCurrent::fetch(client).await?;
                let target = WirelessDeviceDataTarget::detect_device(client).await?;
                (Some(current_wireless), Some(target))
            } else {
                (None, None)
            };
        Ok(DeviceCfg {
            base_current: current,
            base_target: target,
            wireless_current,
            wireless_target,
        })
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct DeviceStats {
    routerboard: GraphqlSystemRouterboard,
}
impl DeviceStats {
    pub async fn fetch(client: &MikrotikDevice) -> Result<DeviceStats, Error> {
        Ok(Self {
            routerboard: GraphqlSystemRouterboard(
                SystemRouterboardState::fetch(&client)
                    .await?
                    .expect("system/routerboard not found"),
            ),
        })
    }
}

#[Object]
impl BaseDeviceDataCurrent {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl BaseDeviceDataTarget {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl AccessibleDevice {
    async fn ping(&self, count: Option<u8>) -> Result<Box<[PingResult]>, SurgeError> {
        self.simple_ping(count.unwrap_or(1)).await
    }

    async fn device_stats(&self) -> Result<DeviceStats, Error> {
        DeviceStats::fetch(&self.client).await
    }

    async fn config(&self) -> Result<DeviceCfg, Error> {
        Ok(self.fetch_config(&self.client).await?)
    }
    async fn generate_cfg(&self) -> Result<Box<str>, Error> {
        let mut device_cfg = self.fetch_config(&self.client).await?;
        device_cfg.generate_from(&self.device_config)?;

        let mutations = device_cfg.generate_mutations()?;
        for m in &mutations {
            info!("Mutation generated: {:?}", m);
        }

        let mutations = ResourceMutation::sort_mutations_with_provided_dependencies(
            mutations.as_ref(),
            [
                (ReferenceType::Interface, b"lo".into()),
                (ReferenceType::RoutingTable, b"main".into()),
                (ReferenceType::FirewallChain, b"input".into()),
                (ReferenceType::FirewallChain, b"output".into()),
                (ReferenceType::FirewallChain, b"forward".into()),
            ],
        )?;
        let mut cfg = String::new();
        let mut generator = Generator::new(&mut cfg);
        for mutation in mutations {
            generator.append_mutation(mutation)?;
        }
        Ok(cfg.into_boxed_str())
    }
}
