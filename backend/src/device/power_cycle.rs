//! „Strom aus und ein“: a cold boot of a device powered over PoE (`docs/concepts/update-fallback.md`,
//! decided 2026-10-23): the device feeding it switches PoE on the port cabled to it off and on
//! again (`/interface/ethernet/poe/power-cycle`). Only on a click, with a confirmation, never by
//! itself. It confirms a device mode change (a warm reboot doesn't) and brings back a device
//! that hangs. It waits while the device or one it powers updates: losing power while it
//! installs can brick it ([`PowerChain::power_cut`]).
use crate::{
    Error,
    device::{bootstrap, power::PowerChain, raw::command},
    job::Log,
    topology::{
        access::{AccessTopology, device::DeviceAccess},
        findings::DeviceRef,
    },
};
use async_graphql::SimpleObject;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};

/// How long the port stays without power.
pub const OFF_SECONDS: u32 = 5;

/// The RouterOS device feeding a device over PoE and its port (NetBox's name).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PowerSupply {
    pub device: DeviceRef,
    pub port: Box<str>,
}

/// The power on `port` of `supplier` went off for `seconds` and on again.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PowerCycled {
    pub supplier: Box<str>,
    pub port: Box<str>,
    pub device: Box<str>,
    pub seconds: u32,
}

/// The RouterOS device whose `pse` port is cabled to `device`, that port and its factory name;
/// the first where several are.
pub fn supply(device: &DeviceAccess) -> Option<(DeviceAccess, PowerSupply, Box<str>)> {
    device
        .interfaces()
        .iter()
        .flat_map(|port| port.connected_interfaces())
        .filter(|far| far.enable_poe())
        .find_map(|far| {
            let supplier = far.device().filter(DeviceAccess::has_routeros)?;
            let factory = far.external_port()?.to_string().into_boxed_str();
            let supply = PowerSupply {
                device: DeviceRef {
                    id: supplier.id().0,
                    name: supplier.name().into(),
                    site: supplier.site().map(|site| site.id().0),
                },
                port: far.name().into(),
            };
            Some((supplier, supply, factory))
        })
}

/// Switches the power on the supplier's port with `factory` name off and on again, once
/// neither `power`'s device nor one it powers updates.
pub async fn power_cycle(
    supplier: &RawDevice<SimpleResult>,
    supply: &PowerSupply,
    factory: &str,
    power: &PowerChain,
    device: &str,
    log: &Log,
) -> Result<(), Error> {
    let _cut = log
        .unless_cancelled(power.power_cut(&supply.device.name, log))
        .await?;
    log.check()?;
    let port = bootstrap::port_name(supplier, factory)
        .await?
        .ok_or_else(|| Error::PoePortNotFound {
            device: supply.device.name.clone(),
            port: supply.port.clone(),
        })?;
    command(
        supplier,
        "/interface/ethernet/poe/power-cycle",
        &[("numbers", &port), ("duration", &format!("{OFF_SECONDS}s"))],
    )
    .await?;
    log.emit(PowerCycled {
        supplier: supply.device.name.clone(),
        port: supply.port.clone(),
        device: device.into(),
        seconds: OFF_SECONDS,
    });
    Ok(())
}
