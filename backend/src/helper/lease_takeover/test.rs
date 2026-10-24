use super::*;
use crate::topology::interfaces::test::Net;

fn facts() -> Facts {
    Facts {
        device_type: "Generic".into(),
        manufacturer: "generic".into(),
        role: "Client".into(),
        role_slug: "client".into(),
    }
}

/// The net with r1 at a site.
fn net() -> Net {
    let mut net = Net::new();
    let site = net.builder.sites.next_id();
    net.builder.sites.insert(
        site,
        crate::topology::SiteData {
            name: "Lab".into(),
            devices: Box::new([net.r1]),
        },
    );
    net.builder.devices.get_mut(&net.r1).unwrap().site = Some(site);
    net
}

fn input(net: &Net, address: &str) -> LeaseTakeoverInput {
    LeaseTakeoverInput {
        device: net.r1.0,
        address: address.into(),
        mac: "aa:bb:cc:dd:ee:01".into(),
        name: "drucker".into(),
        device_type: 8,
        role: 2,
    }
}

/// A lease in the customer network (`bridge-clients`, `dhcp`) becomes a device with `eth0`,
/// its MAC in upper case and the address with the network's length.
#[test]
fn a_lease_becomes_a_client_device() {
    let net = net();
    let input = input(&net, "10.10.0.50");
    let topology = Arc::new(net.builder.build());
    let plan = base(&topology, &input, &facts()).expect("plan");
    assert!(
        matches!(&plan.steps[..], [
            PlanStep::CreateDevice(device),
            PlanStep::CreateInterface(interface),
            PlanStep::CreateAddress(address),
        ] if &*device.name == "drucker" && device.platform_id.is_none()
            && interface.mac.as_deref() == Some("AA:BB:CC:DD:EE:01")
            && &*address.address == "10.10.0.50/24" && address.on == Target::Step(1)),
        "{:?}",
        plan.steps
    );
}

/// An address in no DHCP network of the router, one NetBox has, a bad MAC and a taken name
/// are refused.
#[test]
fn what_doesnt_fit_is_refused() {
    let net = net();
    let outside = input(&net, "192.168.5.1");
    let taken = input(&net, "10.10.0.1");
    let bad_mac = LeaseTakeoverInput {
        mac: "aa:bb".into(),
        ..input(&net, "10.10.0.50")
    };
    let name = LeaseTakeoverInput {
        name: "r2".into(),
        ..input(&net, "10.10.0.50")
    };
    let topology = Arc::new(net.builder.build());
    assert!(matches!(base(&topology, &outside, &facts()), Err(Error::NoDhcpNetwork(_))));
    assert!(matches!(base(&topology, &taken, &facts()), Err(Error::AddressInNetbox(_))));
    assert!(matches!(base(&topology, &bad_mac, &facts()), Err(Error::InvalidMac(_))));
    assert!(matches!(base(&topology, &name, &facts()), Err(Error::DeviceNameTaken(_))));
}

/// In the preview of the stored export, the router's DHCP network has the client as a static
/// lease, with its MAC and name.
#[test]
fn the_preview_has_the_lease() {
    let topology = crate::topology::test::example_topology();
    let (router, net) = topology
        .list_devices()
        .filter(|d| d.has_routeros() && d.site().is_some())
        .find_map(|d| {
            let net = d
                .interfaces()
                .iter()
                .filter(|i| i.is_enable_dhcp_server())
                .flat_map(|i| i.ips())
                .filter(|ip| ip.prefix().is_some())
                .find_map(|ip| match ip.net() {
                    Some(IpNet::V4(net)) if net.prefix_len() < 30 => Some(net),
                    _ => None,
                })?;
            Some((d, net))
        })
        .expect("a router with a DHCP network");
    let address = net
        .hosts()
        .find(|host| topology.ip_address_with_host(IpAddr::V4(*host)).is_none())
        .expect("a free address");
    let input = LeaseTakeoverInput {
        device: router.id().0,
        address: address.to_string(),
        mac: "aa:bb:cc:dd:ee:01".into(),
        name: "lease-test-client".into(),
        device_type: 8,
        role: 2,
    };
    let plan = base(&topology, &input, &facts()).expect("plan");
    let after = plan.preview_topology().expect("preview");
    let router = after.get_device_by_id(&router.id()).unwrap();
    let leases = router
        .interfaces()
        .iter()
        .filter(|i| i.is_enable_dhcp_server())
        .flat_map(|i| i.ips())
        .filter_map(|ip| ip.prefix())
        .flat_map(|prefix| crate::device::ros::dhcp::static_leases(&prefix))
        .filter(|lease| lease.address == address)
        .map(|lease| (lease.mac.to_string(), lease.name))
        .collect::<Vec<_>>();
    assert_eq!(leases, [("AA:BB:CC:DD:EE:01".to_string(), "lease-test-client".into())]);
}
