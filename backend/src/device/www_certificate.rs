//! Scope: WebFig over HTTPS (A7): the certificate of each device, issued by the CA of the
//! config (`www_ca`), installed and attached to `www-ssl`; not the services' settings
//! (`device::ros::services`) nor when an apply installs it (`device::ros::apply`).
//!
//! The provisioner issues a certificate for the device's NetBox addresses and name, uploads
//! it with its key (`/file/add`, then `/certificate/import`, which takes the files away) and
//! attaches it to `www-ssl` over the API, before the apply plans. Only once the device has
//! such a certificate attached the generated config turns `www-ssl` on and `www` off
//! (`services::use_www_ssl`): the services' target keeps the `certificate` the device has, so
//! the diff leaves it as attached here. RouterOS lets a certificate in use be removed (the service then
//! reports a dangling id, CHR 7.24.6), so older ones go only when no other service names them, once
//! `www-ssl` has the new one.
use crate::{
    Error,
    config::CONFIG,
    device::access::{act, remove_if_present},
    error_info::ErrorInfo,
    job::Log,
    topology::access::device::DeviceAccess,
};
use async_graphql::{Enum, SimpleObject};
use mikrotik_model::{
    action::CertificateImport,
    ascii::AsciiString,
    command::{Reads, Writes},
    model::{Certificate, CertificateByName, FileByName, FileCfg, IpService, IpServiceByName},
    resource::{Creatable, KeyedResource, Updatable, apply_mutation, collect_resource},
    value::HasNone,
};
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, Issuer, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256, SanType,
};
use std::{
    collections::BTreeSet,
    net::IpAddr,
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The start of the names of the provisioner's certificates on a device; each has the second
/// it was issued after it.
pub(crate) const PREFIX: &str = "netbox-provisioner-www";
/// How long a certificate is valid: browsers refuse longer ones from public CAs, and some
/// apply the limit to every CA.
const VALIDITY: Duration = Duration::from_secs(397 * 24 * 3600);
/// A certificate expiring within this is issued anew at the next apply.
const RENEW_BEFORE: Duration = Duration::from_secs(30 * 24 * 3600);

/// Where WebFig over HTTPS stands on a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum WwwCertificate {
    /// no CA in the config: WebFig stays on plain HTTP (`www`)
    Off,
    /// a valid certificate of the CA is attached to `www-ssl`: the config uses it
    InPlace,
    /// none, or it doesn't hold the device's addresses, expires soon or isn't attached: an apply
    /// issues and attaches one first
    Needed,
}

/// A CA issuing the devices' certificates: the config's (`www_ca`, read once), or a test's.
pub struct Ca {
    issuer: Issuer<'static, KeyPair>,
    /// its subject key identifier, upper-case hex as RouterOS prints the `akid` of what it
    /// signed
    key_id: Box<str>,
}

static CA: LazyLock<Option<Result<Ca, Box<str>>>> = LazyLock::new(|| {
    CONFIG
        .www_ca
        .as_ref()
        .map(|ca| read_ca(&ca.certificate, &ca.key))
});

impl Ca {
    /// The CA of `certificate` and `key` (PEM); it needs a subject key identifier, which
    /// RouterOS shows as the `akid` of what it signed.
    pub fn from_pem(certificate: &str, key: &str) -> Result<Ca, Error> {
        read_ca(certificate, key).map_err(Error::WwwCaInvalid)
    }
}

fn text(error: impl std::fmt::Display) -> Box<str> {
    error.to_string().into()
}

fn read_ca(certificate: &str, key: &str) -> Result<Ca, Box<str>> {
    let key = KeyPair::from_pem(key).map_err(text)?;
    let issuer = Issuer::from_ca_cert_pem(certificate, key).map_err(text)?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(certificate.as_bytes()).map_err(text)?;
    let cert = pem.parse_x509().map_err(text)?;
    let key_id = cert
        .extensions()
        .iter()
        .find_map(|extension| match extension.parsed_extension() {
            x509_parser::extensions::ParsedExtension::SubjectKeyIdentifier(id) => Some(id.0),
            _ => None,
        })
        .ok_or("the CA certificate has no subject key identifier")?;
    Ok(Ca {
        issuer,
        key_id: key_id.iter().map(|byte| format!("{byte:02X}")).collect(),
    })
}

