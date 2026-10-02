//! Shared QEMU plumbing for the multi-CHR test harness.

/// The `nix` package source is the only one available in CI/sandboxes that
/// have no root/apt and no KVM - `accel=tcg` software emulation only.
/// Pinned to a specific revision (rather than floating `nixpkgs`) so a
/// future nixpkgs update can't silently change QEMU's PCI enumeration order
/// or serial-console behavior underneath this harness; this exact revision
/// was verified to produce a working CHR boot + first-boot dance.
const NIXPKGS_REV: &str = "b6c8664de9b6cc07fe5666a29f91884ba81197c4";

/// Builds a `tokio::process::Command` that runs `qemu-system-x86_64` via the
/// pinned nixpkgs revision. Append qemu's own arguments with `.arg`/`.args`.
pub fn qemu_command() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("nix");
    cmd.args([
        "--extra-experimental-features",
        "nix-command flakes",
        "shell",
        &format!("github:NixOS/nixpkgs/{NIXPKGS_REV}#qemu"),
        "-c",
        "qemu-system-x86_64",
    ]);
    cmd
}

/// True if `nix` itself isn't available, in which case every test in this
/// module should skip with a clear message instead of failing noisily -
/// this harness must never break a normal `cargo test` run on a machine
/// without it.
pub fn nix_unavailable() -> bool {
    std::process::Command::new("nix")
        .arg("--version")
        .output()
        .is_err()
}

/// Binds a throwaway listener to get an OS-assigned free port, then drops it
/// so QEMU can bind the same port right after. Racy in theory (another
/// process could grab it first) but the standard, good-enough pattern for
/// test harnesses - a transient bind failure just means a retry of the test.
pub fn free_tcp_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.local_addr().map(|addr| addr.port())
}
