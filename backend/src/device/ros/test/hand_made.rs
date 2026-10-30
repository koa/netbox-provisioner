//! Addresses on interfaces set up by hand stay (`BaseDeviceDataTarget::keep_hand_made_addresses`):
//! the tunnels of an uplink router, as BGP.
use super::empty_current;
use crate::device::ros::BaseDeviceDataTarget;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use mikrotik_model::{
    model::{IpAddressByAddress, IpAddressCfg, Ipv6AddressByAddress, Ipv6AddressCfg},
    resource::ResourceMutationOperation,
};

fn v4(address: &str, interface: &str) -> IpAddressByAddress {
    IpAddressByAddress(IpAddressCfg {
        address: address.parse().unwrap(),
        interface: interface.into(),
        comment: None,
    })
}

/// A GRE and an IPIP tunnel keep their addresses, IPv4 and IPv6; one on a removed interface
/// (`*35`) and one NetBox doesn't have on a port go, NetBox' own address comes.
#[test]
fn addresses_on_tunnels_stay() {
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    let own: Ipv4Net = "172.17.0.1/30".parse().unwrap();
    target.set_ip_address(IpNet::V4(own), "ether2");
    let mut current = empty_current(&target);
    let tunnel_v6: Ipv6Net = "fd7e:907d:34ab::400:0/127".parse().unwrap();
    current.ipv_4_address = Box::new([
        v4("10.0.38.2/30", "gre-colab"),
        v4("172.17.1.61/30", "ipipv6-loch"),
        v4("10.195.17.1/24", "*35"),
        v4("192.168.9.1/24", "ether2"),
    ]);
    current.ipv_6_address = Box::new([Ipv6AddressByAddress(Ipv6AddressCfg {
        address: tunnel_v6,
        interface: "gre-lkw".into(),
        advertise: false,
        auto_link_local: true,
        comment: None,
        disabled: false,
        eui_64: false,
        from_pool: None,
        no_dad: false,
    })]);
    target.keep_hand_made_addresses(&current, &[]);

    let changes = target
        .generate_mutations(&current)
        .unwrap()
        .iter()
        // an unchanged entry is an update without fields, which the script leaves out
        .filter(|m| m.resource.ends_with(b"address"))
        .filter(|m| !matches!(m.operation, ResourceMutationOperation::UpdateByKey(_)) || !m.fields.is_empty())
        .map(|m| {
            let text = |value: &[u8]| String::from_utf8_lossy(value).into_owned();
            match &m.operation {
                ResourceMutationOperation::RemoveByKey(key) => format!("remove {}", text(&key.value)),
                ResourceMutationOperation::Add => {
                    let address = m.fields.iter().find(|field| *field.key == *b"address");
                    format!("add {}", address.map(|field| text(&field.value)).unwrap_or_default())
                }
                other => format!("{other:?} {:?}", m.fields),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        changes,
        ["remove 10.195.17.1/24", "remove 192.168.9.1/24", "add 172.17.0.1/30"],
    );
}
