//! Scope: WebFig over HTTPS (A7): the certificate of each device, issued by the CA of the
//! config (`www_ca`), installed and attached to `www-ssl`; not the services' settings
//! (`device::ros::services`) nor when an apply installs it (`device::ros::apply`).
//!
//! The provisioner issues a certificate for the device's NetBox addresses and name, uploads
//! it with its key (`/file/add`, then `/certificate/import`, which takes the files away) and
//! attaches it to `www-ssl` over the API, before the apply plans. The CA certificates between
//! it and the root go along (`Ca::chain`), without key and not trusted by the device: only with
//! them in the store `www-ssl` sends them, so a browser trusting the root alone (downloaded from
//! `/www-ca.crt`) gets the whole chain. The root isn't uploaded: lab-np2 (netPower 16P, 7.24.6)
//! never sent a self-signed one, its intermediate CA it does (a CHR sends both). Only once the device has
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
    value::{HasNone, SubjectAltName},
};
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, Issuer, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256, SanType,
};
use std::{
    collections::BTreeSet,
    net::IpAddr,
    sync::LazyLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The start of the names of the provisioner's certificates on a device; each has the second
/// it was issued after it.
pub(crate) const PREFIX: &str = "netbox-provisioner-www";
/// The same for the CA's certificate on a device.
const CA_PREFIX: &str = "netbox-provisioner-ca";
/// How long a certificate is valid: browsers refuse longer ones from public CAs, and some
/// apply the limit to every CA.
const VALIDITY: Duration = Duration::from_secs(397 * 24 * 3600);
/// How long attaching a just imported certificate is tried: with its CA in the store RouterOS
/// refuses it for a few seconds ("input does not match any value of certificate", CHR 7.24.6).
const ATTACH_WITHIN: Duration = Duration::from_secs(20);
/// A certificate expiring within this is issued anew at the next apply.
const RENEW_BEFORE: Duration = Duration::from_secs(30 * 24 * 3600);

/// Where WebFig over HTTPS stands on a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum WwwCertificate {
    /// no CA in the config: WebFig stays on plain HTTP (`www`)
    Off,
    /// a valid certificate of the CA is attached to `www-ssl`: the config uses it
    InPlace,
    /// none, or it doesn't hold the device's addresses, expires soon or isn't attached, or the
    /// CA's certificate is missing for the chain: an apply puts it in place first
    Needed,
}

/// A CA issuing the devices' certificates: the config's (`www_ca`, read once), or a test's.
pub struct Ca {
    issuer: Issuer<'static, KeyPair>,
    /// its subject key identifier, upper-case hex as RouterOS prints the `akid` of what it
    /// signed
    key_id: Box<str>,
    /// the certificates a device sends along with its own: the issuing CA's and the ones above
    /// it up to the root, without the root, which the browsers have
    chain: Box<[ChainCertificate]>,
    /// the root CA's certificate (PEM), for the admins' browsers
    root: Box<str>,
}

/// One certificate of [`Ca::chain`].
pub(crate) struct ChainCertificate {
    pem: Box<str>,
    /// its subject key identifier as RouterOS prints it (`skid`)
    key_id: Box<str>,
}

static CA: LazyLock<Option<Result<Ca, Box<str>>>> = LazyLock::new(|| {
    CONFIG
        .www_ca
        .as_ref()
        .map(|ca| read_ca(&ca.certificate, ca.root.as_deref(), &ca.key))
});

impl Ca {
    /// The CA of `certificate` (PEM, the issuing CA's, then the ones above it), its `key` and
    /// `root` (PEM; without, the self-signed one of `certificate`); every one but the root
    /// needs a subject key identifier, which RouterOS shows as the `akid` of what it signed.
    pub fn from_pem(certificate: &str, root: Option<&str>, key: &str) -> Result<Ca, Error> {
        read_ca(certificate, root, key).map_err(Error::WwwCaInvalid)
    }

    /// The root CA's certificate (PEM).
    pub fn root(&self) -> &str {
        &self.root
    }
}

fn text(error: impl std::fmt::Display) -> Box<str> {
    error.to_string().into()
}

/// The certificates of a PEM text, each as its own PEM.
fn pem_blocks(text: &str) -> impl Iterator<Item = &str> {
    const END: &str = "-----END CERTIFICATE-----";
    text.split_inclusive(END)
        .filter(|block| block.contains(END))
        .map(str::trim)
}

