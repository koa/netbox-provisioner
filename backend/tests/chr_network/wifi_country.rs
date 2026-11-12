//! Scope: CHR test of how RouterOS spells a country in the configurations of both CAPsMANs and
//! how mikrotik-model reads it back; nothing else.
//!
//! The wifi CAPsMAN (`/interface/wifi/configuration`) is in RouterOS's main package since 7.13,
//! so a CHR without any radio has it; it takes and writes the country capitalized
//! (`Switzerland`, `WifiCountry`). The legacy CAPsMAN (`/caps-man`) comes with the `wireless`
//! package, which the CHR fetches from MikroTik; it takes and writes it in lowercase
//! (`switzerland`, `WifiCountryLower`), in its configuration, its interfaces and their actual
//! configuration. An interface no CAP is bound to lacks the fields of a bound one (its current
//! state and rates, `disabled`, `l2mtu`, `dynamic`) and reads all the same.
use crate::chr_network::golden_image::ensure_golden_image;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::{RunningNode, boot_node};
use encoding_rs::mem::encode_latin1_lossy;
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::model::{
    CapsManActualInterfaceConfiguration, CapsManConfiguration, CapsManInterface,
    InterfaceWifiConfiguration, WifiCountry, WifiCountryLower,
};
use mikrotik_model::resource::collect_resource;
use std::collections::HashMap;
use std::time::Duration;
use tokio_stream::StreamExt;

type Row = HashMap<Box<str>, Option<Box<str>>>;

/// The rows `command` with `attributes` answers, or the message of its trap.
async fn answer(
    device: &MikrotikDevice<SimpleResult>,
    command: &str,
    attributes: &[(&str, &str)],
) -> Result<Box<[Row]>, Box<str>> {
    let mut stream = device
        .send_command(
            command.as_bytes(),
            |cmd| {
                attributes.iter().fold(cmd, |cmd, (key, value)| {
                    cmd.attribute(key.as_bytes(), value.as_bytes())
                })
            },
            (),
        )
        .await;
    let mut rows = Vec::new();
    let mut trap = None;
    while let Some(result) = stream.next().await {
        match result {
            SimpleResult::Sentence(row) => rows.push(row),
            SimpleResult::Trap { message, .. } => {
                trap.get_or_insert(Box::from(message.to_string()));
            }
            SimpleResult::Error(error) => panic!("{command}: {error:?}"),
        }
    }
    match trap {
        None => Ok(rows.into()),
        Some(message) => Err(message),
    }
}

fn field<'r>(row: &'r Row, key: &str) -> Option<&'r str> {
    row.get(key).and_then(Option::as_deref)
}

/// `field_name` of every row of `menu`'s print.
async fn printed(
    device: &MikrotikDevice<SimpleResult>,
    menu: &str,
    field_name: &str,
) -> Vec<Box<str>> {
    answer(device, &format!("{menu}/print"), &[])
        .await
        .unwrap()
        .iter()
        .filter_map(|row| field(row, field_name).map(Box::from))
        .collect()
}

/// A configuration of `menu` named `name` with `country`.
async fn add_configuration(
    device: &MikrotikDevice<SimpleResult>,
    menu: &str,
    name: &str,
    country: &str,
) -> Result<Box<[Row]>, Box<str>> {
    answer(
        device,
        &format!("{menu}/add"),
        &[("name", name), ("ssid", name), ("country", country)],
    )
    .await
}

async fn typed(node: &RunningNode) -> mikrotik_model::MikrotikDevice {
    mikrotik_model::MikrotikDevice::connect(
        ("127.0.0.1", node.api_port),
        b"admin",
        Some(encode_latin1_lossy(crate::TEST_PASSWORD)),
    )
    .await
    .unwrap()
}

async fn boot(name: &str) -> RunningNode {
    let golden = ensure_golden_image(&crate::cache_dir(), crate::TEST_PASSWORD)
        .await
        .unwrap();
    let work_dir = std::env::temp_dir().join(format!("chr-network-{name}"));
    boot_node(&golden, &work_dir, name, &[], crate::TEST_PASSWORD)
        .await
        .unwrap()
}

