//! The "realistic path" apply helper: pushes config to a target node through
//! a designated entry node's SOCKS proxy, instead of dialing the target
//! directly - mirroring how a real deployment only has one management entry
//! point and reaches everything else through routing. `/ip socks` on
//! RouterOS is a structural stand-in for a real VPN/jump-host gateway here
//! (same property: one access point, the rest only reachable via routing),
//! not the same transport a production entry point would use.
//!
//! Only the connection this module makes is meant to take that path. Every
//! other access in a scenario - setup, `fetch_config` reads, final
//! assertions - is allowed to go directly to a node's own hostfwd port.

use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::resource::{apply_mutation, ResourceMutation};
use std::net::{IpAddr, SocketAddr};
use tokio_socks::tcp::Socks5Stream;
use tokio_stream::StreamExt;

/// Enables `/ip socks` on `device` (an entry node), reachable on the
/// standard RouterOS SOCKS port (1080). Uses a direct connection - setting
/// up the entry point itself is allowed to be direct, only the connection
/// to the target through it is meant to be realistic.
pub async fn enable_socks(device: &MikrotikDevice<SimpleResult>) -> anyhow::Result<()> {
    let mut stream = device
        .send_command(
            b"/ip/socks/set".as_slice(),
            |cmd| {
                cmd.attribute(b"enabled".as_slice(), b"yes".as_slice())
                    .attribute(b"port".as_slice(), b"1080".as_slice())
                    // RouterOS defaults to SOCKS4 - tokio-socks (and the
                    // "realistic path" connection below) speaks SOCKS5.
                    .attribute(b"version".as_slice(), b"5".as_slice())
            },
            (),
        )
        .await;
    while let Some(result) = stream.next().await {
        if let SimpleResult::Trap { message, .. } = result {
            anyhow::bail!("enabling /ip socks failed: {message}");
        }
    }
    Ok(())
}

/// Dials `target_ip:8728` through the SOCKS proxy at
/// `127.0.0.1:{entry_socks_port}` (an entry node's hostfwd'd SOCKS port) and
/// logs in - the "realistic path" connection itself.
async fn connect_via_entry(
    entry_socks_port: u16,
    target_ip: IpAddr,
    password: &str,
) -> anyhow::Result<MikrotikDevice<SimpleResult>> {
    let stream = Socks5Stream::connect(
        ("127.0.0.1", entry_socks_port),
        SocketAddr::new(target_ip, 8728),
    )
    .await?;
    let device = MikrotikDevice::connect_with_stream(
        stream,
        b"admin",
        Some(encoding_rs::mem::encode_latin1_lossy(password)),
    )
    .await?;
    Ok(device)
}

/// Applies `mutations` to `target_ip` entirely through `entry`'s SOCKS
/// proxy - this is the one call in a scenario that must not use a target
/// node's own direct hostfwd access, even though it technically exists.
pub async fn apply_mutations_via_entry(
    entry_socks_port: u16,
    target_ip: IpAddr,
    mutations: &[ResourceMutation<'_>],
    password: &str,
) -> anyhow::Result<()> {
    let device = connect_via_entry(entry_socks_port, target_ip, password).await?;
    for mutation in mutations {
        apply_mutation(&device, mutation).await?;
    }
    Ok(())
}
