use backend::topology::fetch::build_topology;
use env_logger::{Env, TimestampPrecision};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .parse_env(Env::default().filter_or("LOG_LEVEL", "info"))
        .format_timestamp(Some(TimestampPrecision::Millis))
        .init();
    let topology = Arc::new(build_topology().await?);
    for device in topology.list_devices() {
        if !device.has_routeros() {
            continue;
        }
        let name = device.name();
        let ip = device
            .primary_ip()
            .map(|ip| format!(" ({})", ip))
            .unwrap_or_default();
        println!("Device: {name}{ip}");
        for interface in device.interfaces() {
            println!(
                "  - {}: {}",
                interface.name(),
                interface
                    .external_port()
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}
