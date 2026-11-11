use super::*;
use mikrotik_model::ascii::AsciiString;
use rcgen::{BasicConstraints, IsCa};
use x509_parser::extensions::{GeneralName, ParsedExtension};

/// A CA as `openssl req -x509` makes one: self-signed, with a subject key identifier.
fn test_ca() -> Pem {
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "Test WebFig CA");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = params.self_signed(&key).unwrap();
    Pem {
        certificate: cert.pem().into(),
        key: key.serialize_pem().into(),
    }
}

fn names() -> Names {
    Names {
        ips: [
            "192.168.100.9".parse().unwrap(),
            "2a02:16a:fe00:8000::9".parse().unwrap(),
        ]
        .into(),
        dns: ["lab-hex".into()].into(),
    }
}

#[test]
fn an_issued_certificate_holds_the_names_and_names_its_ca() {
    let ca_pem = test_ca();
    let ca = read_ca(&ca_pem.certificate, &ca_pem.key).unwrap();
    let issued = issue(&ca, &names(), SystemTime::now()).unwrap();
    assert!(issued.key.contains("PRIVATE KEY"));
    let (_, pem) = x509_parser::pem::parse_x509_pem(issued.certificate.as_bytes()).unwrap();
    let cert = pem.parse_x509().unwrap();
    let mut ips = BTreeSet::new();
    let mut dns = BTreeSet::new();
    let mut akid = None;
    for extension in cert.extensions() {
        match extension.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => {
                for name in &san.general_names {
                    match name {
                        GeneralName::IPAddress(bytes) => {
                            ips.insert(match bytes.len() {
                                4 => IpAddr::from(<[u8; 4]>::try_from(*bytes).unwrap()),
                                _ => IpAddr::from(<[u8; 16]>::try_from(*bytes).unwrap()),
                            });
                        }
                        GeneralName::DNSName(name) => {
                            dns.insert(Box::<str>::from(*name));
                        }
                        _ => {}
                    }
                }
            }
            ParsedExtension::AuthorityKeyIdentifier(id) => {
                akid = id.key_identifier.as_ref().map(|id| {
                    id.0.iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<String>()
                });
            }
            _ => {}
        }
    }
    assert_eq!(Names { ips, dns }, names());
    // RouterOS prints it as `akid`, compared with the CA's
    assert_eq!(akid.as_deref(), Some(&*ca.key_id));
    let days =
        (cert.validity().not_after.timestamp() - cert.validity().not_before.timestamp()) / 86400;
    assert_eq!(days, 398, "397 days from now, from a day before");
}

#[test]
fn a_ca_that_cant_be_read_is_refused() {
    let ca = test_ca();
    assert!(read_ca(&ca.certificate, "not a key").is_err());
    assert!(read_ca("not a certificate", &ca.key).is_err());
}

#[test]
fn routeros_names_are_read() {
    let parsed = parse_names("IP:192.168.100.9,IP:2a02:16a:fe00:8000::9,DNS:Lab-Hex");
    assert_eq!(parsed, names());
}

fn on_device(name: &str, akid: &str, names: Names, days_left: u64) -> OnDevice {
    OnDevice {
        name: name.into(),
        akid: Some(akid.into()),
        names,
        expires_after: Some(Duration::from_secs(days_left * 86400)),
        private_key: true,
    }
}

#[test]
fn the_usable_certificate_is_the_ca_s_for_the_names_lasting_longest() {
    let wanted = names();
    let mut fewer = names();
    fewer.ips.pop_first();
    let certificates = [
        on_device("a", "CA", wanted.clone(), 200),
        on_device("b", "CA", wanted.clone(), 300),
        on_device("other-ca", "OTHER", wanted.clone(), 390),
        on_device("fewer-names", "CA", fewer, 390),
        on_device("expiring", "CA", wanted.clone(), 20),
        OnDevice {
            private_key: false,
            ..on_device("no-key", "CA", wanted.clone(), 390)
        },
    ];
    assert_eq!(
        usable(&certificates, &wanted, "CA").map(|cert| &*cert.name),
        Some("b")
    );
    assert_eq!(usable(&certificates[4..], &wanted, "CA"), None);
}

#[test]
fn a_device_name_is_a_dns_name_only_if_it_can_be() {
    assert!(is_dns_name("lab-hex"));
    assert!(is_dns_name("ap.buero-1og"));
    assert!(!is_dns_name("ap buero"));
    assert!(!is_dns_name("-x"));
    assert!(!is_dns_name("router_1"));
}

/// CAPsMAN names a removed certificate by its id, which RouterOS gave WebFig's later
/// (lab-wap, 7.24.6): the provisioner's own are never CAPsMAN's, to be cleaned up.
#[test]
fn webfig_s_certificate_is_never_capsman_s() {
    let generated = crate::device::ros::cfg::capsman_generated([
        Some("netbox-provisioner-www-1794381103".into()),
        Some("capsman-cert".into()),
    ]);
    assert_eq!(&*generated, [AsciiString::from("capsman-cert")]);
}
