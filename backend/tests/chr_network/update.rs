//! RouterOS update pushed by the provisioner (`backend::device::update`) against a real CHR: a
//! factory node of an older release, given a password and an address over mac-telnet from an
//! entry node like a new device in the network, is updated to the current release - packages
//! downloaded from MikroTik, copied over SCP, reboot. Reached like the provisioner reaches such
//! a device: through an SSH tunnel to the entry, which stays open while the node reboots.
use crate::chr_network::golden_image::{CHR, CHR_OLDER, ensure_factory_image, ensure_golden_image};
use crate::chr_network::mac_telnet::wait_for_boot;
use crate::chr_network::qemu::{free_tcp_port, nix_unavailable};
use crate::chr_network::vm::{PortWiring, boot_node, mac_of, start_node};
use backend::Error;
use backend::device::bootstrap::{MacTelnetSetup, set_up};
use backend::device::terminal::{Terminal, Tunnel, upload};
use backend::device::update::{DeviceLink, Version, read_state, update};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::net::IpAddr;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;

const PASSWORD: &str = "Updated-1";

/// Through a tunnel to the entry: the node at its new address, which it has no route back from.
struct TunnelLink {
    tunnel: Tunnel,
    address: IpAddr,
}

impl DeviceLink for TunnelLink {
    async fn api(&self) -> Result<MikrotikDevice<SimpleResult>, Error> {
        let stream = self.tunnel.open(self.address, 8728).await?;
        Ok(
            MikrotikDevice::connect_with_stream(stream, b"admin", Some(encode_latin1_lossy(PASSWORD)))
                .await?,
        )
    }
    async fn upload(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        let stream = self.tunnel.open(self.address, 22).await?;
        Ok(upload(stream, "admin", Some(PASSWORD), name, data).await?)
    }
}

#[tokio::test]
#[ignore]
async fn update_older_chr_to_current() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let older = ensure_factory_image(&crate::cache_dir(), &CHR_OLDER)
        .await
        .expect("older image failed");
    let work_dir = std::env::temp_dir().join("chr-network-update-test");
    let cable = free_tcp_port().unwrap();
    let entry = boot_node(
        &golden,
        &work_dir,
        "entry",
        &[PortWiring::Listen(cable)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let old = start_node(&older, &work_dir, "old", &[PortWiring::Connect(cable)])
        .await
        .expect("older node boot failed");
    let entry_api = entry.connect_api(crate::TEST_PASSWORD).await.expect("entry api");
    let mut s = entry_api
        .send_command(
            b"/ip/address/add".as_slice(),
            |cmd| {
                cmd.attribute(b"address".as_slice(), b"192.168.78.1/30".as_slice())
                    .attribute(b"interface".as_slice(), b"ether2".as_slice())
            },
            (),
        )
        .await;
    while s.next().await.is_some() {}
    let mut s = entry_api
        .send_command(
            b"/ip/ssh/set".as_slice(),
            |cmd| cmd.attribute(b"forwarding-enabled".as_slice(), b"local".as_slice()),
            (),
        )
        .await;
    while s.next().await.is_some() {}
    wait_for_boot(&old).await;

    // like a new device: password and address over mac-telnet first
    let mut terminal = Terminal::connect(
        ([127, 0, 0, 1], entry.ssh_port).into(),
        "admin",
        Some(crate::TEST_PASSWORD),
    )
    .await
    .expect("ssh to entry");
    let setup = MacTelnetSetup {
        macs: vec![mac_of(old.index, 1)],
        port: "ether2".into(),
        address: "192.168.78.2/30".parse().unwrap(),
        on_bridge: false,
        neighbor_address: "192.168.78.1".parse().unwrap(),
    };
    set_up(&mut terminal, &setup, "admin", PASSWORD)
        .await
        .expect("setup over mac-telnet");
    terminal.close().await;

    let ssh = TcpStream::connect(("127.0.0.1", entry.ssh_port))
        .await
        .expect("ssh to entry");
    let link = TunnelLink {
        tunnel: Tunnel::connect_with_stream(ssh, "admin", Some(crate::TEST_PASSWORD))
            .await
            .expect("tunnel to entry"),
        address: "192.168.78.2".parse().unwrap(),
    };
    let before = read_state(&link.api().await.unwrap()).await.unwrap();
    assert_eq!(before.version, Version::parse(CHR_OLDER.version).unwrap());

    let target = Version::parse(CHR.version).unwrap();
    let result = update(&link, &target, &[]).await;
    eprintln!("--- update log ---\n{}", result.log);
    assert_eq!(result.error, None);
    assert_eq!(result.from, CHR_OLDER.version);
    assert_eq!(result.to, CHR.version);
    let after = read_state(&link.api().await.unwrap()).await.unwrap();
    assert_eq!(after.version, target);
}
