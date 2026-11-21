//! Scope: Tests of `bootstrap`; nothing else.
use super::{ConsoleLogin, LoggedIn, MacTelnetSetup, console_error, hidden, quote, setup_commands};

#[test]
fn recognizes_console_errors() {
    assert_eq!(
        console_error("failure: already have such address\n"),
        Some("failure: already have such address")
    );
    assert_eq!(
        console_error("syntax error (line 1 column 5)"),
        Some("syntax error (line 1 column 5)")
    );
    assert_eq!(console_error(""), None);
    assert_eq!(console_error("  name: lab-core\n"), None);
}

#[test]
fn quotes_console_strings() {
    assert_eq!(quote(r#"a"b\c$d"#), r#"a\"b\\c\$d"#);
}

/// a failing setup command goes into the error, which the log and the UI show
#[test]
fn hides_the_password_in_commands() {
    let command = format!(
        "/user set [find name=\"admin\"] password=\"{}\"",
        quote("p\"w$")
    );
    assert_eq!(
        hidden(&command, "p\"w$").as_ref(),
        "/user set [find name=\"admin\"] password=\"***\""
    );
    let command = "/ip service set api disabled=no";
    assert_eq!(hidden(command, "").as_ref(), command);
}

/// sat-switch back from the lab, its API limited to the lab's addresses: the neighbor's
/// address joins the list, as in this script (run on a CHR 7.24.5: added once, an empty list
/// left open).
#[test]
fn adds_the_neighbor_to_a_limited_api() {
    let setup = MacTelnetSetup {
        macs: Box::new([]),
        port: "ether5".into(),
        address: "172.17.0.87/31".parse().unwrap(),
        on_bridge: false,
        neighbor_address: "172.17.0.86".parse().unwrap(),
        neighbor_port: None,
    };
    let mac = "D4:CA:6D:C4:3A:18".parse().unwrap();
    let login = ConsoleLogin {
        user: "admin",
        how: LoggedIn::WithPassword,
        mac: &mac,
        factory: false,
    };
    let commands = setup_commands(&setup, "admin", "pw", &login);
    let api = commands
        .iter()
        .find(|c| c.contains("available-from"))
        .unwrap();
    if let Ok(path) = std::env::var("SETUP_API_COMMAND") {
        std::fs::write(path, api).unwrap();
    }
    assert!(api.starts_with("{:local s \"172.17.0.86/32\";"), "{api}");
    assert!(api.contains("/ip service set $w address=($a, $s)"), "{api}");
}
