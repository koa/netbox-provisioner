//! Scope: Whether a device reaches the internet over IPv4 and IPv6, by `/ping` from it to the
//! targets of `config.yaml`; not what follows from it (the batch's condition, `batch`).
use crate::config::CONFIG;
use async_graphql::SimpleObject;
use mikrotik_model::command::Reads;
use std::net::IpAddr;

/// Which families a device reached the internet over: at least one of its targets answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InternetReached {
    pub v4: bool,
    pub v6: bool,
}

/// What the device reached of the internet before the batch's scripts, or `after_script`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct InternetChecked {
    pub v4: bool,
    pub v6: bool,
    pub after_script: bool,
}

impl InternetChecked {
    pub fn of(reached: InternetReached, after_script: bool) -> Self {
        InternetChecked {
            v4: reached.v4,
            v6: reached.v6,
            after_script,
        }
    }
}

impl InternetReached {
    /// Both families answered; IPv6 counts as much as IPv4.
    pub fn both(&self) -> bool {
        self.v4 && self.v6
    }
}

/// Pings the targets from the device, one after the other (one device's reads stay
/// sequential), each family until one answers.
pub async fn reached(device: &impl Reads) -> InternetReached {
    let targets = CONFIG.internet_targets();
    InternetReached {
        v4: any_answers(device, targets.iter().filter(|target| target.is_ipv4())).await,
        v6: any_answers(device, targets.iter().filter(|target| target.is_ipv6())).await,
    }
}

async fn any_answers<'a>(device: &impl Reads, targets: impl Iterator<Item = &'a IpAddr>) -> bool {
    for target in targets {
        let target = *target;
        if super::access::answering(device, &[target])
            .await
            .contains(&target)
        {
            return true;
        }
    }
    false
}
