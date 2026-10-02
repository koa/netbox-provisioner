//! Drives a freshly booted CHR's first-boot dialog (license paging + initial
//! password set) over the QEMU `-serial tcp:...` console. Ported from a
//! manually-driven session verified live against CHR 7.24.5; see the
//! comments inline for the two non-obvious failure modes that cost real time
//! to track down the first time around.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;

async fn read_for(stream: &mut TcpStream, window: Duration) -> Vec<u8> {
    let mut buf = Vec::new();
    let deadline = Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut chunk = [0u8; 4096];
        match tokio::time::timeout(remaining, stream.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    buf
}

/// Reads until `needle` appears in the accumulated output or `timeout`
/// elapses. Needed for the login prompt specifically: sending "admin\r\n"
/// blind (before the prompt is confirmed to have appeared) races the
/// guest's getty startup - characters sent too early are dropped rather
/// than buffered, silently bouncing back to a fresh "Login:" prompt
/// instead of logging in (observed live: without this wait, the whole
/// first-boot dance just spins on a redisplayed "CHR Login:" banner).
async fn wait_for(stream: &mut TcpStream, needle: &str, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut acc = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(
            !remaining.is_zero(),
            "timed out waiting for {needle:?}, accumulated output so far: {acc:?}"
        );
        let mut chunk = [0u8; 4096];
        match tokio::time::timeout(remaining.min(Duration::from_secs(1)), stream.read(&mut chunk))
            .await
        {
            Ok(Ok(0)) => anyhow::bail!("connection closed while waiting for {needle:?}"),
            Ok(Ok(n)) => {
                acc.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if acc.contains(needle) {
                    return Ok(());
                }
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {} // no data within this slice, keep waiting until the overall deadline
        }
    }
}

async fn connect_with_retry(port: u16, overall_timeout: Duration) -> anyhow::Result<TcpStream> {
    let deadline = Instant::now() + overall_timeout;
    loop {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Logs into a fresh (never-booted) CHR as `admin` with a blank password and
/// sets `password` as the new one, via the serial console at
/// `127.0.0.1:{serial_port}`. Only valid on an image that has never been
/// through this dialog before - running it twice hits "New password is the
/// same as old one" instead of the expected prompts.
pub async fn run_first_boot_setup(serial_port: u16, password: &str) -> anyhow::Result<()> {
    let mut stream = connect_with_retry(serial_port, Duration::from_secs(30)).await?;

    // Wait for the actual "Login:" prompt rather than sending "admin\r\n"
    // blind: characters sent before the guest's getty starts reading the
    // tty are dropped, not buffered, which otherwise bounces straight back
    // to a freshly redisplayed "CHR Login:" banner instead of logging in.
    wait_for(&mut stream, "Login:", Duration::from_secs(60)).await?;
    stream.write_all(b"admin\r\n").await?;
    wait_for(&mut stream, "Password:", Duration::from_secs(10)).await?;
    stream.write_all(b"\r\n").await?; // blank password - factory default

    // What follows is terminal-capability negotiation junk, then the
    // MikroTik splash + "Do you want to see the software license? [Y/n]:",
    // then on the order of 60 "-- press Enter (q to abort)" pages of
    // GPL/BSL license text (exact count isn't pinned by version, so this
    // caps generously rather than hardcoding a page count). A bare `\r\n`
    // default-accepts the Y/n prompt and pages through every later screen.
    const MAX_PAGES: usize = 200;
    let mut reached_password_prompt = false;
    for _ in 0..MAX_PAGES {
        let out = read_for(&mut stream, Duration::from_millis(600)).await;
        let text = String::from_utf8_lossy(&out);
        anyhow::ensure!(
            !text.contains("Login:"),
            "bounced back to the login prompt while paging through the license - \
             the login itself didn't take"
        );
        if text.contains("new password>") {
            reached_password_prompt = true;
            break;
        }
        stream.write_all(b"\r\n").await?;
    }
    anyhow::ensure!(
        reached_password_prompt,
        "never reached the 'new password>' prompt within {MAX_PAGES} pages - \
         the license text layout may have changed for this CHR version"
    );

    // Critical: the confirmation must be a bare `\r`, not `\r\n`. The
    // trailing `\n` of `\r\n` registers as a second, blank Enter-press that
    // gets consumed as a stray empty submission at this specific prompt,
    // reliably producing "New passwords do not match" even when sending the
    // identical password twice in a row. This cost significant
    // back-and-forth to diagnose by hand the first time; don't regress it.
    stream.write_all(password.as_bytes()).await?;
    stream.write_all(b"\r").await?;
    let _ = read_for(&mut stream, Duration::from_secs(2)).await; // "repeat new password>"
    stream.write_all(password.as_bytes()).await?;
    stream.write_all(b"\r").await?;
    let out = read_for(&mut stream, Duration::from_secs(3)).await;
    let text = String::from_utf8_lossy(&out);
    anyhow::ensure!(
        text.contains("Password changed"),
        "password set did not confirm as expected, got: {text:?}"
    );
    Ok(())
}
