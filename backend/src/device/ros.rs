use crate::device::GraphqlSystemRouterboard;
use crate::{config::CONFIG, device::AccessibleDevice, Error};
use async_graphql::{Object, SimpleObject};
use mikrotik_model::resource::SingleResource;
use mikrotik_model::{ascii::AsciiString, hwconfig::DeviceType, mikrotik_model, MikrotikDevice};
use std::borrow::Borrow;
use std::collections::hash_map::Entry;
use std::net::IpAddr;
use std::ops::Deref;

impl AccessibleDevice {
    /*pub async fn get_default_client(&self) -> Result<MappedMutexGuard<MikrotikDevice>, Error> {
        let mut client_ref = self.clients.lock().await;
        //        client_ref.entry()
        if client_ref.is_none() {
            if let Some(credentials) = CONFIG.mikrotik_credentials.get(self.credentials.as_ref()) {
                let connection = MikrotikDevice::connect(
                    (self.address, 8728),
                    credentials.user().as_bytes(),
                    credentials.password().map(|p| p.as_bytes()),
                )
                .await?;
                client_ref.replace(connection);
            }
        }
        MutexGuard::try_map(client_ref, |client| match client {
            None => None,
            Some(c) => Some(c),
        })
        .map_err(|_| Error::MissingCredentials)
    }*/
    pub async fn create_client(
        &self,
        target: Option<IpAddr>,
        credentials: Option<Box<str>>,
    ) -> Result<MikrotikDevice, Error> {
        let addr = target.unwrap_or(self.address);
        let credential_name = credentials.unwrap_or_else(|| self.credentials.clone());
        let key = (addr, credential_name);
        if let Some(credentials) = CONFIG.mikrotik_credentials.get(key.1.as_ref()) {
            let mut client_ref = self.clients.lock().await;
            Ok(match client_ref.entry(key.clone()) {
                Entry::Occupied(e) => e.get().deref().clone(),
                Entry::Vacant(v) => v
                    .insert(
                        MikrotikDevice::connect(
                            (self.address, 8728),
                            credentials.user().as_bytes(),
                            credentials.password().map(|p| p.as_bytes()),
                        )
                        .await?,
                    )
                    .clone(),
            })
        } else {
            Err(Error::MissingCredentials)
        }
    }
    pub async fn fetch_config(&self, client: &MikrotikDevice) -> Result<DeviceCfg, Error> {
        let current = DeviceDataCurrent::fetch(client).await?;
        let target = DeviceDataTarget::detect_device(client).await?;
        Ok(DeviceCfg { current, target })
    }
}

mikrotik_model!(
    name = DeviceData,
    detect = new,
    fields(
        identity(single = "system/identity"),
        ethernet(by_key(path = "interface/ethernet", key = defaultName)),
        bridge(by_key(path = "interface/bridge", key = name)),
        bridge_port(by_id(
            path = "interface/bridge/port",
            keys(bridge, interface)
        ))
    ),
);
impl DeviceDataTarget {
    fn new(device_type: DeviceType) -> Self {
        Self {
            ethernet: device_type
                .build_ethernet_ports()
                .into_iter()
                .map(|e| (e.default_name, e.data))
                .collect(),
            identity: Default::default(),
            bridge: Default::default(),
            bridge_port: Default::default(),
        }
    }
    fn set_identity(&mut self, name: impl Into<AsciiString>) {
        self.identity.name = name.into();
    }
}

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
#[derive(Clone, Debug, SimpleObject)]
pub struct DeviceCfg {
    current: DeviceDataCurrent,
    target: DeviceDataTarget,
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
impl DeviceDataCurrent {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl DeviceDataTarget {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}
