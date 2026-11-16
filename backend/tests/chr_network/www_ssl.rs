//! Scope: CHR test of WebFig's certificate (`backend::device::www_certificate`); nothing else.
//!
//! A certificate issued by a test CA is uploaded, imported with its key and attached to
//! `www-ssl` of a real CHR, the issuing intermediate CA's certificate along, not trusted by
//! the device, so a client trusting only the root gets the chain; read back as in place (its IPv4 and IPv6 names included), it answers HTTPS to a
//! client trusting the CA, and gives way to a new one when the device's names change: the old
//! one, no service naming it anymore, is removed. Another CA replaces the old one's
//! certificate too.
use crate::chr_network::apply::enable_socks;
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::node::set_keyed;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use backend::device::access::{Session, field, observe};
use backend::device::www_certificate::{
    Ca, Names, Pem, WwwCertificate, WwwCertificateInstalled, install_for, read_for,
};
use backend::job::{Event, Log};
use mikrotik_model::action::FetchProbe;
use mikrotik_model::model::{Certificate, CertificateByName, File, IpServiceByName};
use mikrotik_model::resource::{KeyedResource, collect_resource};
use mikrotik_model::value::HasNone;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_socks::tcp::Socks5Stream;

/// A CA as `openssl req -x509` makes one, as PEM.
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

/// An intermediate CA signed by `root`, as PEM: the CA issuing the devices' certificates.
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

async fn www_ssl_certificate(device: &Session) -> Option<Box<str>> {
    let service = IpServiceByName::fetch(device, &"www-ssl".into())
        .await
        .expect("services")
        .expect("www-ssl");
    match service.data.certificate {
        Some(HasNone::Value(name)) => Some(name.to_string().into()),
        _ => None,
    }
}

async fn own_certificates(device: &Session) -> Box<[Box<str>]> {
    collect_resource::<Certificate>(device)
        .await
        .expect("certificates")
        .into_iter()
        .map(|certificate| Box::<str>::from(certificate.cfg.name.to_string()))
        .filter(|name| name.starts_with("netbox-provisioner-www"))
        .collect()
}

/// One of the provisioner's CA certificates on the device.
#[derive(Debug)]
struct OwnCa {
    name: Box<str>,
    trusted: Option<bool>,
}

async fn own_cas(device: &Session) -> Box<[OwnCa]> {
    collect_resource::<Certificate>(device)
        .await
        .expect("certificates")
        .into_iter()
        .map(|certificate| OwnCa {
            name: certificate.cfg.name.to_string().into(),
            trusted: certificate.cfg.trusted,
        })
        .filter(|ca| ca.name.starts_with("netbox-provisioner-ca"))
        .collect()
}

/// How many certificates `www-ssl` sends, through the node's SOCKS to its own 127.0.0.1:443,
/// to a client trusting `ca`: 2 with the chain, the device's and the CA's.
async fn served_chain(socks_port: u16, ca: &Pem) -> usize {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(ca.certificate.as_bytes()).expect("CA PEM"))
        .expect("CA");
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let local = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let stream = Socks5Stream::connect(("127.0.0.1", socks_port), SocketAddr::new(local, 443))
        .await
        .expect("through SOCKS");
    let tls = TlsConnector::from(Arc::new(config))
        .connect(ServerName::IpAddress(local.into()), stream)
        .await
        .expect("TLS with the CA");
    tls.get_ref().1.peer_certificates().map_or(0, <[_]>::len)
}

fn installed(log: &Log) -> WwwCertificateInstalled {
    match log.events().as_slice() {
        [Event::WwwCertificateInstalled(installed)] => installed.clone(),
        events => panic!("{events:?}"),
    }
}