/// Reads the CA of the config, if any, so the server doesn't start with one it can't use.
pub fn check_ca() -> Result<(), Error> {
    ca().transpose().map(|_| ())
}

/// The CA, `None` without one in the config.
fn ca() -> Option<Result<&'static Ca, Error>> {
    CA.as_ref().map(|ca| {
        ca.as_ref()
            .map_err(|error| Error::WwwCaInvalid(error.clone()))
    })
}

/// What a certificate of the device must hold: every address NetBox has on it (but
/// link-local IPv6) and its name, where that is a DNS name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub ips: BTreeSet<IpAddr>,
    pub dns: BTreeSet<Box<str>>,
}

pub fn wanted(device: &DeviceAccess) -> Names {
    let ips = device
        .interfaces()
        .iter()
        .flat_map(|interface| interface.ips())
        .filter_map(|ip| ip.addr())
        .filter(|ip| !matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local()))
        .collect();
    let name = device.name().to_ascii_lowercase();
    let dns = is_dns_name(&name)
        .then(|| name.into_boxed_str())
        .into_iter()
        .collect();
    Names { ips, dns }
}

fn is_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// One of the provisioner's certificates as the device reports it.
#[derive(Debug, Clone, PartialEq)]
struct OnDevice {
    name: Box<str>,
    /// who signed it
    akid: Option<Box<str>>,
    names: Names,
    expires_after: Option<Duration>,
    private_key: bool,
}

/// The `subject-alt-name` as RouterOS prints it: `IP:127.0.0.1,DNS:chr`.
fn parse_names(text: &str) -> Names {
    let mut names = Names::default();
    for entry in text.split(',').map(str::trim) {
        if let Some(ip) = entry.strip_prefix("IP:") {
            if let Ok(ip) = ip.parse() {
                names.ips.insert(ip);
            }
        } else if let Some(dns) = entry.strip_prefix("DNS:") {
            names.dns.insert(dns.to_ascii_lowercase().into());
        }
    }
    names
}

fn on_device(certificate: &Certificate) -> Option<OnDevice> {
    let name = certificate.cfg.name.to_string();
    if !name.starts_with(PREFIX) {
        return None;
    }
    let status = &certificate.status;
    Some(OnDevice {
        name: name.into(),
        akid: status.akid.as_ref().map(|akid| akid.to_string().into()),
        names: status
            .subject_alt_name
            .as_ref()
            .map(|names| parse_names(&names.to_string()))
            .unwrap_or_default(),
        expires_after: status.expires_after,
        private_key: status.private_key == Some(true),
    })
}

/// The certificate `service` uses, if any.
fn certificate_of(service: &IpService) -> Option<Box<str>> {
    match &service.cfg.certificate {
        Some(HasNone::Value(name)) => Some(name.to_string().into()),
        _ => None,
    }
}

/// The certificate of `certificates` to use: signed by the CA (`key_id`), with its key, for
/// exactly `wanted`, expiring after [`RENEW_BEFORE`]; of several the one lasting longest.
fn usable<'c>(certificates: &'c [OnDevice], wanted: &Names, key_id: &str) -> Option<&'c OnDevice> {
    certificates
        .iter()
        .filter(|cert| cert.private_key)
        .filter(|cert| cert.akid.as_deref() == Some(key_id))
        .filter(|cert| cert.names == *wanted)
        .filter(|cert| cert.expires_after.is_some_and(|left| left > RENEW_BEFORE))
        .max_by_key(|cert| cert.expires_after)
}

