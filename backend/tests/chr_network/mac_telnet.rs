//! Scope: CHR tests of the initial setup over mac-telnet; nothing else.
//!
//! Initial setup over `/tool mac-telnet` (`backend::device::bootstrap::set_up`) against real
//! CHRs: an entry node with a password, cabled to one node at factory settings (license not yet
//! accepted, `admin` without password, so RouterOS asks for a new one) and one that has the
//! password already. Both must end up answering the API at their new address with the password,
//! reached the way the provisioner reaches them: through an SSH tunnel to the entry
//! (`forwarding-enabled=local`), so from the entry's address on the cable, with no route back.
use crate::chr_network::golden_image::{CHR, ensure_factory_image, ensure_golden_image};
use crate::chr_network::node::{
    add, add_address, answers_ping, forward_ssh, login_over, set_keyed,
};
use crate::chr_network::qemu::{free_tcp_port, nix_unavailable};
use crate::chr_network::vm::{PortWiring, RunningNode, boot_node, mac_of, start_node};
use crate::chr_network::wait::wait_until;
use backend::device::access::Session;
use backend::device::bootstrap::{MacTelnetSetup, set_up};
use backend::device::terminal::{Terminal, Tunnel};
use backend::job::{Event, Log};
use mikrotik_api::prelude::{Connection, Generation};
use mikrotik_model::model::{
    InterfaceBridgeCfg, InterfaceBridgePortCfg, InterfaceEthernetByDefaultName, IpAddress,
    IpDhcpClient, IpDhcpClientCfg, IpDhcpServerBootpLeaseTime, IpDhcpServerBootpSupport,
    IpDhcpServerCfg, IpDhcpServerNetworkCfg, IpDhcpServerUseRadius, IpPoolCfg, YesNo,
};
use mikrotik_model::resource::collect_resource;
use std::net::IpAddr;
use std::time::Duration;

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
        &[
            PortWiring::Listen(to_factory),
            PortWiring::Listen(to_configured),
        ],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let fresh = start_node(
        &factory,
        &work_dir,
        "fresh",
        &[PortWiring::Connect(to_factory)],
    )
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

    let entry_api = entry
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("entry api");
    for (n, interface) in [(0, "ether2"), (1, "ether3")] {
        add_address(&entry_api, &cable_addresses(n, len).0, interface).await;
    }
    forward_ssh(&entry_api).await;
    wait_for_boot(&fresh).await;
    // an old config has its ports renamed: the setup finds the port by its factory name
    // (berghof-hansueli)
    let configured_api = configured
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("configured api");
    rename_port(&configured_api, "ether3", "e03-old").await;
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
            port: format!("ether{}", slot + 1).into(),
            address: address.parse().unwrap(),
            on_bridge: false,
            neighbor_port: None,
            neighbor_address: neighbor.parse().unwrap(),
        };
        let result = set_up(&mut terminal, &setup, "admin", password, None).await;
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
        assert!(
            !streamed.contains(password),
            "the password must not be streamed"
        );

        // reachable from the entry over the cable, with the password; emulated without KVM,
        // with other tests' VMs beside it, the address took longer than 30 s to answer
        let target: IpAddr = address.split('/').next().unwrap().parse().unwrap();
        wait_until(
            || async { Ok(answers_ping(&entry_api, target, 2).await) },
            Duration::from_secs(120),
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
            .open(target, 8728)
            .await
            .unwrap_or_else(|e| panic!("no tunnel to {}: {e}", node.name));
        let tunneled = login_over(stream, password)
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
                Connection::connect(
                    ("127.0.0.1", node.api_port),
                    b"admin",
                    None::<&[u8]>,
                    Generation::default(),
                )
                .await,
                Ok(_) | Err(mikrotik_api::error::Error::LoginFailed)
            ))
        },
        Duration::from_secs(180),
    )
    .await
    .expect("factory node never came up");
}

