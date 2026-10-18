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
use backend::job::{Event, Log};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::time::Duration;
use tokio_stream::StreamExt;

const NEW_PASSWORD: &str = "Provisioned-1";

#[tokio::test]
#[ignore]
async fn mac_telnet_setup_from_entry() {
    mac_telnet_setup(30, "chr-network-mac-telnet-test").await;
}

/// The same with the cables as `/31` (`.0` the entry, `.1` the new device), the transfer
/// networks the helper „Verbindung“ hands out.
#[tokio::test]
#[ignore]
async fn mac_telnet_setup_over_31() {
    mac_telnet_setup(31, "chr-network-mac-telnet-31-test").await;
}

/// The entry's address and the new device's on cable `n` (0, 1) as `/len` (30 or 31).
fn cable_addresses(n: u8, len: u8) -> (String, String) {
    let base = n * 4 + if len == 31 { 0 } else { 1 };
    (
        format!("192.168.77.{base}/{len}"),
        format!("192.168.77.{}/{len}", base + 1),
    )
}

async fn mac_telnet_setup(len: u8, work_dir: &str) {
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
    let work_dir = std::env::temp_dir().join(work_dir);
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
    for (n, interface) in [(0, "ether2"), (1, "ether3")] {
        let address = cable_addresses(n, len).0;
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
    // an old config has its ports renamed: the setup finds the port by its factory name
    // (berghof-hansueli)
    let configured_api = configured.connect_api(crate::TEST_PASSWORD).await.expect("configured api");
    run(&configured_api, "/interface/ethernet/set", &[("numbers", "ether3"), ("name", "e03-old")]).await;
    drop(configured_api);

    for (node, slot, password) in [
        (&fresh, 1, NEW_PASSWORD),
        // has a password already: the factory login is refused, the second one works
        (&configured, 2, crate::TEST_PASSWORD),
    ] {
        let (neighbor, address) = cable_addresses(slot as u8 - 1, len);
        let neighbor = neighbor.split('/').next().unwrap();
        let address = address.as_str();
        let mut terminal = Terminal::connect(
            ([127, 0, 0, 1], entry.ssh_port).into(),
            "admin",
            Some(crate::TEST_PASSWORD),
        )
        .await
        .expect("ssh to entry");
        terminal.hide(password);
        let log = Log::new(&node.name);
        terminal.stream_to(&log);
        let setup = MacTelnetSetup {
            macs: [mac_of(node.index, slot)].into(),
            port: format!("ether{}", slot + 1),
            address: address.parse().unwrap(),
            on_bridge: false,
            neighbor_port: None,
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
        // the same streamed, line by line
        let streamed = log
            .events()
            .into_iter()
            .map(|event| match event {
                Event::ConsoleOutput(output) => output.text.to_string(),
                other => panic!("{other:?}"),
            })
            .collect::<String>();
        assert!(streamed.contains("Welcome back!"), "{streamed}");
        assert!(!streamed.contains(password), "the password must not be streamed");

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

/// The setup goes through when the device stops answering for a while (the VM stopped for
/// 10 s) at any point of it: then `/tool mac-telnet` hangs, gives up after `Connected`, says
/// "incorrect username or password" for the right password, or the session ends between two
/// commands. The points are offsets from the start of the setup, which takes about 4 s here:
/// before the connection, in the login, in the commands.
#[tokio::test]
#[ignore]
async fn mac_telnet_setup_survives_a_stalled_device() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-mac-telnet-stall-test");
    let link = free_tcp_port().unwrap();
    let entry = boot_node(&golden, &work_dir, "entry", &[PortWiring::Listen(link)], crate::TEST_PASSWORD)
        .await
        .expect("entry boot failed");
    let target = boot_node(&golden, &work_dir, "target", &[PortWiring::Connect(link)], crate::TEST_PASSWORD)
        .await
        .expect("target boot failed");
    let setup = MacTelnetSetup {
        macs: [mac_of(target.index, 1)].into(),
        port: "ether2".into(),
        address: "192.168.77.2/30".parse().unwrap(),
        on_bridge: false,
        neighbor_port: Some("ether2".into()),
        neighbor_address: "192.168.77.1".parse().unwrap(),
    };
    for offset in [0, 500, 1500, 2500, 3200] {
        let mut terminal = Terminal::connect(
            ([127, 0, 0, 1], entry.ssh_port).into(),
            "admin",
            Some(crate::TEST_PASSWORD),
        )
        .await
        .expect("ssh to entry");
        terminal.hide(crate::TEST_PASSWORD);
        let stall = async {
            tokio::time::sleep(Duration::from_millis(offset)).await;
            target.stall(Duration::from_secs(10)).await
        };
        let (result, stalled) =
            tokio::join!(set_up(&mut terminal, &setup, "admin", crate::TEST_PASSWORD), stall);
        stalled.expect("stall the target");
        if let Err(error) = result {
            panic!(
                "setup with the target stalled after {offset} ms failed: {error}\n{}",
                terminal.transcript()
            );
        }
        terminal.close().await;
    }
    let api = target.connect_api(crate::TEST_PASSWORD).await.expect("target api");
    let mut addresses = api
        .send_simple_command(b"/ip/address/print".as_slice(), ())
        .await;
    let mut found = false;
    while let Some(row) = addresses.next().await {
        if let SimpleResult::Sentence(row) = &row
            && row.get("address").and_then(|v| v.as_deref()) == Some("192.168.77.2/30")
        {
            found = true;
        }
    }
    assert!(found, "no 192.168.77.2/30 on the target");
}

/// Runs `command` with `attributes`, printing the answers.
async fn run(device: &MikrotikDevice<SimpleResult>, command: &str, attributes: &[(&str, &str)]) {
    let mut stream = device
        .send_command(
            command.as_bytes(),
            |cmd| {
                attributes
                    .iter()
                    .fold(cmd, |cmd, (k, v)| cmd.attribute(k.as_bytes(), v.as_bytes()))
            },
            (),
        )
        .await;
    while let Some(r) = stream.next().await {
        eprintln!("{command}: {r:?}");
    }
}

/// The rows of `/ip/address/print` with `address`: whether each is dynamic.
async fn address_rows(device: &MikrotikDevice<SimpleResult>, address: &str) -> Vec<bool> {
    let mut stream = device.send_simple_command(b"/ip/address/print".as_slice(), ()).await;
    let mut rows = Vec::new();
    while let Some(row) = stream.next().await {
        if let SimpleResult::Sentence(row) = &row
            && row.get("address").and_then(|v| v.as_deref()) == Some(address)
        {
            rows.push(row.get("dynamic").and_then(|v| v.as_deref()) == Some("true"));
        }
    }
    rows
}

/// A device whose bridge holds the setup address from DHCP (link-5-station: its old config's
/// client, served by the router on the other side of the radio link): RouterOS refuses the
/// address statically next to the lease, so the setup turns the client off and adds it.
#[tokio::test]
#[ignore]
async fn mac_telnet_setup_takes_the_address_from_a_dhcp_client() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    let work_dir = std::env::temp_dir().join("chr-network-mac-telnet-dhcp-test");
    let link = free_tcp_port().unwrap();
    let entry = boot_node(&golden, &work_dir, "entry", &[PortWiring::Listen(link)], crate::TEST_PASSWORD)
        .await
        .expect("entry boot failed");
    let target = boot_node(&golden, &work_dir, "target", &[PortWiring::Connect(link)], crate::TEST_PASSWORD)
        .await
        .expect("target boot failed");
    let entry_api = entry.connect_api(crate::TEST_PASSWORD).await.expect("entry api");
    let target_api = target.connect_api(crate::TEST_PASSWORD).await.expect("target api");
    let address = "192.168.77.4/29";
    run(&entry_api, "/ip/address/add", &[("address", "192.168.77.1/29"), ("interface", "ether2")]).await;
    run(&entry_api, "/ip/pool/add", &[("name", "one"), ("ranges", "192.168.77.4")]).await;
    run(&entry_api, "/ip/dhcp-server/network/add", &[("address", "192.168.77.0/29"), ("gateway", "192.168.77.1")]).await;
    run(&entry_api, "/ip/dhcp-server/add", &[("name", "lan"), ("interface", "ether2"), ("address-pool", "one")]).await;
    run(&entry_api, "/ip/ssh/set", &[("forwarding-enabled", "local")]).await;
    run(&target_api, "/interface/ethernet/set", &[("numbers", "ether2"), ("name", "e02-old")]).await;
    run(&target_api, "/interface/bridge/add", &[("name", "bridge-lan")]).await;
    run(&target_api, "/interface/bridge/port/add", &[("bridge", "bridge-lan"), ("interface", "e02-old")]).await;
    run(&target_api, "/ip/dhcp-client/add", &[("interface", "bridge-lan")]).await;
    wait_until(
        || async { Ok(address_rows(&target_api, address).await == [true]) },
        Duration::from_secs(120),
    )
    .await
    .expect("no lease on the target");

    let mut terminal = Terminal::connect(
        ([127, 0, 0, 1], entry.ssh_port).into(),
        "admin",
        Some(crate::TEST_PASSWORD),
    )
    .await
    .expect("ssh to entry");
    terminal.hide(crate::TEST_PASSWORD);
    let setup = MacTelnetSetup {
        macs: [mac_of(target.index, 1)].into(),
        port: "ether2".into(),
        address: address.parse().unwrap(),
        on_bridge: true,
        neighbor_port: Some("ether2".into()),
        neighbor_address: "192.168.77.1".parse().unwrap(),
    };
    let result = set_up(&mut terminal, &setup, "admin", crate::TEST_PASSWORD).await;
    let transcript = terminal.transcript();
    eprintln!("--- transcript ---\n{transcript}");
    result.unwrap_or_else(|e| panic!("setup failed: {e}"));
    terminal.close().await;
    assert!(transcript.contains("disabled: it held 192.168.77.4/29"), "{transcript}");

    assert_eq!(address_rows(&target_api, address).await, [false], "the static address only");
    let mut clients = target_api.send_simple_command(b"/ip/dhcp-client/print".as_slice(), ()).await;
    while let Some(row) = clients.next().await {
        if let SimpleResult::Sentence(row) = &row
            && row.get("interface").and_then(|v| v.as_deref()) == Some("bridge-lan")
        {
            assert_eq!(row.get("disabled").and_then(|v| v.as_deref()), Some("true"), "{row:?}");
        }
    }
    let ssh = tokio::net::TcpStream::connect(("127.0.0.1", entry.ssh_port))
        .await
        .expect("ssh to entry");
    let tunnel = Tunnel::connect_with_stream(ssh, "admin", Some(crate::TEST_PASSWORD))
        .await
        .expect("tunnel to entry");
    let stream = tunnel.open("192.168.77.4".parse().unwrap(), 8728).await.expect("tunnel");
    MikrotikDevice::<SimpleResult>::connect_with_stream(
        stream,
        b"admin",
        Some(crate::TEST_PASSWORD.as_bytes()),
    )
    .await
    .expect("API through the entry at the static address");
}
