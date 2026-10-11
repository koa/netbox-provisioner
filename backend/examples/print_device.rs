//! Prints what some `print` commands answer on one device, read-only: the rows as they come.
//! Uses `config.yaml` from the current directory and its `management_proxy`, if there is one.
//!
//!     cargo run -p backend --example print_device -- <address> <credentials> /file/print ...
//!
//! With `--via <neighbor address> <neighbor credentials>` first, the device is reached through
//! an SSH tunnel to that neighbor, like a device after its initial setup.
//!
//! Anything but a `.../print` is refused, so this can't change a device.
use backend::{
    config::CONFIG,
    device::{Io, proxied_stream, terminal::Tunnel},
};
use mikrotik_api::simple::SimpleResult;
use mikrotik_api::prelude::MikrotikDevice;
use std::net::IpAddr;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    let via = if args.peek().map(String::as_str) == Some("--via") {
        args.next();
        let neighbor: IpAddr = args.next().expect("neighbor address").parse()?;
        let name = args.next().expect("neighbor credentials name");
        let credentials = CONFIG
            .mikrotik_credentials
            .get(name.as_str())
            .ok_or_else(|| anyhow::anyhow!("no credentials {name}"))?;
        let ssh: Box<dyn Io> = match proxied_stream(neighbor, 22).await? {
            Some(stream) => Box::new(stream),
            None => Box::new(tokio::net::TcpStream::connect((neighbor, 22)).await?),
        };
        Some(Tunnel::connect_with_stream(ssh, credentials.user(), credentials.password()).await?)
    } else {
        None
    };
    let address: IpAddr = args.next().expect("address").parse()?;
    let name = args.next().expect("credentials name");
    let credentials = CONFIG
        .mikrotik_credentials
        .get(name.as_str())
        .ok_or_else(|| anyhow::anyhow!("no credentials {name}"))?;
    let (user, password) = (
        credentials.user().as_bytes(),
        credentials.password().map(str::as_bytes),
    );
    let device: MikrotikDevice<SimpleResult> = if let Some(tunnel) = &via {
        MikrotikDevice::connect_with_stream(tunnel.open(address, 8728).await?, user, password)
            .await?
    } else {
        match proxied_stream(address, 8728).await? {
            Some(stream) => MikrotikDevice::connect_with_stream(stream, user, password).await?,
            None => MikrotikDevice::connect((address, 8728), user, password).await?,
        }
    };
    for command in args {
        anyhow::ensure!(command.ends_with("/print"), "only print commands: {command}");
        println!("{command}");
        let mut rows = device
            .send_simple_command(command.as_bytes(), ())
            .await;
        while let Some(row) = rows.next().await {
            println!("  {row:?}");
        }
    }
    Ok(())
}