/// A certificate read from PEM: whether it signed itself and its key identifier, upper-case
/// hex, if it has one.
struct Read {
    self_signed: bool,
    key_id: Option<Box<str>>,
}

fn read_certificate(pem: &str) -> Result<Read, Box<str>> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem.as_bytes()).map_err(text)?;
    let cert = pem.parse_x509().map_err(text)?;
    let key_id = cert
        .extensions()
        .iter()
        .find_map(|extension| match extension.parsed_extension() {
            x509_parser::extensions::ParsedExtension::SubjectKeyIdentifier(id) => Some(id.0),
            _ => None,
        })
        .map(|id| id.iter().map(|byte| format!("{byte:02X}")).collect());
    Ok(Read {
        self_signed: cert.subject().as_raw() == cert.issuer().as_raw(),
        key_id,
    })
}

fn read_ca(certificate: &str, root: Option<&str>, key: &str) -> Result<Ca, Box<str>> {
    let blocks = pem_blocks(certificate).collect::<Box<[_]>>();
    let issuing = *blocks.first().ok_or("no certificate")?;
    let key = KeyPair::from_pem(key).map_err(text)?;
    let issuer = Issuer::from_ca_cert_pem(issuing, key).map_err(text)?;
    let no_key_id = || "the CA certificate has no subject key identifier";
    let key_id = read_certificate(issuing)?.key_id.ok_or_else(no_key_id)?;
    let mut chain = Vec::new();
    let mut self_signed = None;
    for block in &blocks {
        let read = read_certificate(block)?;
        if read.self_signed {
            self_signed = Some(*block);
        } else {
            chain.push(ChainCertificate {
                pem: (*block).into(),
                key_id: read.key_id.ok_or_else(no_key_id)?,
            });
        }
    }
    let root = match root {
        Some(root) => {
            let root = pem_blocks(root).next().ok_or("root: no certificate")?;
            read_certificate(root)?;
            root
        }
        None => self_signed.ok_or(
            "no root CA: give `root` (cert-manager's `ca.crt`) or put it last in `certificate`",
        )?,
    };
    Ok(Ca {
        issuer,
        key_id,
        chain: chain.into(),
        root: root.into(),
    })
}

/// The root CA's certificate of the config, `None` without `www_ca` or with one that can't be
/// read (the server doesn't start then).
pub fn root() -> Option<&'static str> {
    ca()?.ok().map(Ca::root)
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

/// The names of a `subject-alt-name`.
fn names_of(subject_alt_name: &BTreeSet<SubjectAltName>) -> Names {
    let mut names = Names::default();
    for entry in subject_alt_name {
        match entry {
            SubjectAltName::Ip(ip) => {
                names.ips.insert(*ip);
            }
            SubjectAltName::Dns(dns) => {
                names
                    .dns
                    .insert(dns.to_string().to_ascii_lowercase().into());
            }
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
        names: names_of(&status.subject_alt_name),
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
    /// the key identifiers of [`Ca::chain`] the store lacks, under any name
    missing_chain: Box<[Box<str>]>,
    /// the provisioner's CA certificates (`CA_PREFIX`) not of the chain: a former CA's
    other_cas: Box<[Box<str>]>,
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
            missing_chain: Box::default(),
            other_cas: Box::default(),
        });
    };
    let all = collect_resource::<Certificate>(raw).await?;
    let certificates = all.iter().filter_map(on_device).collect::<Box<[_]>>();
    let cas = CaCertificates::of(&all, &ca.chain);
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
        (Some(usable), Some(attached))
            if *usable == attached && cas.missing.is_empty() && cas.others.is_empty() =>
        {
            WwwCertificate::InPlace
        }
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
        missing_chain: cas.missing,
        other_cas: cas.others,
    })
}

/// The CA certificates on a device against the chain it is to send.
struct CaCertificates {
    /// the key identifiers of the chain no certificate has
    missing: Box<[Box<str>]>,
    /// the provisioner's not of the chain
    others: Box<[Box<str>]>,
}

