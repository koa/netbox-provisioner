use crate::{Error, config::CONFIG, device::AccessibleDevice};
use async_graphql::Object;
use mikrotik_model::{MikrotikDevice, ascii::AsciiString, hwconfig::DeviceType, mikrotik_model};
use tokio::sync::{MappedMutexGuard, MutexGuard};

impl AccessibleDevice {
    pub async fn get_default_client(&self) -> Result<MappedMutexGuard<MikrotikDevice>, Error> {
        let mut client_ref = self.default_client.lock().await;
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
