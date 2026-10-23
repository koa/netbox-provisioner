use crate::components::adjust_target::{SelectedCredentials, SelectedTarget};

pub const SHOW: &str = "Ändern";
pub const HIDE: &str = "Ausblenden";
pub const ACCESS: &str = "Zugang";

/// How the device is reached: as NetBox and the config say, or what was chosen by hand.
pub fn describe(target: &SelectedTarget) -> String {
    let address = match target.address {
        Some(address) => format!("Adresse von Hand: {address}"),
        None => "Adresse aus NetBox".into(),
    };
    let credentials = match &target.credentials {
        SelectedCredentials::Default => "Zugangsdaten des Tenants".into(),
        SelectedCredentials::Named(name) => format!("Zugangsdaten von Hand: „{name}“"),
        SelectedCredentials::Adhoc { username, .. } if username.is_empty() => {
            "Passwort von Hand".into()
        }
        SelectedCredentials::Adhoc { username, .. } => format!("Benutzer von Hand: {username}"),
    };
    format!("{address}, {credentials}")
}
pub const ADDRESS: &str = "IP-Adresse";
pub const ADDRESS_PLACEHOLDER: &str = "Andere IP-Adresse";
pub const INVALID_ADDRESS: &str = "Keine gültige IP-Adresse";
pub const CREDENTIALS: &str = "Hinterlegte Zugangsdaten";
pub const USERNAME: &str = "Benutzername";
pub const PASSWORD: &str = "Passwort";