impl CaCertificates {
    fn of(certificates: &[Certificate], chain: &[ChainCertificate]) -> CaCertificates {
        let skid = |certificate: &Certificate| {
            certificate
                .status
                .skid
                .as_ref()
                .map(|skid| skid.to_string())
        };
        let in_store = certificates
            .iter()
            .filter_map(skid)
            .collect::<BTreeSet<_>>();
        let of_chain = |certificate: &Certificate| {
            skid(certificate).is_some_and(|skid| chain.iter().any(|ca| *ca.key_id == *skid))
        };
        CaCertificates {
            missing: chain
                .iter()
                .filter(|ca| !in_store.contains(&*ca.key_id))
                .map(|ca| ca.key_id.clone())
                .collect(),
            others: certificates
                .iter()
                .filter(|certificate| !of_chain(certificate))
                .map(|certificate| certificate.cfg.name.to_string())
                .filter(|name| name.starts_with(CA_PREFIX))
                .map(Box::from)
                .collect(),
        }
    }
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
        for (index, missing) in ca
            .chain
            .iter()
            .enumerate()
            .filter(|(_, chained)| found.missing_chain.contains(&chained.key_id))
        {
            upload_ca(raw, &missing.pem, index).await?;
        }
        let name = match &found.usable {
            Some(name) => name.clone(),
            None => upload(raw, names, ca).await?,
        };
        attach(raw, &name).await?;
        Ok::<_, Error>(name)
    };
    match installed.await {
        Ok(name) => {
            let old = found.unused.iter().filter(|old| **old != name);
            for old in old.chain(&found.other_cas) {
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

/// Attaches the certificate `name` to `www-ssl`, waiting up to [`ATTACH_WITHIN`] for RouterOS
/// to take it.
async fn attach(raw: &impl Writes, name: &str) -> Result<(), Error> {
    let start = Instant::now();
    loop {
        match attach_once(raw, name).await {
            Err(Error::MikrotikModel(mikrotik_model::resource::Error::Trap(trap)))
                if trap
                    .message
                    .starts_with(b"input does not match any value of certificate")
                    && start.elapsed() < ATTACH_WITHIN =>
            {
                log::debug!("certificate {name} not taken yet");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            result => return result,
        }
    }
}

async fn attach_once(raw: &impl Writes, name: &str) -> Result<(), Error> {
    let current = IpServiceByName::fetch(raw, &AsciiString::from("www-ssl"))
        .await?
        .ok_or_else(|| Error::Refused("/ip/service: no www-ssl".into()))?;
    let mut target = current.clone();
    target.data.certificate = Some(HasNone::Value(name.into()));
    apply_mutation(raw, &target.calculate_update(&current)).await?;
    Ok(())
}

/// A new name of a certificate: `prefix` and the second `now`.
fn new_name(prefix: &str, now: SystemTime) -> String {
    format!(
        "{prefix}-{}",
        now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
    )
}

/// Uploads `contents` as `file` and imports it as the certificate `name` (its key, if a key).
async fn import(raw: &impl Writes, name: &str, file: &str, contents: &str) -> Result<(), Error> {
    let mut entry = FileCfg::new(file.into());
    entry.contents = Some(contents.into());
    apply_mutation(raw, &entry.calculate_create()).await?;
    let import = CertificateImport {
        file_name: file,
        passphrase: "",
        name,
    };
    act(raw, &import).await?;
    Ok(())
}

/// Imports the `index`th certificate of the chain, `pem`, under a new name, not trusted by
/// the device.
async fn upload_ca(raw: &impl Writes, pem: &str, index: usize) -> Result<(), Error> {
    let name = format!("{}-{index}", new_name(CA_PREFIX, SystemTime::now()));
    let file = format!("{name}.crt");
    let imported = async {
        import(raw, &name, &file, pem).await?;
        let current = CertificateByName::fetch(raw, &AsciiString::from(name.as_str()))
            .await?
            .ok_or_else(|| Error::WwwCertificateNotImported(name.as_str().into()))?;
        let mut target = current.clone();
        target.0.trusted = Some(false);
        apply_mutation(raw, &target.calculate_update(&current)).await?;
        Ok(())
    };
    let result = imported.await;
    let _ = remove_if_present::<FileByName>(raw, &AsciiString::from(file.as_str())).await;
    if result.is_err() {
        let _ =
            remove_if_present::<CertificateByName>(raw, &AsciiString::from(name.as_str())).await;
    }
    result
}

/// Issues a certificate for `names` and imports it with its key under a new name.
async fn upload(raw: &impl Writes, names: &Names, ca: &Ca) -> Result<Box<str>, Error> {
    let now = SystemTime::now();
    let pem = issue(ca, names, now)?;
    let name = new_name(PREFIX, now);
    let files = [format!("{name}.crt"), format!("{name}.key")];
    let imported = async {
        for (file, contents) in files.iter().zip([&pem.certificate, &pem.key]) {
            import(raw, &name, file, contents).await?;
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
