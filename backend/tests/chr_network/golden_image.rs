//! Builds and caches a "golden" CHR disk image: license accepted, admin
//! password set, otherwise factory-default. Every test run boots a
//! disposable `qcow2` overlay on top of this image (see `vm.rs`) rather than
//! touching it directly, so a hard `kill -9` of a test VM is always safe -
//! only the first-boot dance that produces this file itself needs the
//! careful clean-shutdown handling below.

use crate::chr_network::qemu::{free_tcp_port, qemu_command};
use crate::chr_network::serial::run_first_boot_setup;
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_stream::StreamExt;

const CHR_VERSION: &str = "7.24.5";
const CHR_IMAGE_URL: &str = "https://download.mikrotik.com/routeros/7.24.5/chr-7.24.5.img.zip";
/// Pinned from the zip at CHR_IMAGE_URL as downloaded and verified while
/// writing this module - MikroTik does not publish an official checksum for
/// CHR releases. Bump together with CHR_VERSION/CHR_IMAGE_URL, and re-verify
/// the whole first-boot dance by hand, if this ever needs to change.
const CHR_IMAGE_SHA256: &str = "16f07222a3213352c9c4fbea4f4dec6a8ffb78fdc0fa5e398c23103ed357c6eb";

pub struct GoldenImage {
    pub path: PathBuf,
}

/// Returns the cached golden image at `cache_dir`, building it first if
/// missing. Safe to call concurrently from multiple test processes sharing
/// the same `cache_dir` - a file lock serializes the (one-time) build.
pub async fn ensure_golden_image(cache_dir: &Path, password: &str) -> anyhow::Result<GoldenImage> {
    let golden_path = cache_dir.join(format!("chr-{CHR_VERSION}-golden.img"));
    if golden_path.is_file() {
        return Ok(GoldenImage { path: golden_path });
    }

    tokio::fs::create_dir_all(cache_dir).await?;
    let _lock = BuildLock::acquire(cache_dir).await?;
    // Re-check: another process may have finished the build while we were
    // waiting for the lock.
    if golden_path.is_file() {
        return Ok(GoldenImage { path: golden_path });
    }

    let zip_path = cache_dir.join(format!("chr-{CHR_VERSION}.img.zip"));
    download_and_verify(&zip_path).await?;

    let building_path = cache_dir.join(format!("chr-{CHR_VERSION}-building.img"));
    extract_image(&zip_path, &building_path).await?;

    boot_set_password_and_shutdown(&building_path, password).await?;

    tokio::fs::rename(&building_path, &golden_path).await?;
    Ok(GoldenImage { path: golden_path })
}

async fn download_and_verify(zip_path: &Path) -> anyhow::Result<()> {
    if zip_path.is_file() && sha256_file(zip_path).await? == CHR_IMAGE_SHA256 {
        return Ok(());
    }
    let bytes = reqwest::get(CHR_IMAGE_URL)
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    anyhow::ensure!(
        digest == CHR_IMAGE_SHA256,
        "downloaded CHR image checksum mismatch: expected {CHR_IMAGE_SHA256}, got {digest} - \
         MikroTik may have replaced the {CHR_VERSION} artifact; re-verify the whole first-boot \
         dance by hand before updating this pin"
    );
    tokio::fs::write(zip_path, &bytes).await?;
    Ok(())
}

async fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = tokio::fs::read(path).await?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

async fn extract_image(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let extract_dir = dest.with_extension("extract");
    tokio::fs::create_dir_all(&extract_dir).await?;
    let status = tokio::process::Command::new("nix")
        .args([
            "--extra-experimental-features",
            "nix-command flakes",
            "shell",
            "nixpkgs#unzip",
            "-c",
            "unzip",
            "-o",
        ])
        .arg(zip_path)
        .arg("-d")
        .arg(&extract_dir)
        .status()
        .await?;
    anyhow::ensure!(status.success(), "unzip failed with {status}");
    let extracted = extract_dir.join(format!("chr-{CHR_VERSION}.img"));
    tokio::fs::rename(&extracted, dest).await?;
    tokio::fs::remove_dir_all(&extract_dir).await?;
    Ok(())
}

async fn boot_set_password_and_shutdown(image_path: &Path, password: &str) -> anyhow::Result<()> {
    let serial_port = free_tcp_port()?;
    let api_port = free_tcp_port()?;

    let mut child = qemu_command()
        .args(["-machine", "pc,accel=tcg", "-cpu", "max", "-m", "512", "-smp", "2"])
        .arg("-drive")
        .arg(format!("file={},format=raw,if=virtio", image_path.display()))
        .arg("-netdev")
        .arg(format!("user,id=net0,hostfwd=tcp:127.0.0.1:{api_port}-:8728"))
        .args(["-device", "virtio-net-pci,netdev=net0"])
        .arg("-serial")
        .arg(format!("tcp:127.0.0.1:{serial_port},server,nowait"))
        .args(["-display", "none", "-monitor", "none"])
        .kill_on_drop(true)
        .spawn()?;

    if let Err(e) = run_first_boot_setup(serial_port, password).await {
        // Nothing usable was ever produced on this path - a hard kill here
        // doesn't risk the golden image itself (it isn't renamed into place
        // until after a clean shutdown below).
        let _ = child.kill().await;
        return Err(e);
    }

    // Confirm the new credential actually works over the API, then reuse
    // that same connection to shut down cleanly. A hard kill at this point
    // would risk corrupting the disk (the exact failure mode that destroyed
    // one hand-built CHR image earlier in this project's testing).
    let device: MikrotikDevice<SimpleResult> = MikrotikDevice::connect(
        ("127.0.0.1", api_port),
        b"admin",
        Some(encode_latin1_lossy(password)),
    )
    .await?;
    let _ = device
        .send_simple_command(b"/system/shutdown".as_slice(), ())
        .await
        .next()
        .await;

    tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .map_err(|_| anyhow::anyhow!("CHR did not shut down within 30s after /system/shutdown"))??;
    Ok(())
}

/// Exclusive, cross-process file lock so concurrent test runs sharing a
/// cache dir don't race on building the golden image.
struct BuildLock {
    path: PathBuf,
}

impl BuildLock {
    async fn acquire(cache_dir: &Path) -> anyhow::Result<Self> {
        let path = cache_dir.join(".building.lock");
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A crashed build must not wedge every future run
                    // forever - steal a sufficiently stale lock.
                    let age = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|m| m.elapsed().unwrap_or_default());
                    if age.is_ok_and(|age| age > Duration::from_secs(600)) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for BuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
