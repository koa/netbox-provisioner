//! Scope: The typed commands the tests send a node for their setup and checks; not booting it
//! (`vm`), not what a test checks.
//!
//! Like the provisioner, the tests reach a node only through mikrotik-model: a menu's model
//! for a `print`, `add` or `set`, an action of its catalog for the rest. So they exercise the
//! same typed commands, and a model missing something they need gets it there.

use backend::device::access::{Session, act, observe};
use backend::device::update::DeviceLink;
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::{Connection, Generation};
use mikrotik_model::{
    action::{Execute, Ping},
    command::Writes,
    model::{IpAddressCfg, IpSshCfg, IpSshForwardingEnabled, SystemIdentityCfg, SystemScriptCfg},
    resource::{
        CfgResource, Creatable, KeyedResource, SingleResource, Updatable, apply_mutation,
        generate_single_update,
    },
};
use std::{net::IpAddr, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};

/// Logs in as `admin` at the node's forwarded API `port`.
pub async fn login(port: u16, password: &str) -> Result<Session, mikrotik_api::prelude::Error> {
    let connection = Connection::connect(
        ("127.0.0.1", port),
        b"admin",
        Some(encode_latin1_lossy(password)),
        Generation::default(),
    )
    .await?;
    Ok(Session::new(connection))
}

/// Logs in as `admin` over `stream` (a connection through a SOCKS proxy or a tunnel).
pub async fn login_over<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    password: &str,
) -> Result<Session, mikrotik_api::prelude::Error> {
    let connection = Connection::connect_with_stream(
        stream,
        b"admin",
        Some(encode_latin1_lossy(password)),
        Generation::default(),
    )
    .await?;
    Ok(Session::new(connection))
}

/// A job's way to the node: straight to its forwarded API port, nothing to upload.
pub struct Direct(pub u16);

impl DeviceLink for Direct {
    async fn api(&self) -> Result<Session, backend::Error> {
        Ok(login(self.0, crate::TEST_PASSWORD).await?)
    }
    async fn upload(
        &self,
        _name: &str,
        _data: &[u8],
        _progress: &(impl Fn(usize) + Sync),
    ) -> Result<(), backend::Error> {
        unreachable!("no upload over this link")
    }
}

/// Adds `entry` with the fields it has, the rest left to RouterOS.
pub async fn add<R: Creatable>(node: &impl Writes, entry: &R) -> anyhow::Result<()> {
    apply_mutation(node, &entry.calculate_create()).await?;
    Ok(())
}

/// Changes the one entry of `R`'s menu (`/ip/socks`) as `change` does: what differs is set.
pub async fn set_single<R: SingleResource + CfgResource + Clone>(
    node: &Session,
    change: impl FnOnce(&mut R),
) -> anyhow::Result<()> {
    let current = R::fetch(node)
        .await?
        .ok_or_else(|| anyhow::anyhow!("{} has no entry", String::from_utf8_lossy(R::path())))?;
    let mut target = current.clone();
    change(&mut target);
    apply_mutation(node, &generate_single_update(&current, &target)).await?;
    Ok(())
}

/// Changes the entry of `R`'s menu with `key` as `change` does: what differs is set.
pub async fn set_keyed<R: KeyedResource + Updatable<R> + Clone + Send>(
    node: &Session,
    key: &R::Key,
    change: impl FnOnce(&mut R),
) -> anyhow::Result<()>
where
    R::Key: Sync,
{
    let current = R::fetch(node, key).await?.ok_or_else(|| {
        anyhow::anyhow!("{} has no such entry", String::from_utf8_lossy(R::path()))
    })?;
    let mut target = current.clone();
    change(&mut target);
    apply_mutation(node, &target.calculate_update(&current)).await?;
    Ok(())
}

/// Whether `address` answers any of `count` pings from `node`.
pub async fn answers_ping(node: &Session, address: IpAddr, count: u8) -> bool {
    let ping = Ping {
        address,
        count,
        interval: Duration::from_secs(1),
    };
    match observe(node, &ping).await {
        Ok(rows) => rows.iter().any(|row| {
            backend::device::access::field(row, "received").is_some_and(|received| received != "0")
        }),
        Err(error) => {
            eprintln!("ping {address}: {error}");
            false
        }
    }
}

/// The node's `/system/identity`.
pub async fn identity(node: &Session) -> String {
    SystemIdentityCfg::fetch(node)
        .await
        .ok()
        .flatten()
        .map(|identity| identity.name.to_string())
        .unwrap_or_default()
}

/// Names the node `name` (`/system/identity`).
pub async fn set_identity(node: &Session, name: &str) {
    set_single(node, |identity: &mut SystemIdentityCfg| {
        identity.name = name.into()
    })
    .await
    .expect("identity not set");
}

/// Adds the script `name` with `source` (`/system/script`).
pub async fn add_script(node: &Session, name: &str, source: &str) {
    let mut script = SystemScriptCfg::new(name.into());
    script.source = Some(source.into());
    add(node, &script).await.expect("script added");
}

/// Runs `script` (a line of RouterOS script) in the background (`/execute`).
pub async fn execute(node: &Session, script: &str) {
    act(node, &Execute { script })
        .await
        .unwrap_or_else(|error| panic!("/execute {script}: {error}"));
}

/// Adds `address` (`a.b.c.d/len`) to `interface`.
pub async fn add_address(node: &Session, address: &str, interface: &str) {
    let address = IpAddressCfg {
        address: address.parse().expect("an IPv4 network"),
        interface: interface.into(),
        comment: None,
    };
    add(node, &address).await.expect("address added");
}

/// Lets the node forward SSH connections (`/ip ssh forwarding-enabled=local`), the tunnel the
/// provisioner reaches a new node through.
pub async fn forward_ssh(node: &Session) {
    set_single(node, |ssh: &mut IpSshCfg| {
        ssh.forwarding_enabled = IpSshForwardingEnabled::Local
    })
    .await
    .expect("ssh forwarding");
}
