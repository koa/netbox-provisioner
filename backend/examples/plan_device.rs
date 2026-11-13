//! Prints the script the provisioner would apply to one device, without applying anything:
//! fetches the topology from NetBox, reads the device's current config (read-only) and diffs it
//! against the generated target. Uses `config.yaml` from the current directory.
//!
//!     cargo run -p backend --example plan_device -- <device name> [<address>]
//!
//! `<address>` overrides the device's primary IP, e.g. for a router reached over its uplink.
//! Goes through the `management_proxy` of the config, if there is one.
use backend::{
    config::CONFIG,
    device::{access::Session, proxied_stream, ros::cfg::DeviceCfg},
    topology::fetch::build_topology,
};
use mikrotik_api::prelude::{Connection, Generation};
use std::{net::IpAddr, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().filter_or("LOG_LEVEL", "warn"));
    let mut args = std::env::args().skip(1);
    let name = args.next().expect("device name");
    let address: Option<IpAddr> = args.next().map(|a| a.parse()).transpose()?;

    let topology = Arc::new(build_topology().await?);
    let device = topology
        .list_devices()
        .into_iter()
        .find(|d| d.name() == name)
        .ok_or_else(|| anyhow::anyhow!("no device {name} in NetBox"))?;
    let address = address
        .or(device.primary_ip())
        .ok_or_else(|| anyhow::anyhow!("{name} has no primary IP"))?;
    let credentials = device
        .credentials()
        .and_then(|c| CONFIG.mikrotik_credentials.get(c))
        .ok_or_else(|| anyhow::anyhow!("{name} has no credentials"))?;
    let (user, password) = (
        credentials.user().as_bytes(),
        credentials.password().map(str::as_bytes),
    );
    let generation = Generation::default();
    let connection = match proxied_stream(address, 8728).await? {
        Some(stream) => Connection::connect_with_stream(stream, user, password, generation).await?,
        None => Connection::connect((address, 8728), user, password, generation).await?,
    };
    let client = Session::new(connection);
    let mut cfg = DeviceCfg::fetch(&client, &device).await?;
    cfg.generate_for(&device)?;
    println!("{}", cfg.script()?);
    Ok(())
}
