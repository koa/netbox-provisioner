pub const AGAIN: &str = "Erneut anmelden";

pub fn failed(failure: &str) -> String {
    format!("Anmeldung fehlgeschlagen: {failure}")
}
