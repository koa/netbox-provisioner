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
use backend::device::power::PowerChain;
use backend::device::terminal::{Terminal, Tunnel, upload};
use backend::device::update::{DeviceLink, Downgraded, UpdateDone, Version, downgrade, read_state, update};
use backend::device::update_fallback::{UpdateWithoutFallback, WayBack};
use backend::device_state::UpdateState;
use backend::job::{Event, Log};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::net::IpAddr;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;

const PASSWORD: &str = "Updated-1";

/// A job's log as the mutation hands it on: past `Reading` (`device_state::UpdateState`).
fn reading() -> Log {
    let log = Log::new("chr");
    log.enter(UpdateState::Reading);
    log
}

/// The job kept to the update's state machine.
fn kept_to_states(log: &Log) {
    assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
}

/// Straight to a node's forwarded ports, with the harness's password.
struct DirectLink {
    api_port: u16,
    ssh_port: u16,
}

impl DeviceLink for DirectLink {
    async fn api(&self) -> Result<MikrotikDevice<SimpleResult>, Error> {
        Ok(MikrotikDevice::connect(
            ("127.0.0.1", self.api_port),
            b"admin",
            Some(encode_latin1_lossy(crate::TEST_PASSWORD)),
        )
        .await?)
    }
    async fn upload(
        &self,
        name: &str,
        data: &[u8],
        progress: &(dyn Fn(usize) + Sync),
    ) -> Result<(), Error> {
        let stream = TcpStream::connect(("127.0.0.1", self.ssh_port)).await.expect("ssh to the node");
        Ok(upload(stream, "admin", Some(crate::TEST_PASSWORD), name, data, progress).await?)
    }
}

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
    async fn upload(
        &self,
        name: &str,
        data: &[u8],
        progress: &(dyn Fn(usize) + Sync),
    ) -> Result<(), Error> {
        let stream = self.tunnel.open(self.address, 22).await?;
        Ok(upload(stream, "admin", Some(PASSWORD), name, data, progress).await?)
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
        macs: [mac_of(old.index, 1)].into(),
        port: "ether2".into(),
        address: "192.168.78.2/30".parse().unwrap(),
        on_bridge: false,
        neighbor_port: Some("ether2".into()),
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

    // cancelled once the package is downloaded: it is uploaded, then removed again
    let log = reading();
    let cancel = async {
        while !log.events().iter().any(|e| matches!(e, Event::PackageDownloaded(_))) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        log.cancel();
    };
    let power = PowerChain::unpowered("chr");
    let fallback = std::time::Duration::from_secs(900);
    // the CHR is reached through the entry, its way back
    let way_back = WayBack {
        device: "chr".into(),
        neighbors: ["entry".into()].into(),
        confirmed: false,
    };

    // neither partitions nor a neighbor nor confirmed: refused before anything is downloaded
    let alone = reading();
    let without = WayBack {
        neighbors: [].into(),
        ..way_back.clone()
    };
    let result = update(&link, &power, &target, &[], fallback, &without, &alone).await;
    assert!(matches!(result, Err(Error::NoUpdateFallback(_))), "{result:?}");
    assert!(!alone.events().iter().any(|e| matches!(e, Event::PackageDownloaded(_))));

    let (result, ()) =
        tokio::join!(update(&link, &power, &target, &[], fallback, &way_back, &log), cancel);
    eprintln!("--- cancelled update log ---\n{:#?}", log.events());
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    kept_to_states(&log);
    assert!(log.events().iter().any(|e| matches!(e, Event::PackageUploaded(_))));
    let api = link.api().await.unwrap();
    let mut files = api.send_simple_command(b"/file/print".as_slice(), ()).await;
    while let Some(row) = files.next().await {
        if let SimpleResult::Sentence(row) = row {
            let name = row.get("name").cloned().flatten().unwrap_or_default();
            assert!(!name.ends_with(".npk"), "{name} left on the device");
        }
    }
    drop(api);
    assert_eq!(read_state(&link.api().await.unwrap()).await.unwrap().version, before.version);

    let log = reading();
    let result = update(&link, &power, &target, &[], fallback, &way_back, &log).await;
    eprintln!("--- update log ---\n{:#?}", log.events());
    result.expect("update");
    kept_to_states(&log);
    // a CHR has no partitions to fall back to
    assert!(log.events().contains(&Event::UpdateWithoutFallback(UpdateWithoutFallback {
        partitions: 0,
        neighbors: ["entry".into()].into(),
    })));
    assert_eq!(
        log.events().last(),
        Some(&Event::UpdateDone(UpdateDone {
            from: CHR_OLDER.version.into(),
            to: CHR.version.into(),
        }))
    );
    let after = read_state(&link.api().await.unwrap()).await.unwrap();
    assert_eq!(after.version, target);
}

/// The current release taken back to the older one with `/system/package/downgrade`: the
/// device page's rescue after an update.
#[tokio::test]
#[ignore]
async fn downgrade_chr_to_older() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-downgrade-test");
    let node = boot_node(&golden, &work_dir, "current", &[], crate::TEST_PASSWORD)
        .await
        .expect("boot failed");
    let link = DirectLink {
        api_port: node.api_port,
        ssh_port: node.ssh_port,
    };
    let before = read_state(&link.api().await.unwrap()).await.unwrap();
    assert_eq!(before.version, Version::parse(CHR.version).unwrap());
    let power = PowerChain::unpowered("chr");
    // no neighbor here, updating without a way back confirmed
    let way_back = WayBack {
        device: "chr".into(),
        neighbors: [].into(),
        confirmed: true,
    };

    // only to an older version: refused before anything is downloaded
    let log = reading();
    let newer = Version::parse("7.99").unwrap();
    let result = downgrade(&link, &power, &newer, &way_back, &log).await;
    assert!(matches!(result, Err(Error::NoDowngrade { .. })), "{result:?}");
    assert!(!log.events().iter().any(|e| matches!(e, Event::PackageDownloaded(_))));

    let older = Version::parse(CHR_OLDER.version).unwrap();
    let log = reading();
    let result = downgrade(&link, &power, &older, &way_back, &log).await;
    eprintln!("--- downgrade log ---\n{:#?}", log.events());
    result.expect("downgrade");
    kept_to_states(&log);
    assert_eq!(
        log.events().last(),
        Some(&Event::Downgraded(Downgraded {
            from: CHR.version.into(),
            to: CHR_OLDER.version.into(),
        }))
    );
    assert_eq!(read_state(&link.api().await.unwrap()).await.unwrap().version, older);
}