#[tokio::test]
#[ignore]
async fn webfig_gets_a_certificate_of_the_ca() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .unwrap();
    let work_dir = std::env::temp_dir().join("chr-network-www-ssl");
    let node = boot_node(&golden, &work_dir, "www", &[], crate::TEST_PASSWORD)
        .await
        .unwrap();
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    let root_pem = test_ca();
    let ca_pem = intermediate_of(&root_pem);
    let ca = Ca::from_pem(
        &ca_pem.certificate,
        Some(&root_pem.certificate),
        &ca_pem.key,
    )
    .unwrap();
    let mut names = Names {
        ips: ["127.0.0.1".parse().unwrap(), "2001:db8::9".parse().unwrap()].into(),
        dns: ["chr-www".into()].into(),
    };

    let found = read_for(&device, &names, Some(&ca)).await.unwrap();
    assert_eq!(found.state(), WwwCertificate::Needed);
    let log = Log::new("www-ssl");
    install_for(&device, "chr-www", &names, &ca, &found, &log).await;
    let first = installed(&log);
    assert_eq!(first.error, None);
    let first = first.name.unwrap();
    assert_eq!(www_ssl_certificate(&device).await.as_ref(), Some(&first));
    assert_eq!(own_certificates(&device).await, [first.clone()].into());
    let cas = own_cas(&device).await;
    let [first_ca] = &*cas else {
        panic!("one CA certificate: {cas:?}")
    };
    assert_eq!(
        first_ca.trusted,
        Some(false),
        "the device doesn't trust the CA"
    );
    let files = collect_resource::<File>(&device)
        .await
        .expect("files")
        .into_iter()
        .map(|file| file.cfg.name.to_string())
        .collect::<Vec<_>>();
    assert!(
        !files
            .iter()
            .any(|name| name.contains("netbox-provisioner-www")),
        "the import takes the files away: {files:?}"
    );
    let found = read_for(&device, &names, Some(&ca)).await.unwrap();
    assert_eq!(
        found.state(),
        WwwCertificate::InPlace,
        "its names read back"
    );

    // a client trusting the CA gets in over HTTPS (the generated config turns www-ssl on)
    set_keyed(
        &device,
        &"www-ssl".into(),
        |service: &mut IpServiceByName| service.data.disabled = false,
    )
    .await
    .expect("www-ssl on");
    // a client trusting the CA gets the chain
    enable_socks(&device).await.expect("SOCKS");
    assert_eq!(
        served_chain(node.socks_port, &root_pem).await,
        2,
        "its own and the intermediate's"
    );

    // the device as client trusts it for this check only
    set_keyed(
        &device,
        &(&*first_ca.name).into(),
        |certificate: &mut CertificateByName| certificate.0.trusted = Some(true),
    )
    .await
    .expect("CA trusted");
    let https = FetchProbe {
        url: "https://127.0.0.1/",
        src_address: None,
        duration: None,
        check_certificate: true,
    };
    let fetched = observe(&device, &https).await.expect("WebFig over HTTPS");
    assert!(
        fetched
            .iter()
            .any(|row| field(row, "status") == Some("finished")),
        "{fetched:?}"
    );

    // another address on the device: a new certificate, the old one goes
    names.ips.insert("192.168.77.1".parse().unwrap());
    let found = read_for(&device, &names, Some(&ca)).await.unwrap();
    assert_eq!(found.state(), WwwCertificate::Needed);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let log = Log::new("www-ssl");
    install_for(&device, "chr-www", &names, &ca, &found, &log).await;
    let second = installed(&log).name.unwrap();
    assert_ne!(second, first);
    assert_eq!(www_ssl_certificate(&device).await.as_ref(), Some(&second));
    assert_eq!(own_certificates(&device).await, [second.clone()].into());
    assert_eq!(
        read_for(&device, &names, Some(&ca)).await.unwrap().state(),
        WwwCertificate::InPlace
    );

    // a certificate of another CA is no use: a new one of this one, its certificate replacing
    // the old CA's
    let other_root = test_ca();
    let other_pem = intermediate_of(&other_root);
    let other = Ca::from_pem(
        &other_pem.certificate,
        Some(&other_root.certificate),
        &other_pem.key,
    )
    .unwrap();
    let found = read_for(&device, &names, Some(&other)).await.unwrap();
    assert_eq!(found.state(), WwwCertificate::Needed);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let log = Log::new("www-ssl");
    install_for(&device, "chr-www", &names, &other, &found, &log).await;
    let third = installed(&log).name.unwrap();
    assert_eq!(own_certificates(&device).await, [third].into());
    let cas = own_cas(&device).await;
    assert!(
        matches!(&*cas, [ca] if ca.name != first_ca.name && ca.trusted == Some(false)),
        "{cas:?}"
    );
    assert_eq!(served_chain(node.socks_port, &other_root).await, 2);
    assert_eq!(
        read_for(&device, &names, Some(&other))
            .await
            .unwrap()
            .state(),
        WwwCertificate::InPlace
    );
    assert_eq!(
        read_for(&device, &names, None).await.unwrap().state(),
        WwwCertificate::Off
    );
}
