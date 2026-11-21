//! Scope: Tests of `config`; nothing else.
use super::{environment, settings_from};
use config::{File, FileFormat};

const YAML: &str = "
settings:
  auth_client_id: client
  auth_issuer: https://id.example
  auth_users: [someone]
  netbox_url: https://netbox.example
  netbox_token: from-file
  mikrotik_credentials:
    lab:
      user: admin
";

#[test]
fn environment_overrides_single_values() {
    let env = [
        ("APP_SETTINGS__NETBOX_TOKEN", "from-env"),
        ("APP_SETTINGS__ALLOW_APPLY", "true"),
        ("APP_SETTINGS__SERVER_PORT", "9000"),
        ("APP_SETTINGS__MIKROTIK_CREDENTIALS__LAB__PASSWORD", "0123"),
        (
            "APP_SETTINGS__MIKROTIK_CREDENTIALS__LAB__PREVIOUS_PASSWORD",
            "old",
        ),
        ("OTHER_SETTINGS__NETBOX_URL", "ignored"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = settings_from(
        File::from_str(YAML, FileFormat::Yaml),
        environment().source(Some(env)),
    )
    .unwrap();
    assert_eq!(settings.netbox_token, "from-env".into());
    assert_eq!(settings.netbox_url, "https://netbox.example".into());
    assert!(settings.allow_apply);
    assert_eq!(settings.server_port(), 9000);
    let lab = &settings.mikrotik_credentials["lab"];
    assert_eq!((lab.user(), lab.password()), ("admin", Some("0123")));
    assert_eq!(lab.previous_password(), Some("old"));
}

#[test]
fn api_networks_are_prefixes() {
    let yaml = format!("{YAML}  api_networks: [192.168.0.0/16, \"2a02:16a:fe00:8000::/50\"]\n");
    let settings = settings_from(File::from_str(&yaml, FileFormat::Yaml), environment()).unwrap();
    let networks = settings.api_networks.expect("api_networks");
    assert_eq!(networks.len(), 2);
    assert_eq!(networks[1].to_string(), "2a02:16a:fe00:8000::/50");
}

/// The version comes from the file, its secret from the environment (a Secret of the
/// chart); a version without a secret is refused.
#[test]
fn ospf_auth_takes_its_secrets_by_version() {
    let yaml = format!("{YAML}  ospf_auth:\n    current: 2\n");
    let env = [
        ("APP_SETTINGS__OSPF_AUTH__SECRETS__1", "old"),
        ("APP_SETTINGS__OSPF_AUTH__SECRETS__2", "new"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = settings_from(
        File::from_str(&yaml, FileFormat::Yaml),
        environment().source(Some(env)),
    )
    .unwrap();
    let auth = settings.ospf_auth.expect("ospf_auth");
    assert_eq!((auth.current(), auth.current_secret()), (2, "new"));

    let yaml = format!("{YAML}  ospf_auth:\n    current: 3\n    secrets:\n      \"1\": old\n");
    let refused = settings_from(File::from_str(&yaml, FileFormat::Yaml), environment());
    assert!(refused.is_err());
}

/// Without `topics` the devices send what they sent before the provisioner.
#[test]
fn device_syslog_defaults_to_the_four_levels() {
    let yaml = format!("{YAML}  device_syslog:\n    remote: 2a02:16a:fe00:1a0::10:8\n");
    let settings = settings_from(File::from_str(&yaml, FileFormat::Yaml), environment()).unwrap();
    let syslog = settings.device_syslog.expect("device_syslog");
    assert_eq!(syslog.port, None);
    assert_eq!(
        syslog.topics().collect::<Vec<_>>(),
        ["info", "warning", "error", "critical"]
    );
}

/// The chart hands certificate and key over as variables (each a value or a key of a
/// Secret, cert-manager's `tls.crt` and `tls.key`), none in the file; both keep their
/// lines, which PEM needs.
#[test]
fn www_ca_comes_from_the_environment() {
    let certificate = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
    let key = "-----BEGIN PRIVATE KEY-----\nBBBB\n-----END PRIVATE KEY-----\n";
    let env = [
        ("APP_SETTINGS__WWW_CA__CERTIFICATE", certificate),
        ("APP_SETTINGS__WWW_CA__KEY", key),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect();
    let settings = settings_from(
        File::from_str(YAML, FileFormat::Yaml),
        environment().source(Some(env)),
    )
    .unwrap();
    let ca = settings.www_ca.expect("www_ca");
    assert_eq!(&*ca.certificate, certificate);
    assert_eq!(&*ca.key, key);
}
