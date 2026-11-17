use super::*;
use rcgen::{BasicConstraints, IsCa, Issuer};
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
    let ca = read_ca(&ca_pem.certificate, None, &ca_pem.key).unwrap();
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
    assert!(read_ca(&ca.certificate, None, "not a key").is_err());
    assert!(read_ca("not a certificate", None, &ca.key).is_err());
}

#[test]
fn routeros_names_are_read() {
    let certificate: Certificate = crate::device::answer::parsed(&[
        (".id", "*1"),
        ("name", "netbox-provisioner-www-1"),
        (
            "subject-alt-name",
            "IP:192.168.100.9,IP:2a02:16a:fe00:8000::9,DNS:Lab-Hex",
        ),
    ]);
    assert_eq!(names_of(&certificate.status.subject_alt_name), names());
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

/// CAPsMAN names a removed certificate by its id, which RouterOS gives another one later:
/// WebFig's (lab-wap, 7.24.6), a hand-made CA (router-schmidrueti). Only those still carrying
/// RouterOS' names are CAPsMAN's, to be cleaned up.
#[test]
fn only_certificates_named_by_routeros_are_capsman_s() {
    let generated = crate::device::ros::cfg::capsman_generated([
        Some("netbox-provisioner-www-1794381103".into()),
        Some("CA_Neu".into()),
    ]);
    assert!(generated.is_empty());
    let generated = crate::device::ros::cfg::capsman_generated([
        Some("WiFi-CAPsMAN-CA-F07AAC44DF79".into()),
        Some("WiFi-CAPsMAN-5B8D49CBC3E9".into()),
    ]);
    assert_eq!(generated.len(), 2);
    assert!(crate::device::ros::cfg::capsman_generated([Some("*1".into()), None]).is_empty());
}

/// A certificate as RouterOS prints it (CHR 7.24.5), named `name` with `skid`.
fn printed(name: &str, skid: &str) -> Certificate {
    crate::device::answer::parsed(&[
        (".id", "*1"),
        ("name", name),
        ("skid", skid),
        ("trusted", "false"),
    ])
}

fn chained(key_id: &str) -> ChainCertificate {
    ChainCertificate {
        pem: "".into(),
        key_id: key_id.into(),
    }
}

#[test]
fn the_chain_counts_under_any_name_and_only_the_provisioner_s_others_go() {
    let certificates = [
        printed("webfig-ca", "F9B9"),
        printed("netbox-provisioner-ca-1-0", "0A0B"),
        printed("someone-elses-ca", "0C0D"),
        printed("netbox-provisioner-www-2", "1111"),
    ];
    let found = CaCertificates::of(&certificates, &[chained("F9B9")]);
    assert!(found.missing.is_empty());
    assert_eq!(&*found.others, [Box::from("netbox-provisioner-ca-1-0")]);
    let found = CaCertificates::of(&certificates, &[chained("AAAA"), chained("0A0B")]);
    assert_eq!(&*found.missing, [Box::from("AAAA")]);
    assert!(found.others.is_empty(), "of the chain");
    assert!(CaCertificates::of(&certificates, &[]).missing.is_empty());
}

/// An intermediate CA signed by `root`, as PEM.
fn intermediate_of(root: &Pem) -> Pem {
    let issuer =
        Issuer::from_ca_cert_pem(&root.certificate, KeyPair::from_pem(&root.key).unwrap()).unwrap();
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "Test WebFig intermediate CA");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = params.signed_by(&key, &issuer).unwrap();
    Pem {
        certificate: cert.pem().into(),
        key: key.serialize_pem().into(),
    }
}

#[test]
fn the_chain_is_the_certificates_below_the_root() {
    let root = test_ca();
    let intermediate = intermediate_of(&root);
    let key_id = |pem: &str| read_certificate(pem).unwrap().key_id.unwrap();
    // cert-manager's `tls.crt` of a CA issued by another one, with the root last
    let bundle = format!("{}{}", intermediate.certificate, root.certificate);
    let ca = read_ca(&bundle, None, &intermediate.key).unwrap();
    assert_eq!(ca.key_id, key_id(&intermediate.certificate));
    let chain = ca.chain.iter().map(|ca| &*ca.key_id).collect::<Box<[_]>>();
    assert_eq!(&*chain, [&*key_id(&intermediate.certificate)]);
    assert_eq!(ca.root(), root.certificate.trim());
    // the root from `ca.crt`
    let ca = read_ca(
        &intermediate.certificate,
        Some(&root.certificate),
        &intermediate.key,
    )
    .unwrap();
    assert_eq!(ca.chain.len(), 1);
    assert_eq!(ca.root(), root.certificate.trim());
    assert!(
        read_ca(&intermediate.certificate, None, &intermediate.key).is_err(),
        "no root"
    );
    // a self-signed CA issuing directly: nothing to send along
    let ca = read_ca(&root.certificate, None, &root.key).unwrap();
    assert!(ca.chain.is_empty());
    assert_eq!(ca.root(), root.certificate.trim());
}
