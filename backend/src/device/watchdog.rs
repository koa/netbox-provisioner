//! The watchdogs of the devices powering a device that updates. A device with
//! `/system/watchdog watch-address` reboots when that address stops answering for a while. It
//! does so when the address is behind the updating device, which is cut off while it installs
//! its packages. Rebooting a device it powers over PoE cuts that device's power then and can
//! brick it (burri-schopf, a wAP 60G fed by berghof-burri). The power leases
//! ([`power`](super::power)) only hold back reboots the provisioner sends itself.
//!
//! [`read`] shows the watch address of each supplier before the update. Asked to, the update
//! [`suspend`]s them for its duration (`watch-address=none`) and [`restore`]s them afterwards,
//! whatever the update's outcome. In case the provisioner doesn't get that far, a scheduler
//! entry on the supplier restores the address by itself [`SAFETY_NET`] later; it is added
//! before the address goes and removed after it is back.
use crate::{
    Error,
    device::raw::{command, field, rows},
    error_info::ErrorInfo,
    graphql::scalars::ScalarDuration,
    job::Log,
};
use async_graphql::SimpleObject;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::time::Duration;

/// The name of the scheduler entry restoring the watch address.
const NAME: &str = "netbox-provisioner-watchdog";
/// Longer than any update takes: two steps (RouterOS 6 to 7), each with the update fallback's
/// wait, then the firmware.
pub const SAFETY_NET: Duration = Duration::from_secs(2 * 60 * 60);

/// The watchdog of a device powering the one the page is about.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SupplierWatchdog {
    pub device: Box<str>,
    /// the address it watches; `None` without one
    pub watch_address: Option<Box<str>>,
    /// why it couldn't be read
    pub problem: Option<ErrorInfo>,
}

/// The update suspended the watchdog of `device`, which powers the updating one; the scheduler
/// entry restores it after `safety_net` should the provisioner not.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogSuspended {
    pub device: Box<str>,
    pub watch_address: Box<str>,
    pub safety_net: ScalarDuration,
}

/// The watchdog of `device` watches `watch_address` again.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogRestored {
    pub device: Box<str>,
    pub watch_address: Box<str>,
}

/// The watchdog of `device` couldn't be restored; its scheduler entry does it after the
/// safety net.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogNotRestored {
    pub device: Box<str>,
    pub watch_address: Box<str>,
    pub error: ErrorInfo,
}

/// The address the watchdog watches, `None` without one. While a scheduler entry of a suspend
/// the provisioner didn't see to the end is there, the address it restores.
pub async fn read(device: &RawDevice<SimpleResult>) -> Result<Option<Box<str>>, Error> {
    if let Some(address) = pending(device).await? {
        return Ok(Some(address));
    }
    let rows = rows(device, "/system/watchdog/print").await?;
    Ok(rows
        .first()
        .and_then(|row| field(row, "watch-address"))
        .filter(|address| !address.is_empty() && *address != "none")
        .map(Box::from))
}

/// Sets `watch-address=none` and schedules restoring it after `safety_net`; the address it
/// watched, `None` (and nothing changed) without one.
pub async fn suspend(
    device: &RawDevice<SimpleResult>,
    safety_net: Duration,
) -> Result<Option<Box<str>>, Error> {
    let Some(address) = read(device).await? else {
        return Ok(None);
    };
    remove_entry(device).await?;
    command(
        device,
        "/system/scheduler/add",
        &[
            ("name", NAME),
            ("interval", &format!("{}s", safety_net.as_secs().max(1))),
            ("on-event", &script(&address)),
            (
                "comment",
                "restores the watchdog's watch-address suspended for an update of a device this one powers",
            ),
        ],
    )
    .await?;
    set_watch_address(device, "none").await?;
    Ok(Some(address))
}

