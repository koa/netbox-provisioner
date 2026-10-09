use crate::device::ros::l2::{EndpointNameGenerator, L2Setup, NameGenerator};
use crate::device::ros::{WifiDeviceDataCurrent, WifiDeviceDataTarget};
use crate::{
    Error,
    device::{
        AccessibleDevice, GraphqlSystemRouterboard, PingResult,
        ros::{
            BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError, W60GDeviceDataCurrent,
            W60GDeviceDataTarget, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
            hw_facts::build_w60g_ports,
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
    wifi_current: Option<WifiDeviceDataCurrent>,
    wifi_target: Option<WifiDeviceDataTarget>,
    w60g_current: Option<W60GDeviceDataCurrent>,
    w60g_target: Option<W60GDeviceDataTarget>,
}

impl DeviceCfg {
    pub fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        name_generator: N,
    ) -> Result<(), SetupError> {
        self.base_target.set_name_and_loopback(device);

        let l2 = L2Setup::new(device, name_generator)?;

        self.base_target.setup_ip_addresses(&l2)?;
        self.base_target.setup_ospf(device, &l2);
        if let Some(wireless_target) = self.wireless_target.as_mut() {
            wireless_target.generate_from(device, &l2)?;
        }
        if let Some(w60g_target) = self.w60g_target.as_mut() {
            w60g_target.generate_from(device, &l2)?;
        }
        if let Some(wifi_target) = self.wifi_target.as_mut() {
            wifi_target.generate_from(device, &l2)?;
        }
        self.base_target.setup_l2(device, &l2)?;
        Ok(())
    }
    pub fn generate_mutations(&self) -> Result<Box<[ResourceMutation]>, Error> {
        let mutations = self.base_target.generate_mutations(&self.base_current)?;
        let mutations = if let (Some(wireless_target), Some(wireless_current)) =
            (&self.wireless_target, &self.wireless_current)
        {
            let wireless_mutations = wireless_target.generate_mutations(wireless_current)?;
            mutations.into_iter().chain(wireless_mutations).collect()
        } else {
            mutations
        };
        let mutations = if let (Some(wifi_target), Some(wifi_current)) =
            (&self.wifi_target, &self.wifi_current)
        {
            let wireless_mutations = wifi_target.generate_mutations(wifi_current)?;
            mutations.into_iter().chain(wireless_mutations).collect()
        } else {
            mutations
        };
        let mutations = if let (Some(w60g_target), Some(w60g_current)) =
            (&self.w60g_target, &self.w60g_current)
        {
            let w60g_mutations = w60g_target.generate_mutations(w60g_current)?;
            mutations.into_iter().chain(w60g_mutations).collect()
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

impl DeviceCfg {
    /// Fetches a device's current config over an already-connected client.
    /// A free function rather than a method on `AccessibleDevice` (which has
    /// no public constructor outside this crate) since it never actually
    /// used `AccessibleDevice`'s own state - any reachable `MikrotikDevice`
    /// connection works, which is what the multi-CHR test harness
    /// (`netbox-provisioner/backend/tests/chr_network/`) needs: its nodes
    /// are reached via dynamic hostfwd ports, not `AccessibleDevice::
    /// create_client`'s hardcoded port 8728.
    pub async fn fetch(client: &MikrotikDevice) -> Result<DeviceCfg, SetupError> {
        let routerboard = <SystemRouterboardState as SingleResource>::fetch(client)
            .await?
            .ok_or(SetupError::RouterboardNotDefined)?;
        Self::fetch_with_board_model(client, &routerboard.model.0).await
    }

    /// Like [`Self::fetch`], but takes the board model name directly instead
    /// of reading it from `/system/routerboard` - CHR has no routerboard at
    /// all (it isn't a physical RouterBOARD), so the multi-CHR test harness
    /// (`netbox-provisioner/backend/tests/chr_network/`) needs to supply the
    /// board it's emulating itself rather than have it detected live.
    pub async fn fetch_with_board_model(
        client: &MikrotikDevice,
        model: &[u8],
    ) -> Result<DeviceCfg, SetupError> {
        let installed_packages = collect_resource::<SystemPackageState>(client)
            .await?
            .into_iter()
            .filter(|p| !p.disabled)
            .map(|p| p.name.0)
            .collect::<HashSet<_>>();

        let current = BaseDeviceDataCurrent::fetch(client).await?;
        let target = BaseDeviceDataTarget::new(model)?;
        let (wireless_current, wireless_target) =
            if installed_packages.contains(b"wireless".as_ref()) {
                (
                    Some(WirelessDeviceDataCurrent::fetch(client).await?),
                    Some(WirelessDeviceDataTarget::detect_device(client).await?),
                )
            } else {
                (None, None)
            };
        let (wifi_current, wifi_target) = if installed_packages.contains(b"wifi-qcom".as_ref()) {
            let current_wifi = WifiDeviceDataCurrent::fetch(client).await?;
            let target = WifiDeviceDataTarget::detect_device(client).await?;

            (Some(current_wifi), Some(target))
        } else {
            (None, None)
        };
        // The board, not a package, decides: the 60 GHz radios have their own menu
        // that only exists on 60 GHz hardware.
        let (w60g_current, w60g_target) = if build_w60g_ports(model).is_empty() {
            (None, None)
        } else {
            (
                Some(W60GDeviceDataCurrent::fetch(client).await?),
                Some(W60GDeviceDataTarget::detect_device(client).await?),
            )
        };
        Ok(DeviceCfg {
            base_current: current,
            base_target: target,
            wireless_current,
            wireless_target,
            wifi_current,
            wifi_target,
            w60g_current,
            w60g_target,
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
        Ok(DeviceCfg::fetch(&self.client).await?)
    }
    async fn generate_cfg(&self) -> Result<Box<str>, Error> {
        let mut device_cfg = DeviceCfg::fetch(&self.client).await?;
        device_cfg.generate_from(&self.device_config, EndpointNameGenerator)?;

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
