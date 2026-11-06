//! `/ip/socks/access` of the management proxy (`setup_management_proxy`, A3): with
//! `api_networks` an allow for each, then a deny for the rest, after the rules set up by hand.
use crate::device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, test::empty_current};
use ipnet::IpNet;
use mikrotik_model::{
    generator::Generator,
    model::{IpSocksAccessAction, IpSocksAccessById, IpSocksAccessCfg},
    value::Id,
};

fn rule(
    action: IpSocksAccessAction,
    src_address: Option<&str>,
    comment: Option<&str>,
) -> IpSocksAccessCfg {
    IpSocksAccessCfg {
        action,
        comment: comment.map(Into::into),
        disabled: false,
        dst_address: None,
        dst_port: None,
        src_address: src_address.map(Into::into),
        src_port: None,
    }
}

/// The script lines of `/ip/socks/access`, the proxy set up with `networks`.
fn access_script(networks: &[&str], rules: Vec<IpSocksAccessCfg>) -> String {
    let networks = networks
        .iter()
        .map(|n| n.parse::<IpNet>().unwrap())
        .collect::<Vec<_>>();
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    let mut current: BaseDeviceDataCurrent = empty_current(&target);
    current.socks_access = rules
        .into_iter()
        .zip(1..)
        .map(|(data, id)| IpSocksAccessById { id: Id(id), data })
        .collect();
    target.setup_management_proxy("prov", "secret", &networks);
    let mut script = String::new();
    let mut generator = Generator::new(&mut script);
    for mutation in target.generate_mutations(&current).unwrap().iter() {
        if mutation.resource == b"ip/socks/access" {
            generator.append_mutation(mutation).unwrap();
        }
    }
    script
}

#[test]
fn only_the_networks_of_the_api_get_through() {
    let script = access_script(
        &["192.168.0.0/16", "2a02:16a:fe00:8000::/50", "10.1.2.3/32"],
        vec![],
    );
    assert!(script.contains("action=allow comment=\"netbox-provisioner: provisioner 192.168.0.0/16\" disabled=no src-address=\"192.168.0.0/16\""));
    assert!(script.contains("src-address=\"2a02:16a:fe00:8000::/50\""));
    // as RouterOS keeps it, or the next diff would set it again (CHR 7.24.5)
    assert!(script.contains("src-address=\"10.1.2.3\""));
    let deny = script.find("action=deny").expect("a deny for the rest");
    assert!(
        script.rfind("action=allow").unwrap() < deny,
        "the deny comes after the allows"
    );
}

#[test]
fn without_networks_of_the_api_the_proxy_takes_every_source() {
    assert_eq!(access_script(&[], vec![]), "");
}

/// A rule set up by hand stays, after the provisioner's allows and before its deny; an own
/// rule of an earlier `api_networks` is taken over for the new one.
#[test]
fn a_hand_made_rule_stays_before_the_deny() {
    let script = access_script(
        &["192.168.0.0/16"],
        vec![
            rule(IpSocksAccessAction::Allow, Some("10.9.0.0/16"), Some("vpn")),
            rule(
                IpSocksAccessAction::Allow,
                Some("172.16.0.0/12"),
                Some("netbox-provisioner: provisioner 172.16.0.0/12"),
            ),
        ],
    );
    assert!(
        !script.contains("10.9.0.0/16"),
        "the hand-made rule is left alone"
    );
    assert!(script.contains("set [ find .id=*2] comment=\"netbox-provisioner: provisioner 192.168.0.0/16\" src-address=\"192.168.0.0/16\""));
    assert!(script.contains(
        "move [find comment=\"netbox-provisioner: provisioner 192.168.0.0/16\"] destination=*1"
    ));
    assert!(script.ends_with("move [find comment=\"netbox-provisioner: last rest\"]\n"));
}