/// Watches `address` again and removes the scheduler entry.
pub async fn restore(device: &RawDevice<SimpleResult>, address: &str) -> Result<(), Error> {
    set_watch_address(device, address).await?;
    remove_entry(device).await
}

/// Restores the address and removes its own entry; it checks the entry first, since removing a
/// scheduler entry doesn't stop the script it runs.
fn script(address: &str) -> String {
    format!(
        ":if ([:len [/system scheduler find name=\"{NAME}\"]] > 0) do={{ \
         /system watchdog set watch-address={address}; \
         /system scheduler remove [find name=\"{NAME}\"] }}"
    )
}

/// The address in the script of a scheduler entry left from a suspend.
fn restored_by(script: &str) -> Option<&str> {
    let (_, rest) = script.split_once("watch-address=")?;
    rest.split([';', ' ']).next().filter(|address| !address.is_empty())
}

async fn pending(device: &RawDevice<SimpleResult>) -> Result<Option<Box<str>>, Error> {
    Ok(entries(device)
        .await?
        .iter()
        .find_map(|row| field(row, "on-event").and_then(restored_by).map(Box::from)))
}

async fn entries(device: &RawDevice<SimpleResult>) -> Result<Vec<crate::device::raw::Row>, Error> {
    Ok(rows(device, "/system/scheduler/print")
        .await?
        .into_iter()
        .filter(|row| field(row, "name") == Some(NAME))
        .collect())
}

async fn remove_entry(device: &RawDevice<SimpleResult>) -> Result<(), Error> {
    for row in entries(device).await? {
        if let Some(id) = field(&row, ".id") {
            command(device, "/system/scheduler/remove", &[(".id", id)]).await?;
        }
    }
    Ok(())
}

async fn set_watch_address(device: &RawDevice<SimpleResult>, address: &str) -> Result<(), Error> {
    command(device, "/system/watchdog/set", &[("watch-address", address)]).await?;
    Ok(())
}

/// A supplier whose watchdog an update suspended.
pub struct Suspended<L> {
    pub device: Box<str>,
    pub link: L,
    pub watch_address: Box<str>,
}

/// Suspends the watchdog of each supplier that has one, before an update; a supplier refusing
/// stops it, with those suspended so far restored.
pub async fn suspend_all<L: super::update::DeviceLink>(
    suppliers: Vec<(Box<str>, L)>,
    log: &Log,
) -> Result<Vec<Suspended<L>>, Error> {
    let mut suspended = Vec::new();
    for (device, link) in suppliers {
        let result = async { suspend(&link.api().await?, SAFETY_NET).await }.await;
        match result {
            Ok(Some(watch_address)) => {
                log.emit(WatchdogSuspended {
                    device: device.clone(),
                    watch_address: watch_address.clone(),
                    safety_net: SAFETY_NET.into(),
                });
                suspended.push(Suspended {
                    device,
                    link,
                    watch_address,
                });
            }
            Ok(None) => {}
            Err(error) => {
                restore_all(suspended, log).await;
                return Err(Error::WatchdogNotSuspended {
                    device,
                    cause: error.to_string().into(),
                });
            }
        }
    }
    Ok(suspended)
}

/// Restores what [`suspend_all`] suspended; one it can't is left to its scheduler entry.
pub async fn restore_all<L: super::update::DeviceLink>(suspended: Vec<Suspended<L>>, log: &Log) {
    for Suspended {
        device,
        link,
        watch_address,
    } in suspended
    {
        match async { restore(&link.api().await?, &watch_address).await }.await {
            Ok(()) => log.emit(WatchdogRestored {
                device,
                watch_address,
            }),
            Err(error) => log.emit(WatchdogNotRestored {
                device,
                watch_address,
                error: (&error).into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{restored_by, script};

    #[test]
    fn the_script_names_the_address_it_restores() {
        assert_eq!(restored_by(&script("172.16.1.2")), Some("172.16.1.2"));
        assert_eq!(restored_by("/system reboot"), None);
    }
}
