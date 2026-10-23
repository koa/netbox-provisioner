/// Why the provisioner needs a feature of the device mode (`DeviceModeReason`).
pub enum DeviceModeNeed<'a> {
    ApplyFallback,
    ManagementProxy,
    UpdateFallback,
    Other(&'a str),
}

/// a feature of the device mode and why it is needed, e.g. `partitions (für …)`
pub fn device_mode_feature(feature: &str, need: DeviceModeNeed) -> String {
    let need = match need {
        DeviceModeNeed::ApplyFallback => "für den Fallback beim Anwenden",
        DeviceModeNeed::ManagementProxy => "für den Management-Proxy",
        DeviceModeNeed::UpdateFallback => "für den Fallback bei Updates: zwei Partitionen",
        DeviceModeNeed::Other(other) => other,
    };
    format!("{feature} ({need})")
}

pub const CANCEL: &str = "Abbrechen";
/// a list without entries
pub const NONE: &str = "keine";

/// `items` joined, or [`NONE`].
pub fn list(items: &[String]) -> String {
    if items.is_empty() {
        NONE.into()
    } else {
        items.join(", ")
    }
}
