//! Boots disposable per-node CHR VMs on top of a [`GoldenImage`], wired up
//! per a small, explicit NIC-slot plan (see [`PortWiring`]). Every node gets
//! its own `qcow2` overlay, so a hard `kill -9` (or just dropping the
//! [`RunningNode`]) is always safe - nothing here ever touches the golden
//! image file itself.

use crate::chr_network::golden_image::GoldenImage;
use crate::chr_network::qemu::{free_tcp_port, qemu_command, qemu_img_command};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tokio::process::Child;
use tokio::time::Instant;

/// How a node's NIC slot (beyond slot 0, which is always the host-reachable
/// management NIC) is wired for a given test run.
#[derive(Debug, Clone, Copy)]
pub enum PortWiring {
    /// Not used by this scenario, but the slot must still exist so later
    /// slots keep their correct PCI position (see hwconfig::DeviceType's
    /// ordered port list) - an unconnected `user` netdev placeholder.
    Idle,
    /// This end of a cable binds a TCP socket and waits for the other end
    /// to connect.
    Listen(u16),
    /// This end of a cable dials the TCP port the other end is listening
    /// on. Only valid once that listener is actually bound - callers must
    /// spawn `Listen` nodes before `Connect` nodes.
    Connect(u16),
}

pub struct RunningNode {
    pub name: String,
    /// Host-reachable port for the native API (hostfwd onto slot 0's NIC).
    /// Direct access via this port is always allowed, even for the
    /// "realistic path" scenarios - only the specific apply-under-test call
    /// is meant to avoid it.
    pub api_port: u16,
    /// Host-reachable port forwarded to this node's guest port 1080
    /// (hostfwd onto slot 0's NIC, same as `api_port`). Only meaningful once
    /// `/ip/socks` has actually been enabled on the node - a node acting as
    /// a scenario's "entry" node uses this to let the test harness reach it
    /// as a SOCKS proxy from the host, the way the "realistic apply path"
    /// is meant to be reached. Forwarded unconditionally on every node since
    /// it costs nothing when unused.
    pub socks_port: u16,
    /// Host-reachable port forwarded to this node's SSH (guest port 22).
    pub ssh_port: u16,
    /// Number of this node in the run; its NIC in slot `i` > 0 has the MAC
    /// [`mac_of`]`(index, i)`, so cabled nodes never share one (QEMU's
    /// default would give every node the same).
    pub index: u8,
    child: Child,
    overlay_path: PathBuf,
}

impl RunningNode {
    pub async fn connect_api(
        &self,
        password: &str,
    ) -> anyhow::Result<MikrotikDevice<SimpleResult>> {
        connect_api_with_retry(self.api_port, password, Duration::from_secs(30))
            .await
            .map_err(|e| e.context(format!("connecting to node {:?}", self.name)))
    }

    /// Sends SIGKILL and waits for exit - a hard `kill -9`, not a graceful
    /// shutdown. Only ever safe to do on an overlay-booted node (see this
    /// module's doc comment); never on the golden image build path, which
    /// uses a real `/system/shutdown` instead (see `golden_image.rs`).
    pub async fn kill(&mut self) -> anyhow::Result<()> {
        self.child.kill().await?;
        Ok(())
    }
}

impl Drop for RunningNode {
    fn drop(&mut self) {
        // kill_on_drop(true) (set at spawn) handles terminating the process;
        // unlinking the overlay here is safe even if it's still exiting -
        // on Linux the inode stays live under the process's open fd until
        // that fd closes, this just removes it from the directory listing.
        let _ = std::fs::remove_file(&self.overlay_path);
    }
}

