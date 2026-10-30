//! `/ip/service` (`setup_services`): telnet and ftp off, www on, with `api_networks` the plain
//! API and www only from those; everything else as the device has it.
use crate::device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, test::empty_current};
use ipnet::IpNet;
use mikrotik_model::{
    generator::Generator,
    model::{IpServiceByName, IpServiceCfg},
};

fn service(name: &str, disabled: bool, available_from: Option<&str>, address: Option<&str>) -> IpServiceByName {
    IpServiceByName {
        name: name.into(),
        data: IpServiceCfg {
            disabled,
            available_from: available_from.map(Into::into),
            address: address.map(Into::into),
        },
    }
}

/// The script lines of `/ip/service`.
fn service_script(networks: Option<&[IpNet]>, services: Vec<IpServiceByName>) -> String {
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    let mut current: BaseDeviceDataCurrent = empty_current(&target);
    current.ip_service = services.into();
    target.setup_services(&current.ip_service, networks);
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in target.generate_mutations(&current).unwrap().iter() {
        if mutation.resource == b"ip/service" {
            generator.append_mutation(mutation).unwrap();
        }
    }
    script
}

/// RouterOS's services of 7.24.5, all on and open.
fn factory() -> Vec<IpServiceByName> {
    ["ftp", "ssh", "telnet", "www", "www-ssl", "winbox", "api", "api-ssl"]
        .into_iter()
        .map(|name| service(name, name == "www-ssl", None, None))
        .collect()
}

#[test]
fn telnet_and_ftp_go_off() {
    let script = service_script(None, factory());
    assert_eq!(
        script,
        "/ip/service\n\
         set [ find name=ftp dynamic=no] disabled=yes \n\
         set [ find name=telnet dynamic=no] disabled=yes \n"
    );
}

/// WebFig is what's left in the field where Winbox isn't at hand: www goes on again where it
/// was off (as an apply of A2's first version left it), from the networks of the API.
#[test]
fn webfig_answers_the_networks() {
    let networks = ["192.168.0.0/16"].map(|n| n.parse::<IpNet>().unwrap());
    let mut services = factory();
    services[3] = service("www", true, None, None);
    let script = service_script(Some(&networks), services);
    assert!(
        script.contains("set [ find name=www dynamic=no] disabled=no address=\"192.168.0.0/16\""),
        "{script}"
    );
}

/// An open API takes `address`, which every version knows; once RouterOS 7.24 reports
/// `available-from` for it, that is written, and a hand-set restriction elsewhere stays.
#[test]
fn the_api_answers_the_networks() {
    let networks = ["192.168.0.0/16", "2a02:16a:fe00:8000::/50", "172.17.1.5/24"]
        .map(|n| n.parse::<IpNet>().unwrap());
    let script = service_script(Some(&networks), factory());
    assert!(
        script.contains("set [ find name=api dynamic=no] address=\"192.168.0.0/16,2a02:16a:fe00:8000::/50,172.17.1.0/24\""),
        "{script}"
    );
    let mut services = factory();
    services[6] = service("api", false, Some("10.0.0.0/8"), None);
    services[5] = service("winbox", false, Some("10.0.0.0/8"), None);
    let script = service_script(Some(&networks), services);
    assert!(script.contains("set [ find name=api dynamic=no] available-from=\"192.168.0.0/16,"), "{script}");
    assert!(!script.contains("winbox"), "{script}");
    // as RouterOS reports it after the apply: nothing more to do
    let mut services = factory();
    for s in &mut services {
        s.data.disabled |= matches!(&*s.name.0, b"ftp" | b"telnet");
    }
    services[3] = service(
        "www",
        false,
        Some("192.168.0.0/16,2a02:16a:fe00:8000::/50,172.17.1.0/24"),
        None,
    );
    services[6] = service(
        "api",
        false,
        Some("192.168.0.0/16,2a02:16a:fe00:8000::/50,172.17.1.0/24"),
        None,
    );
    assert_eq!(service_script(Some(&networks), services), "");
}

/// A client of the API outside the networks (the provisioner over IPv6 through the management
/// proxy, as at lab-wap): the API stays open, with a warning.
#[test]
fn the_api_stays_open_for_a_client_outside() {
    use crate::device::ros::warning::{ApiClientsOutside, ApplyWarning};
    let networks = ["192.168.0.0/16"].map(|n| n.parse::<IpNet>().unwrap());
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    let mut current: BaseDeviceDataCurrent = empty_current(&target);
    current.ip_service = factory().into();
    target.setup_services(&current.ip_service, Some(&networks));
    let inside = ["192.168.100.1".parse().unwrap()];
    assert_eq!(target.keep_api_open_for(&current.ip_service, &inside, &networks), None);
    let clients = ["192.168.100.1".parse().unwrap(), "2a02:16a:fe00:8000::1".parse().unwrap()];
    assert_eq!(
        target.keep_api_open_for(&current.ip_service, &clients, &networks),
        Some(ApplyWarning::ApiClientsOutside(ApiClientsOutside { clients: vec!["2a02:16a:fe00:8000::1".into()] }))
    );
    let mutations = target.generate_mutations(&current).unwrap();
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in mutations.iter().filter(|m| m.resource == b"ip/service") {
        generator.append_mutation(mutation).unwrap();
    }
    assert!(!script.contains("name=api dynamic=no]"), "{script}");
    assert!(script.contains("name=telnet dynamic=no] disabled=yes"), "{script}");
}

#[test]
fn a_remote_gives_its_address() {
    use crate::device::ros::graphql::remote_address;
    assert_eq!(remote_address("10.0.2.2:40374"), Some("10.0.2.2".parse().unwrap()));
    assert_eq!(remote_address("[2a02:16a:fe00:8000::1]:40374"), Some("2a02:16a:fe00:8000::1".parse().unwrap()));
    // as lab-wap (7.23.8) reports its cloud connection: no brackets, and the port `:0` makes
    // it a valid address of its own
    assert_eq!(remote_address("2a02:610:7501:1000::201:0"), Some("2a02:610:7501:1000::201".parse().unwrap()));
}
