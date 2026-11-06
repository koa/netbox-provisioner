pub const AGAIN: &str = "Erneut anmelden";
/// the server refuses the token on any page (`components::login_expired`)
pub const EXPIRED: &str = "Anmeldung abgelaufen";
pub const EXPIRED_TEXT: &str = "Der Provisioner nimmt die Anmeldung nicht mehr an. Laufende \
    Aufträge gehen auf dem Server weiter; nach dem Anmelden kommt diese Seite wieder.";

pub fn failed(failure: &str) -> String {
    format!("Anmeldung fehlgeschlagen: {failure}")
}
