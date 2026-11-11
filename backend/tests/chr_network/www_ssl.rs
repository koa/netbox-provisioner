//! Scope: CHR test of WebFig's certificate (`backend::device::www_certificate`); nothing else.
//!
//! A certificate issued by a test CA is uploaded, imported with its key and attached to
//! `www-ssl` of a real CHR, read back as in place (its IPv4 and IPv6 names included), answers
//! HTTPS to a client trusting the CA, and gives way to a new one when the device's names
//! change: the old one, no service naming it anymore, is removed.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::boot_node;
use backend::device::www_certificate::{
    Ca, Names, Pem, WwwCertificate, WwwCertificateInstalled, install_for, read_for,
};
use backend::job::{Event, Log};
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio_stream::StreamExt;

/// The rows `command` with `attributes` answers, failing on a trap.
async fn rows(
    device: &MikrotikDevice<SimpleResult>,
    command: &str,
    attributes: &[(&str, &str)],
) -> Box<[HashMap<Box<str>, Option<Box<str>>>]> {
    let mut stream = device
        .send_command(
            command.as_bytes(),
            |cmd| {
                attributes.iter().fold(cmd, |cmd, (key, value)| {
                    cmd.attribute(key.as_bytes(), value.as_bytes())
                })
            },
            (),
        )
        .await;
    let mut rows = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(row) => rows.push(row),
            SimpleResult::Trap { message, .. } => panic!("{command}: {message}"),
            SimpleResult::Error(error) => panic!("{command}: {error:?}"),
        }
    }
    rows.into()
}

fn field<'r>(row: &'r HashMap<Box<str>, Option<Box<str>>>, key: &str) -> Option<&'r str> {
    row.get(key).and_then(Option::as_deref)
}

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

async fn www_ssl_certificate(device: &MikrotikDevice<SimpleResult>) -> Option<Box<str>> {
    rows(device, "/ip/service/print", &[])
        .await
        .into_iter()
        .find(|row| field(row, "name") == Some("www-ssl"))
        .and_then(|row| field(&row, "certificate").map(Box::from))
}

async fn own_certificates(device: &MikrotikDevice<SimpleResult>) -> Box<[Box<str>]> {
    rows(device, "/certificate/print", &[])
        .await
        .into_iter()
        .filter_map(|row| field(&row, "name").map(Box::<str>::from))
        .filter(|name| name.starts_with("netbox-provisioner-www"))
        .collect()
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
    let ca_pem = test_ca();
    let ca = Ca::from_pem(&ca_pem.certificate, &ca_pem.key).unwrap();
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
    let files = rows(&device, "/file/print", &[]).await;
    assert!(
        !files
            .iter()
            .any(|row| field(row, "name")
                .is_some_and(|name| name.contains("netbox-provisioner-www"))),
        "the import takes the files away: {files:?}"
    );
    let found = read_for(&device, &names, Some(&ca)).await.unwrap();
    assert_eq!(
        found.state(),
        WwwCertificate::InPlace,
        "its names read back"
    );

    // a client trusting the CA gets in over HTTPS (the generated config turns www-ssl on)
    rows(
        &device,
        "/ip/service/set",
        &[("numbers", "www-ssl"), ("disabled", "no")],
    )
    .await;
    rows(
        &device,
        "/file/add",
        &[("name", "test-ca.crt"), ("contents", &ca_pem.certificate)],
    )
    .await;
    rows(
        &device,
        "/certificate/import",
        &[
            ("file-name", "test-ca.crt"),
            ("passphrase", ""),
            ("name", "test-ca"),
        ],
    )
    .await;
    let fetched = rows(
        &device,
        "/tool/fetch",
        &[
            ("url", "https://127.0.0.1/"),
            ("check-certificate", "yes"),
            ("output", "none"),
        ],
    )
    .await;
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

    // a certificate of another CA is no use: a new one of this one
    let other_pem = test_ca();
    let other = Ca::from_pem(&other_pem.certificate, &other_pem.key).unwrap();
    let found = read_for(&device, &names, Some(&other)).await.unwrap();
    assert_eq!(found.state(), WwwCertificate::Needed);
    assert_eq!(
        read_for(&device, &names, None).await.unwrap().state(),
        WwwCertificate::Off
    );
}