async fn connect_api_with_retry(
    api_port: u16,
    password: &str,
    timeout: Duration,
) -> anyhow::Result<MikrotikDevice<SimpleResult>> {
    let deadline = Instant::now() + timeout;
    loop {
        match MikrotikDevice::connect(
            ("127.0.0.1", api_port),
            b"admin",
            Some(encode_latin1_lossy(password)),
        )
        .await
        {
            Ok(device) => return Ok(device),
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// The MAC of NIC `slot` (1 = `ether2`) of node number `node`.
pub fn mac_of(node: u8, slot: usize) -> String {
    format!("52:54:00:{node:02X}:00:{slot:02X}")
}

static NEXT_NODE: AtomicU8 = AtomicU8::new(1);

/// Boots one disposable node. `wiring[i]` describes NIC slot `i + 1`
/// (slot 0 is always the management NIC); `wiring.len()` should equal
/// `board.build_ethernet_ports().len() - 1` for the node's board so every
/// physical port the generated config might reference has a matching slot.
pub async fn boot_node(
    golden: &GoldenImage,
    work_dir: &Path,
    name: &str,
    wiring: &[PortWiring],
    password: &str,
) -> anyhow::Result<RunningNode> {
    let node = start_node(golden, work_dir, name, wiring).await?;
    node.connect_api(password).await?;
    Ok(node)
}

/// Boots one disposable node without waiting for it to answer - for a
/// factory image, whose `admin` has no password yet (RouterOS asks for one
/// at the first console login).
pub async fn start_node(
    golden: &GoldenImage,
    work_dir: &Path,
    name: &str,
    wiring: &[PortWiring],
) -> anyhow::Result<RunningNode> {
    let index = NEXT_NODE.fetch_add(1, Ordering::Relaxed);
    tokio::fs::create_dir_all(work_dir).await?;
    let overlay_path = work_dir.join(format!("{name}.qcow2"));

    let status = qemu_img_command()
        .args(["create", "-f", "qcow2", "-b"])
        .arg(&golden.path)
        .args(["-F", "raw"])
        .arg(&overlay_path)
        .status()
        .await?;
    anyhow::ensure!(
        status.success(),
        "qemu-img create for node {name:?} failed with {status}"
    );

    let api_port = free_tcp_port()?;
    let socks_port = free_tcp_port()?;
    let ssh_port = free_tcp_port()?;
    let mut cmd = qemu_command();
    cmd.args(["-machine", "pc,accel=tcg", "-cpu", "max", "-m", "512", "-smp", "2"])
        .arg("-drive")
        .arg(format!("file={},format=qcow2", overlay_path.display()))
        .arg("-netdev")
        .arg(format!(
            "user,id=net0,hostfwd=tcp:127.0.0.1:{api_port}-:8728,hostfwd=tcp:127.0.0.1:{socks_port}-:1080,hostfwd=tcp:127.0.0.1:{ssh_port}-:22"
        ))
        // QEMU's default MAC, the one the golden image was set up with: RouterOS takes a new
        // MAC for a new interface, which would leave the image's DHCP client without its port
        .args(["-device", "virtio-net-pci,netdev=net0"])
        .args(["-display", "none", "-monitor", "none"]);

    for (i, slot) in wiring.iter().enumerate() {
        let id = format!("net{}", i + 1);
        match slot {
            PortWiring::Idle => {
                cmd.arg("-netdev").arg(format!("user,id={id}"));
            }
            PortWiring::Listen(port) => {
                cmd.arg("-netdev")
                    .arg(format!("socket,id={id},listen=127.0.0.1:{port}"));
            }
            PortWiring::Connect(port) => {
                cmd.arg("-netdev")
                    .arg(format!("socket,id={id},connect=127.0.0.1:{port}"));
            }
        }
        cmd.args([
            "-device",
            &format!("virtio-net-pci,netdev={id},mac={}", mac_of(index, i + 1)),
        ]);
    }

    let child = cmd.kill_on_drop(true).spawn()?;

    let node = RunningNode {
        name: name.to_string(),
        api_port,
        socks_port,
        ssh_port,
        index,
        child,
        overlay_path,
    };
    Ok(node)
}
