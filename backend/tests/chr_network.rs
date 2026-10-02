//! Multi-CHR network test harness: boots several MikroTik CHR instances
//! under QEMU, wires them up per a `TopologyBuilder` scenario, and proves
//! real distributed network behavior (OSPF convergence across hops) rather
//! than just single-device command acceptance.
//!
//! Gated behind `--ignored` since it needs `nix`/QEMU and real wall-clock
//! time to boot VMs - this must never slow down or break a normal
//! `cargo test` run. Every test here starts by calling `qemu::nix_unavailable()`
//! and skipping with a clear message if it's missing.

mod chr_network {
    pub mod golden_image;
    pub mod qemu;
    pub mod serial;
}

use chr_network::golden_image::ensure_golden_image;
use chr_network::qemu::nix_unavailable;
use std::path::PathBuf;

const TEST_PASSWORD: &str = "NetboxTest123!";

fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CHR_TEST_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend/ has a parent")
        .join(".chr-test-cache")
}

#[tokio::test]
#[ignore]
async fn build_golden_image() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = ensure_golden_image(&cache_dir(), TEST_PASSWORD)
        .await
        .expect("golden image build failed");
    assert!(golden.path.is_file());
}
