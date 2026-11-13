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
use crate::chr_network::node::add;
use crate::chr_network::qemu::nix_unavailable;
use crate::chr_network::vm::{RunningNode, boot_node};
use backend::Error;
use backend::device::access::{Session, act};
use mikrotik_api::command::Write;
use mikrotik_api::prelude::RawCommand;
use mikrotik_model::MacAddress;
use mikrotik_model::action::{FetchFile, Reboot};
use mikrotik_model::model::{
    CapsManActualInterfaceConfiguration, CapsManConfiguration, CapsManConfigurationCfg,
    CapsManInterface, CapsManInterfaceCfg, InterfaceWifiConfiguration,
    InterfaceWifiConfigurationCfg, SystemPackageState, WifiCountry, WifiCountryLower,
};
use mikrotik_model::resource::collect_resource;
use std::time::Duration;

/// RouterOS's answer to adding a configuration to `menu` with `country` spelled as given.
///
/// Raw on purpose: the typed configurations take only the spelling their menu writes
/// (`WifiCountry`, `WifiCountryLower`), so the other one, which this test checks RouterOS
/// refuses, can't be written typed.
async fn add_spelled(device: &Session, menu: &str, name: &str, country: &str) -> Result<(), Error> {
    let add = RawCommand::<Write>::new(format!("{menu}/add").into_bytes())
        .attribute(b"name".as_slice(), name.as_bytes())
        .attribute(b"ssid".as_slice(), name.as_bytes())
        .attribute(b"country".as_slice(), country.as_bytes());
    act(device, &add).await.map(|_| ())
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

fn refused_as_unknown(result: &Result<(), Error>) -> bool {
    matches!(result, Err(Error::Refused(message)) if message.ends_with(NO_SUCH_COUNTRY))
}

#[tokio::test]
#[ignore]
async fn the_wifi_capsman_spells_the_country_capitalized() {
    if nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let node = boot("wifi-country").await;
    let device = node.connect_api(crate::TEST_PASSWORD).await.unwrap();
    let mut upper = InterfaceWifiConfigurationCfg::new(false, "upper".into());
    upper.ssid = Some("upper".into());
    upper.country = Some(WifiCountry::Switzerland);
    add(&device, &upper).await.unwrap();
    let lower = add_spelled(
        &device,
        "/interface/wifi/configuration",
        "lower",
        "switzerland",
    )
    .await;
    assert!(refused_as_unknown(&lower), "{lower:?}");
    // the typed read takes only the spelling `WifiCountry` has
    let read = collect_resource::<InterfaceWifiConfiguration>(&device)
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
    let version = collect_resource::<SystemPackageState>(&device)
        .await
        .unwrap()
        .into_iter()
        .find(|package| *package.name.0 == *b"routeros")
        .map(|package| package.version.to_string())
        .unwrap();
    // x86 packages carry no architecture
    let url = format!("https://download.mikrotik.com/routeros/{version}/wireless-{version}.npk");
    act(&device, &FetchFile { url: &url }).await.unwrap();
    // the connection goes down with the reboot
    let _ = act(&device, &Reboot).await;
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
    let mut lower = CapsManConfigurationCfg::new("lower".into());
    lower.ssid = Some("lower".into());
    lower.country = Some(WifiCountryLower::Switzerland);
    add(&device, &lower).await.unwrap();
    let upper = add_spelled(&device, "/caps-man/configuration", "upper", "Switzerland").await;
    assert!(refused_as_unknown(&upper), "{upper:?}");
    // no CAP is bound to it
    let mut interface = CapsManInterfaceCfg::new(
        "02:00:00:00:00:11".parse::<MacAddress>().unwrap(),
        "cap-test".into(),
        "02:00:00:00:00:10".parse::<MacAddress>().unwrap(),
    );
    interface.configuration = Some("lower".into());
    interface.configuration_country = Some(WifiCountryLower::Switzerland);
    add(&device, &interface).await.unwrap();
    // the typed reads take only the spelling `WifiCountryLower` has
    let typed = device;
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
