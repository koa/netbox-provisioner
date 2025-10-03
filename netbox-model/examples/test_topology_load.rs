use env_logger::{Env, TimestampPrecision};
use netbox_model::graphql::fetch_topology;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .parse_env(Env::default().filter_or("LOG_LEVEL", "info"))
        .format_timestamp(Some(TimestampPrecision::Millis))
        .init();
    let topology = fetch_topology().await?;
    topology.devices().for_each(|device| {
        println!("{:?} {:?}", device.id, device.name);
    });
    Ok(())
}
