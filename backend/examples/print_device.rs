//! Prints what some `print` commands answer on one device, read-only: the rows as they come.
//! Uses `config.yaml` from the current directory and its `management_proxy`, if there is one.
//!
//!     cargo run -p backend --example print_device -- <address> <credentials> /file/print ...
//!
//! With `--via <neighbor address> <neighbor credentials>` first, the device is reached through
//! an SSH tunnel to that neighbor, like a device after its initial setup.
//!
//! Anything but a `.../print` is refused, so this can't change a device. The prints are raw
//! commands, the one place in the provisioner where that is the point: it shows what a menu
//! answers, also one mikrotik-model has no type for yet.
use backend::{
    config::CONFIG,
    device::{
        DeviceStream,
        access::{Session, observe},
        proxied_stream,
        terminal::Tunnel,
    },
};
use mikrotik_api::{
    command::Observe,
    prelude::{Connection, Generation, RawCommand},
};
use std::net::IpAddr;

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
        let ssh = match proxied_stream(neighbor, 22).await? {
            Some(stream) => DeviceStream::Proxied(stream),
            None => DeviceStream::Direct(tokio::net::TcpStream::connect((neighbor, 22)).await?),
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
    let generation = Generation::default();
    let connection = if let Some(tunnel) = &via {
        let stream = tunnel.open(address, 8728).await?;
        Connection::connect_with_stream(stream, user, password, generation).await?
    } else {
        match proxied_stream(address, 8728).await? {
            Some(stream) => {
                Connection::connect_with_stream(stream, user, password, generation).await?
            }
            None => Connection::connect((address, 8728), user, password, generation).await?,
        }
    };
    let device = Session::new(connection);
    for command in args {
        anyhow::ensure!(
            command.ends_with("/print"),
            "only print commands: {command}"
        );
        println!("{command}");
        let print = RawCommand::<Observe>::new(command.into_bytes());
        match observe(&device, &print).await {
            Ok(rows) => rows.iter().for_each(|row| println!("  {row:?}")),
            Err(error) => println!("  {error}"),
        }
    }
    Ok(())
}