/// What the device has: the provisioner's certificates and what `www-ssl` uses.
pub struct Found {
    state: WwwCertificate,
    /// the certificate to use, attached or not
    usable: Option<Box<str>>,
    /// the provisioner's certificates no service but `www-ssl` names
    unused: Box<[Box<str>]>,
}

impl Found {
    pub fn state(&self) -> WwwCertificate {
        self.state
    }
}

/// Reads the provisioner's certificates of `device` over `raw`, against the config's CA;
/// `Off` without one.
pub(crate) async fn read(raw: &impl Reads, device: &DeviceAccess) -> Result<Found, Error> {
    read_for(raw, &wanted(device), ca().transpose()?).await
}

/// [`read`] for a certificate holding `names`, of `ca`.
pub async fn read_for(raw: &impl Reads, names: &Names, ca: Option<&Ca>) -> Result<Found, Error> {
    let Some(ca) = ca else {
        return Ok(Found {
            state: WwwCertificate::Off,
            usable: None,
            unused: Box::default(),
        });
    };
    let certificates = collect_resource::<Certificate>(raw)
        .await?
        .iter()
        .filter_map(on_device)
        .collect::<Box<[_]>>();
    let services = collect_resource::<IpService>(raw).await?;
    let www_ssl = |service: &IpService| *service.status.name.0 == *b"www-ssl";
    // `www-ssl`'s own is free once the new one is attached
    let named: BTreeSet<Box<str>> = services
        .iter()
        .filter(|service| !www_ssl(service))
        .filter_map(certificate_of)
        .collect();
    let attached = services
        .iter()
        .find(|service| www_ssl(service))
        .and_then(certificate_of);
    let usable = usable(&certificates, names, &ca.key_id).map(|cert| cert.name.clone());
    let state = match (&usable, attached) {
        (Some(usable), Some(attached)) if *usable == attached => WwwCertificate::InPlace,
        _ => WwwCertificate::Needed,
    };
    Ok(Found {
        state,
        usable,
        unused: certificates
            .iter()
            .filter(|cert| !named.contains(&cert.name))
            .map(|cert| cert.name.clone())
            .collect(),
    })
}

/// A certificate and its key, as PEM.
#[derive(Debug, Clone)]
pub struct Pem {
    pub certificate: Box<str>,
    pub key: Box<str>,
}

/// A certificate for `names` signed by `ca`, valid from a day before `now` for [`VALIDITY`].
fn issue(ca: &Ca, names: &Names, now: SystemTime) -> Result<Pem, Error> {
    let failed = |error: rcgen::Error| Error::WwwCertificateNotIssued(error.to_string().into());
    let mut params = CertificateParams::default();
    let common_name = names
        .dns
        .iter()
        .next()
        .map(|dns| dns.to_string())
        .or_else(|| names.ips.iter().next().map(IpAddr::to_string))
        .unwrap_or_else(|| PREFIX.into());
    params
        .distinguished_name
        .push(DnType::CommonName, common_name);
    for dns in &names.dns {
        params
            .subject_alt_names
            .push(SanType::DnsName((&**dns).try_into().map_err(failed)?));
    }
    params
        .subject_alt_names
        .extend(names.ips.iter().copied().map(SanType::IpAddress));
    let now = time::OffsetDateTime::from(now);
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + VALIDITY;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(failed)?;
    let certificate = params.signed_by(&key, &ca.issuer).map_err(failed)?;
    Ok(Pem {
        certificate: certificate.pem().into(),
        key: key.serialize_pem().into(),
    })
}

/// A certificate issued, uploaded and attached to `www-ssl` (`name`, valid `days`); or why
/// not (`error`), WebFig then stays as it is.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WwwCertificateInstalled {
    pub name: Option<Box<str>>,
    pub days: u32,
    pub error: Option<ErrorInfo>,
}