/// RouterOS's refusal of a country it doesn't know.
const NO_SUCH_COUNTRY: &str = "input does not match any value of country";

#[tokio::test]
#[ignore]
async fn the_wifi_capsman_spells_the_country_capitalized() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let node = boot("wifi-country").await;
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    let menu = "/interface/wifi/configuration";
    add_configuration(&device, menu, "upper", "Switzerland")
        .await
        .unwrap();
    let lower = add_configuration(&device, menu, "lower", "switzerland").await;
    assert_eq!(lower.err().as_deref(), Some(NO_SUCH_COUNTRY));
    assert_eq!(
        printed(&device, menu, "country").await,
        [Box::from("Switzerland")]
    );
    let read = collect_resource::<InterfaceWifiConfiguration>(&typed(&node).await)
        .await
        .unwrap();
    let countries = read.iter().map(|c| c.cfg.country).collect::<Vec<_>>();
    assert_eq!(countries, [Some(WifiCountry::Switzerland)]);
}

#[tokio::test]
#[ignore]
async fn the_legacy_capsman_spells_the_country_in_lowercase() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let node = boot("capsman-country").await;
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    let version = answer(&device, "/system/package/print", &[])
        .await
        .unwrap()
        .iter()
        .find(|row| field(row, "name") == Some("routeros"))
        .and_then(|row| field(row, "version").map(Box::<str>::from))
        .unwrap();
    // x86 packages carry no architecture
    let url = format!("https://download.mikrotik.com/routeros/{version}/wireless-{version}.npk");
    answer(&device, "/tool/fetch", &[("url", &url)])
        .await
        .unwrap();
    // the connection goes down with the reboot
    let _ = answer(&device, "/system/reboot", &[]).await;
    drop(device);
    tokio::time::sleep(Duration::from_secs(20)).await;
    let mut device = None;
    for _ in 0..10 {
        if let Ok(connected) = node.connect_api(crate::TEST_PASSWORD).await {
            device = Some(connected);
            break;
        }
    }
    let device = device.expect("back after the reboot");
    let menu = "/caps-man/configuration";
    add_configuration(&device, menu, "lower", "switzerland")
        .await
        .unwrap();
    let upper = add_configuration(&device, menu, "upper", "Switzerland").await;
    assert_eq!(upper.err().as_deref(), Some(NO_SUCH_COUNTRY));
    // no CAP is bound to it
    answer(
        &device,
        "/caps-man/interface/add",
        &[
            ("name", "cap-test"),
            ("radio-mac", "02:00:00:00:00:10"),
            ("configuration", "lower"),
            ("configuration.country", "switzerland"),
        ],
    )
    .await
    .unwrap();
    let lowercase = [Box::from("switzerland")];
    assert_eq!(printed(&device, menu, "country").await, lowercase);
    for menu in [
        "/caps-man/interface",
        "/caps-man/actual-interface-configuration",
    ] {
        assert_eq!(
            printed(&device, menu, "configuration.country").await,
            lowercase,
            "{menu}"
        );
    }
    let typed = typed(&node).await;
    let switzerland = [Some(WifiCountryLower::Switzerland)];
    let configurations = collect_resource::<CapsManConfiguration>(&typed)
        .await
        .unwrap();
    let countries = configurations
        .iter()
        .map(|c| c.cfg.country)
        .collect::<Vec<_>>();
    assert_eq!(countries, switzerland);
    let interfaces = collect_resource::<CapsManInterface>(&typed).await.unwrap();
    let countries = interfaces
        .iter()
        .map(|i| i.cfg.configuration_country)
        .collect::<Vec<_>>();
    assert_eq!(countries, switzerland);
    let actual = collect_resource::<CapsManActualInterfaceConfiguration>(&typed)
        .await
        .unwrap();
    let countries = actual
        .iter()
        .map(|a| a.cfg.configuration_country)
        .collect::<Vec<_>>();
    assert_eq!(countries, switzerland);
}
