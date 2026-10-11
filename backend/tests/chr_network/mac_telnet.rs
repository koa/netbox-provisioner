//! Initial setup over `/tool mac-telnet` (`backend::device::bootstrap::set_up`) against real
//! CHRs: an entry node with a password, cabled to one node at factory settings (license not yet
//! accepted, `admin` without password, so RouterOS asks for a new one) and one that has the
//! password already. Both must end up answering the API at their new address with the password,
//! reached the way the provisioner reaches them: through an SSH tunnel to the entry
//! (`forwarding-enabled=local`), so from the entry's address on the cable, with no route back.
use crate::chr_network::golden_image::{CHR, ensure_factory_image, ensure_golden_image};
use crate::chr_network::qemu::{free_tcp_port, nix_unavailable};
use crate::chr_network::vm::{PortWiring, RunningNode, boot_node, mac_of, start_node};
use crate::chr_network::wait::wait_until;
use backend::device::bootstrap::{MacTelnetSetup, set_up};
use backend::device::terminal::{Terminal, Tunnel};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio_stream::StreamExt;

const NEW_PASSWORD: &str = "Provisioned-1";

#[tokio::test]
#[ignore]
async fn mac_telnet_setup_from_entry() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let factory = ensure_factory_image(&crate::cache_dir(), &CHR)
        .await
        .expect("factory image failed");
    let work_dir = std::env::temp_dir().join("chr-network-mac-telnet-test");
    let (to_factory, to_configured) = (free_tcp_port().unwrap(), free_tcp_port().unwrap());
    let entry = boot_node(
        &golden,
        &work_dir,
        "entry",
        &[PortWiring::Listen(to_factory), PortWiring::Listen(to_configured)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let fresh = start_node(&factory, &work_dir, "fresh", &[PortWiring::Connect(to_factory)])
        .await
        .expect("factory node boot failed");
    let configured = boot_node(
        &golden,
        &work_dir,
        "configured",
        &[PortWiring::Idle, PortWiring::Connect(to_configured)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("configured node boot failed");

    let entry_api = entry.connect_api(crate::TEST_PASSWORD).await.expect("entry api");
    for address in ["192.168.77.1/30", "192.168.77.5/30"] {
        let interface = if address.ends_with(".1/30") { "ether2" } else { "ether3" };
        let mut s = entry_api
            .send_command(
                b"/ip/address/add".as_slice(),
                |cmd| {
                    cmd.attribute(b"address".as_slice(), address.as_bytes())
                        .attribute(b"interface".as_slice(), interface.as_bytes())
                },
                (),
            )
            .await;
        while let Some(r) = s.next().await {
            eprintln!("entry address: {r:?}");
        }
    }
    let mut s = entry_api
        .send_command(
            b"/ip/ssh/set".as_slice(),
            |cmd| cmd.attribute(b"forwarding-enabled".as_slice(), b"local".as_slice()),
            (),
        )
        .await;
    while let Some(r) = s.next().await {
        eprintln!("entry ssh forwarding: {r:?}");
    }
    wait_for_boot(&fresh).await;

    for (node, slot, address, neighbor, password) in [
        (&fresh, 1, "192.168.77.2/30", "192.168.77.1", NEW_PASSWORD),
        // has a password already: the factory login is refused, the second one works
        (&configured, 2, "192.168.77.6/30", "192.168.77.5", crate::TEST_PASSWORD),
    ] {
        let mut terminal = Terminal::connect(
            ([127, 0, 0, 1], entry.ssh_port).into(),
            "admin",
            Some(crate::TEST_PASSWORD),
        )
        .await
        .expect("ssh to entry");
        terminal.hide(password);
        let setup = MacTelnetSetup {
            macs: vec![mac_of(node.index, slot)],
            port: format!("ether{}", slot + 1),
            address: address.parse().unwrap(),
            on_bridge: false,
            neighbor_address: neighbor.parse().unwrap(),
        };
        let result = set_up(&mut terminal, &setup, "admin", password).await;
        let transcript = terminal.transcript();
        eprintln!("--- transcript {} ---\n{transcript}", node.name);
        result.unwrap_or_else(|e| panic!("setup of {} failed: {e}", node.name));
        terminal.close().await;
        assert!(
            !transcript.contains(password),
            "the password must not be in the transcript"
        );

        // reachable from the entry over the cable, with the password
        let target = address.split('/').next().unwrap().to_string();
        wait_until(
            || {
                let target = target.clone();
                let entry_api = &entry_api;
                async move { Ok(ping(entry_api, &target).await) }
            },
            Duration::from_secs(30),
        )
        .await
        .unwrap_or_else(|e| panic!("{} not reachable at {target}: {e}", node.name));
        let ssh = tokio::net::TcpStream::connect(("127.0.0.1", entry.ssh_port))
            .await
            .expect("ssh to entry");
        let tunnel = Tunnel::connect_with_stream(ssh, "admin", Some(crate::TEST_PASSWORD))
            .await
            .expect("tunnel to entry");
        let stream = tunnel
            .open(target.parse().unwrap(), 8728)
            .await
            .unwrap_or_else(|e| panic!("no tunnel to {}: {e}", node.name));
        let tunneled: MikrotikDevice<SimpleResult> = MikrotikDevice::connect_with_stream(
            stream,
            b"admin",
            Some(encode_latin1_lossy(password)),
        )
        .await
        .unwrap_or_else(|e| panic!("{} not reached through the entry: {e}", node.name));
        drop(tunneled);
    }
}

/// A factory CHR can't be logged in to over the API before its first console login; it is up
/// once the API port answers at all (with a login error).
pub async fn wait_for_boot(node: &RunningNode) {
    wait_until(
        || async {
            Ok(matches!(
                MikrotikDevice::<SimpleResult>::connect(("127.0.0.1", node.api_port), b"admin", None::<&[u8]>)
                    .await,
                Ok(_) | Err(mikrotik_api::error::Error::LoginFailed)
            ))
        },
        Duration::from_secs(180),
    )
    .await
    .expect("factory node never came up");
}

pub async fn ping(device: &MikrotikDevice<SimpleResult>, address: &str) -> bool {
    let mut stream = device
        .send_command(
            b"/ping".as_slice(),
            |cmd| {
                cmd.attribute(b"address".as_slice(), address.as_bytes())
                    .attribute(b"count".as_slice(), b"2".as_slice())
            },
            (),
        )
        .await;
    let mut received = false;
    while let Some(result) = stream.next().await {
        if let SimpleResult::Sentence(row) = &result
            && row.get("received").and_then(|v| v.as_deref()) == Some("1")
        {
            received = true;
        }
    }
    received
}