/// Names the Ethernet port with the factory name `port` `name`, as an old config has it.
async fn rename_port(node: &Session, port: &str, name: &str) {
    set_keyed(
        node,
        &port.into(),
        |ethernet: &mut InterfaceEthernetByDefaultName| ethernet.data.name = name.into(),
    )
    .await
    .expect("port renamed");
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
    let entry = boot_node(
        &golden,
        &work_dir,
        "entry",
        &[PortWiring::Listen(link)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let target = boot_node(
        &golden,
        &work_dir,
        "target",
        &[PortWiring::Connect(link)],
        crate::TEST_PASSWORD,
    )
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
        let (result, stalled) = tokio::join!(
            set_up(&mut terminal, &setup, "admin", crate::TEST_PASSWORD, None),
            stall
        );
        stalled.expect("stall the target");
        if let Err(error) = result {
            panic!(
                "setup with the target stalled after {offset} ms failed: {error}\n{}",
                terminal.transcript()
            );
        }
        terminal.close().await;
    }
    let api = target
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("target api");
    assert_eq!(
        address_rows(&api, "192.168.77.2/30").await,
        [false],
        "no 192.168.77.2/30 on the target"
    );
}

/// The entries of `/ip/address` with `address`: whether each is dynamic.
async fn address_rows(node: &Session, address: &str) -> Vec<bool> {
    let address = address.parse().expect("an IPv4 network");
    collect_resource::<IpAddress>(node)
        .await
        .expect("addresses")
        .iter()
        .filter(|entry| entry.cfg.address == address)
        .map(|entry| entry.status.dynamic)
        .collect()
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
    let entry = boot_node(
        &golden,
        &work_dir,
        "entry",
        &[PortWiring::Listen(link)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("entry boot failed");
    let target = boot_node(
        &golden,
        &work_dir,
        "target",
        &[PortWiring::Connect(link)],
        crate::TEST_PASSWORD,
    )
    .await
    .expect("target boot failed");
    let entry_api = entry
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("entry api");
    let target_api = target
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("target api");
    let address = "192.168.77.4/29";
    add_address(&entry_api, "192.168.77.1/29", "ether2").await;
    let mut pool = IpPoolCfg::new("one".into());
    pool.ranges = ["192.168.77.4".into()].into();
    add(&entry_api, &pool).await.expect("pool");
    let mut network = IpDhcpServerNetworkCfg::new("192.168.77.0/29".parse().unwrap(), false, 29);
    network.gateway = ["192.168.77.1".parse().unwrap()].into();
    add(&entry_api, &network).await.expect("dhcp network");
    let server = IpDhcpServerCfg::new(
        "one".into(),
        IpDhcpServerBootpLeaseTime::Forever,
        IpDhcpServerBootpSupport::Static,
        false,
        "ether2".into(),
        "lan".into(),
        IpDhcpServerUseRadius::No,
    );
    add(&entry_api, &server).await.expect("dhcp server");
    forward_ssh(&entry_api).await;
    rename_port(&target_api, "ether2", "e02-old").await;
    let bridge = InterfaceBridgeCfg {
        name: "bridge-lan".into(),
        ..InterfaceBridgeCfg::default()
    };
    add(&target_api, &bridge).await.expect("bridge");
    let port = InterfaceBridgePortCfg {
        bridge: "bridge-lan".into(),
        interface: "e02-old".into(),
        ..InterfaceBridgePortCfg::default()
    };
    add(&target_api, &port).await.expect("bridge port");
    let client = IpDhcpClientCfg::new("bridge-lan".into(), YesNo::Yes, 1, true, true);
    add(&target_api, &client).await.expect("dhcp client");
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
    let result = set_up(&mut terminal, &setup, "admin", crate::TEST_PASSWORD, None).await;
    let transcript = terminal.transcript();
    eprintln!("--- transcript ---\n{transcript}");
    result.unwrap_or_else(|e| panic!("setup failed: {e}"));
    terminal.close().await;
    assert!(
        transcript.contains("disabled: it held 192.168.77.4/29"),
        "{transcript}"
    );

    assert_eq!(
        address_rows(&target_api, address).await,
        [false],
        "the static address only"
    );
    for client in collect_resource::<IpDhcpClient>(&target_api)
        .await
        .expect("dhcp clients")
        .iter()
        .filter(|client| *client.cfg.interface.0 == *b"bridge-lan")
    {
        assert!(client.status.disabled, "{client:?}");
    }
    let ssh = tokio::net::TcpStream::connect(("127.0.0.1", entry.ssh_port))
        .await
        .expect("ssh to entry");
    let tunnel = Tunnel::connect_with_stream(ssh, "admin", Some(crate::TEST_PASSWORD))
        .await
        .expect("tunnel to entry");
    let stream = tunnel
        .open("192.168.77.4".parse().unwrap(), 8728)
        .await
        .expect("tunnel");
    login_over(stream, crate::TEST_PASSWORD)
        .await
        .expect("API through the entry at the static address");
}
