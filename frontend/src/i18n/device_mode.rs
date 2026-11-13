//! Scope: The words of the device mode's features; nothing else.
use crate::graphql::authenticated::types::{DeviceModeFeature, DeviceModeReason};
use ::i18n::common::{DeviceModeNeed, device_mode_feature};

/// The features of a device-mode change, each with why it is needed (also on the device page).
pub fn features<'a>(
    changes: impl Iterator<Item = (&'a DeviceModeFeature, &'a DeviceModeReason)>,
) -> Vec<String> {
    changes
        .map(|(feature, reason)| {
            device_mode_feature(
                // RouterOS's own names
                match feature {
                    DeviceModeFeature::Scheduler => "scheduler",
                    DeviceModeFeature::Socks => "socks",
                    DeviceModeFeature::Partitions => "partitions",
                    DeviceModeFeature::Other(other) => other,
                },
                match reason {
                    DeviceModeReason::ApplyFallback => DeviceModeNeed::ApplyFallback,
                    DeviceModeReason::ManagementProxy => DeviceModeNeed::ManagementProxy,
                    DeviceModeReason::UpdateFallback => DeviceModeNeed::UpdateFallback,
                    DeviceModeReason::Other(other) => DeviceModeNeed::Other(other),
                },
            )
        })
        .collect()
}