/// Brings the device's certificate in line where `found` needs it (`WwwCertificateInstalled`):
/// attaches the usable one, else issues one and attaches that, then removes the provisioner's
/// certificates no service names anymore. A failure is an event only, the apply goes on with
/// WebFig as it is.
pub(crate) async fn install(raw: &impl Writes, device: &DeviceAccess, found: &Found, log: &Log) {
    match ca().transpose() {
        Ok(Some(ca)) => install_for(raw, device.name(), &wanted(device), ca, found, log).await,
        Ok(None) => {}
        Err(error) => log.emit(WwwCertificateInstalled {
            name: None,
            days: 0,
            error: Some(ErrorInfo::from(&error)),
        }),
    }
}

/// [`install`] of a certificate for `names` of the device `device`, issued by `ca`.
pub async fn install_for(
    raw: &impl Writes,
    device: &str,
    names: &Names,
    ca: &Ca,
    found: &Found,
    log: &Log,
) {
    if found.state != WwwCertificate::Needed {
        return;
    }
    let installed = async {
        let name = match &found.usable {
            Some(name) => name.clone(),
            None => upload(raw, names, ca).await?,
        };
        attach(raw, &name).await?;
        Ok::<_, Error>(name)
    };
    match installed.await {
        Ok(name) => {
            for old in found.unused.iter().filter(|old| **old != name) {
                if let Err(error) =
                    remove_if_present::<CertificateByName>(raw, &AsciiString::from(&**old)).await
                {
                    log::warn!("{device}: certificate {old} not removed: {error}");
                }
            }
            log.emit(WwwCertificateInstalled {
                name: Some(name),
                days: (VALIDITY.as_secs() / 86400) as u32,
                error: None,
            });
        }
        Err(error) => {
            log::warn!("{device}: no certificate for www-ssl: {error}");
            log.emit(WwwCertificateInstalled {
                name: None,
                days: 0,
                error: Some(ErrorInfo::from(&error)),
            });
        }
    }
}

/// Attaches the certificate `name` to `www-ssl`.
async fn attach(raw: &impl Writes, name: &str) -> Result<(), Error> {
    let current = IpServiceByName::fetch(raw, &AsciiString::from("www-ssl"))
        .await?
        .ok_or_else(|| Error::Refused("/ip/service: no www-ssl".into()))?;
    let mut target = current.clone();
    target.data.certificate = Some(HasNone::Value(name.into()));
    apply_mutation(raw, &target.calculate_update(&current)).await?;
    Ok(())
}

/// Issues a certificate for `names` and imports it with its key under a new name.
async fn upload(raw: &impl Writes, names: &Names, ca: &Ca) -> Result<Box<str>, Error> {
    let now = SystemTime::now();
    let pem = issue(ca, names, now)?;
    let name = format!(
        "{PREFIX}-{}",
        now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
    );
    let files = [format!("{name}.crt"), format!("{name}.key")];
    let imported = async {
        for (file, contents) in files.iter().zip([&pem.certificate, &pem.key]) {
            let mut entry = FileCfg::new(file.as_str().into());
            entry.contents = Some((&**contents).into());
            apply_mutation(raw, &entry.calculate_create()).await?;
        }
        for file in &files {
            let import = CertificateImport {
                file_name: file,
                passphrase: "",
                name: &name,
            };
            act(raw, &import).await?;
        }
        let with_key = collect_resource::<Certificate>(raw)
            .await?
            .iter()
            .any(|certificate| {
                *certificate.cfg.name.0 == *name.as_bytes()
                    && certificate.status.private_key == Some(true)
            });
        if !with_key {
            return Err(Error::WwwCertificateNotImported(name.as_str().into()));
        }
        Ok(())
    };
    let result = imported.await;
    // the import takes the files away; after a failure they may be left, and the key with them
    for file in &files {
        let _ = remove_if_present::<FileByName>(raw, &AsciiString::from(file.as_str())).await;
    }
    if let Err(error) = result {
        let _ =
            remove_if_present::<CertificateByName>(raw, &AsciiString::from(name.as_str())).await;
        return Err(error);
    }
    Ok(name.into())
}

#[cfg(test)]
mod test;
