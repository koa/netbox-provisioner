//! Helper „PoE“: who feeds whom. One port powers the device at the other end of its cable
//! (NetBox `poe_mode=pse`), that device's port is fed (`pd`). The two ports are chosen by hand
//! or taken from what a device reports (`observed`: `/interface/ethernet/poe/monitor`
//! `poe-out-status=powered-on`, the far end from NetBox's cable or else from `/ip/neighbor`).
//! Both ports get their mode, and the cable between them if NetBox lacks it.
//!
//! Whether a board has PoE out at that port NetBox doesn't know (it has no board model); a
//! port without it gets no `poe-out` from the generator anyway (`setup_poe`).
use super::{CreateCable, NetboxPlan, PlanStep, UpdateInterface, port_name};
use crate::{
    Error,
    device::{
        neighbors::{PortRef, port_named},
        raw::{Row, command, field, rows},
    },
    topology::{
        DeviceId, InterfaceId, PoeMode, Topology,
        access::{device::DeviceAccess, interface::InterfaceAccess},
        findings::{InterfaceRef, interface_ref},
        natural::{naturalize, naturalize_interface},
    },
};
use async_graphql::{InputObject, SimpleObject};
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{collections::HashMap, sync::Arc};

/// The port that feeds and the port fed, for the device of `device` (one of their devices).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct PoeInput {
    pub device: u32,
    /// becomes `pse`
    pub pse: u32,
    /// becomes `pd`
    pub pd: u32,
}

pub fn plan(topology: &Arc<Topology>, input: &PoeInput) -> Result<NetboxPlan, Error> {
    let end = |id| {
        topology
            .get_interface_by_id(InterfaceId(id))
            .ok_or(Error::UnknownInterface(id))
    };
    let (pse, pd) = (end(input.pse)?, end(input.pd)?);
    let (Some(pse_device), Some(pd_device)) = (pse.device(), pd.device()) else {
        return Err(Error::UnknownInterface(input.pse));
    };
    if pse_device.id() == pd_device.id() {
        return Err(Error::SameDevice(pse_device.name().into()));
    }
    let device = [&pse_device, &pd_device]
        .into_iter()
        .find(|d| d.id() == DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    for port in [&pse, &pd] {
        if !port.is_ethernet() {
            return Err(Error::NotEthernet(port_name(port)));
        }
    }
    let mut plan = NetboxPlan::new(
        device,
        format!(
            "PoE {} {} → {} {}",
            pse_device.name(),
            pse.name(),
            pd_device.name(),
            pd.name()
        ),
    );
    if !pse.connected_interfaces().iter().any(|i| i.id() == pd.id()) {
        for port in [&pse, &pd] {
            if port.cable().is_some() {
                return Err(Error::PortCabled {
                    port: port_name(port),
                    far: port.connected_interfaces().first().map(port_name),
                });
            }
        }
        plan.steps.push(PlanStep::CreateCable(CreateCable {
            a: interface_ref(&pse),
            b: interface_ref(&pd),
        }));
    }
    for (port, mode) in [(&pse, PoeMode::Pse), (&pd, PoeMode::Pd)] {
        if port.poe() != Some(mode) {
            plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
                interface: interface_ref(port),
                label: None,
                label_before: port.label().map(Box::from),
                mtu: None,
                mtu_before: port.mtu(),
                tags_added: Vec::new(),
                tags_removed: Vec::new(),
                poe: Some(mode),
                poe_before: port.poe(),
            }));
        }
    }
    Ok(plan)
}

/// An Ethernet port to choose for „PoE“, with its mode and where its cable goes.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoePort {
    pub id: u32,
    pub name: Box<str>,
    pub label: Option<Box<str>>,
    pub poe: Option<PoeMode>,
    pub cabled: bool,
    pub connected: Vec<InterfaceRef>,
}

/// Another device of the site, with its Ethernet ports.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoePeer {
    pub id: u32,
    pub name: Box<str>,
    pub ports: Vec<PoePort>,
}

/// What „PoE“ chooses from: the device's Ethernet ports, and the other devices of its site
/// (and those its cables reach) with theirs.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoeOptions {
    pub ports: Vec<PoePort>,
    pub peers: Vec<PoePeer>,
}

pub fn options(device: &DeviceAccess) -> PoeOptions {
    let mut peers: Vec<DeviceAccess> = device
        .site()
        .map(|site| site.devices().collect())
        .unwrap_or_default();
    for far in device
        .interfaces()
        .iter()
        .flat_map(InterfaceAccess::connected_interfaces)
        .filter_map(|far| far.device())
    {
        if !peers.iter().any(|d| d.id() == far.id()) {
            peers.push(far);
        }
    }
    peers.retain(|d| d.id() != device.id());
    peers.sort_by_cached_key(|d| naturalize(d.name()));
    PoeOptions {
        ports: poe_ports(device),
        peers: peers
            .iter()
            .map(|peer| PoePeer {
                id: peer.id().0,
                name: peer.name().into(),
                ports: poe_ports(peer),
            })
            .filter(|peer| !peer.ports.is_empty())
            .collect(),
    }
}

fn poe_ports(device: &DeviceAccess) -> Vec<PoePort> {
    let mut ports: Vec<PoePort> = device
        .interfaces()
        .iter()
        .filter(|interface| interface.is_ethernet())
        .map(|interface| PoePort {
            id: interface.id().0,
            name: interface.name().into(),
            label: interface.label().map(Box::from),
            poe: interface.poe(),
            cabled: interface.cable().is_some(),
            connected: interface
                .connected_interfaces()
                .iter()
                .map(interface_ref)
                .collect(),
        })
        .collect();
    ports.sort_by_cached_key(|port| naturalize_interface(&port.name));
    ports
}

/// A port the device reports feeding something (`poe-out-status=powered-on`), and the port fed
/// as far as it is known: NetBox's cable, else the neighbor the device sees there
/// (`from_neighbor`).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoeObserved {
    pub port: PortRef,
    pub far: Option<InterfaceRef>,
    pub from_neighbor: bool,
}

/// Reads which ports of the device feed something now, and what NetBox and the neighbors say is
/// at their other end.
pub async fn observed(
    client: &RawDevice<SimpleResult>,
    device: &DeviceAccess,
) -> Result<Vec<PoeObserved>, Error> {
    let ethernet = rows(client, "/interface/ethernet/print").await?;
    let with_poe = ethernet
        .iter()
        .filter(|row| field(row, "poe-out").is_some())
        .filter_map(|row| field(row, "name"))
        .collect::<Vec<_>>();
    if with_poe.is_empty() {
        return Ok(Vec::new());
    }
    let monitor = command(
        client,
        "/interface/ethernet/poe/monitor",
        &[("numbers", &with_poe.join(",")), ("once", "")],
    )
    .await?;
    let neighbors = rows(client, "/ip/neighbor/print").await?;
    Ok(compare(device, &ethernet, &monitor, &neighbors))
}

/// [`observed`] on what the device answered: `ethernet` from `/interface/ethernet/print`,
/// `monitor` from the PoE monitor, `neighbors` from `/ip/neighbor/print`.
fn compare(
    device: &DeviceAccess,
    ethernet: &[Row],
    monitor: &[Row],
    neighbors: &[Row],
) -> Vec<PoeObserved> {
    let default_names = ethernet
        .iter()
        .filter_map(|row| Some((field(row, "name")?, field(row, "default-name")?)))
        .collect::<HashMap<_, _>>();
    let devices = device.topology().list_devices().collect::<Vec<_>>();
    let mut observed: Vec<PoeObserved> = monitor
        .iter()
        .filter(|row| field(row, "poe-out-status") == Some("powered-on"))
        .filter_map(|row| field(row, "name"))
        .map(|name| {
            let default_name = default_names.get(name).copied().unwrap_or(name);
            let interface = device
                .interfaces()
                .into_iter()
                .find(|i| i.external_port().is_some() && i.name() == default_name);
            let cabled = interface
                .as_ref()
                .and_then(|i| i.connected_interfaces().into_iter().next());
            let seen = || {
                neighbors
                    .iter()
                    .filter(|row| {
                        field(row, "interface")
                            .is_some_and(|i| i.split(',').any(|i| i == name))
                    })
                    .find_map(|row| {
                        let identity = field(row, "identity")?;
                        let far_device = devices.iter().find(|d| d.name() == identity)?;
                        port_named(far_device, field(row, "interface-name")?)
                    })
            };
            let (far, from_neighbor) = match cabled {
                Some(far) => (Some(far), false),
                None => {
                    let seen = seen();
                    let from_neighbor = seen.is_some();
                    (seen, from_neighbor)
                }
            };
            PoeObserved {
                port: PortRef {
                    name: name.into(),
                    interface: interface.as_ref().map(interface_ref),
                },
                far: far.as_ref().map(interface_ref),
                from_neighbor,
            }
        })
        .collect();
    observed.sort_by_cached_key(|o| naturalize_interface(&o.port.name));
    observed
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        device::ros::netbox_example_test::example_topology,
        topology::{
            Cable, CablePort, Device, Interface, PhysicalPortId, PortType, test::TopologyBuilder,
        },
    };
    use std::str::FromStr;

    /// Two Ethernet ports of different devices of the export, cabled to each other.
    fn cabled_ports(topology: &Arc<Topology>) -> (InterfaceAccess, InterfaceAccess) {
        topology
            .list_devices()
            .filter(|d| d.has_routeros())
            .flat_map(|d| d.interfaces().into_vec())
            .find_map(|a| {
                let b = a.connected_interfaces().first()?.clone();
                (a.is_ethernet() && b.is_ethernet() && a.poe().is_none() && b.poe().is_none())
                    .then_some((a, b))
            })
            .expect("a cable between Ethernet ports")
    }

    /// Both ports get their mode, the preview has them; asked from the far device it is the
    /// same plan, and once NetBox has the modes nothing is left.
    #[test]
    fn both_ports_get_their_mode() {
        let topology = example_topology();
        let (pse, pd) = cabled_ports(&topology);
        let input = |device: &InterfaceAccess| PoeInput {
            device: device.device().unwrap().id().0,
            pse: pse.id().0,
            pd: pd.id().0,
        };
        let plan = super::plan(&topology, &input(&pse)).expect("plan");
        let modes = plan
            .steps
            .iter()
            .map(|step| match step {
                PlanStep::UpdateInterface(update) => (update.interface.id, update.poe),
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            modes,
            [(pse.id().0, Some(PoeMode::Pse)), (pd.id().0, Some(PoeMode::Pd))]
        );
        let from_far = super::plan(&topology, &input(&pd)).expect("plan");
        assert_eq!(from_far.steps, plan.steps);

        let after = plan.preview_topology().expect("preview");
        let pse_after = after.get_interface_by_id(pse.id()).unwrap();
        let pd_after = after.get_interface_by_id(pd.id()).unwrap();
        assert!(pse_after.enable_poe());
        assert_eq!(pd_after.poe(), Some(PoeMode::Pd));
        let again = super::plan(&after, &input(&pse)).expect("plan");
        assert!(again.steps.is_empty(), "{:?}", again.steps);
    }

    /// Without a cable between them, the plan cables two free ports; a port cabled elsewhere
    /// is refused.
    #[test]
    fn a_cable_is_added_or_in_the_way() {
        let topology = example_topology();
        let (pse, cabled) = cabled_ports(&topology);
        let free = topology
            .list_devices()
            .filter(|d| Some(d.id()) != pse.device().map(|d| d.id()))
            .flat_map(|d| d.interfaces().into_vec())
            .find(|i| i.is_ethernet() && i.cable().is_none())
            .expect("a free port");
        let input = PoeInput {
            device: pse.device().unwrap().id().0,
            pse: pse.id().0,
            pd: free.id().0,
        };
        match super::plan(&topology, &input) {
            Err(Error::PortCabled { far: Some(far), .. }) => {
                assert!(far.ends_with(cabled.name()), "{far}")
            }
            other => panic!("{:?}", other.map(|plan| plan.steps)),
        }
        let other_free = topology
            .list_devices()
            .filter(|d| Some(d.id()) != free.device().map(|d| d.id()))
            .flat_map(|d| d.interfaces().into_vec())
            .find(|i| i.is_ethernet() && i.cable().is_none())
            .expect("another free port");
        let input = PoeInput {
            device: free.device().unwrap().id().0,
            pse: other_free.id().0,
            pd: free.id().0,
        };
        let plan = super::plan(&topology, &input).expect("plan");
        assert!(matches!(&plan.steps[0], PlanStep::CreateCable(c)
            if c.a.id == other_free.id().0 && c.b.id == free.id().0));
        assert_eq!(plan.steps.len(), 3);
    }

    fn device(builder: &mut TopologyBuilder, name: &str) -> DeviceId {
        let id = builder.devices.next_id();
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                has_routeros: name == "sw",
                ..Device::default()
            },
        );
        id
    }
    fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str) -> InterfaceId {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external: PhysicalPortId::from_str(name).ok(),
                port_type: Some(PortType::Ethernet),
                ..Interface::default()
            },
        );
        id
    }
    fn row(fields: &[(&str, &str)]) -> Row {
        fields
            .iter()
            .map(|(k, v)| (Box::from(*k), Some(Box::from(*v))))
            .collect()
    }

    /// `sw` feeds on `ether2` (renamed, cabled to `ap` in NetBox) and `ether3` (no cable, `cam`
    /// seen there); `ether4` only waits for a load.
    #[test]
    fn powered_ports_with_their_far_end() {
        let mut builder = TopologyBuilder::default();
        let sw = device(&mut builder, "sw");
        let ap = device(&mut builder, "ap");
        let cam = device(&mut builder, "cam");
        let sw2 = port(&mut builder, sw, "ether2");
        let sw3 = port(&mut builder, sw, "ether3");
        port(&mut builder, sw, "ether4");
        let ap1 = port(&mut builder, ap, "ether1");
        let cam1 = port(&mut builder, cam, "eth0");
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(sw2)]),
                port_b: Box::new([CablePort::Interface(ap1)]),
            },
        );
        let topology = Arc::new(builder.build());
        let sw = topology.get_device_by_id(&sw).unwrap();
        let ethernet = [
            row(&[("name", "e02-ap"), ("default-name", "ether2"), ("poe-out", "auto-on")]),
            row(&[("name", "ether3"), ("default-name", "ether3"), ("poe-out", "auto-on")]),
            row(&[("name", "ether4"), ("default-name", "ether4"), ("poe-out", "auto-on")]),
        ];
        let monitor = [
            row(&[("name", "e02-ap"), ("poe-out-status", "powered-on")]),
            row(&[("name", "ether3"), ("poe-out-status", "powered-on")]),
            row(&[("name", "ether4"), ("poe-out-status", "waiting-for-load")]),
        ];
        let neighbors = [row(&[
            ("interface", "ether3,bridge"),
            ("identity", "cam"),
            ("interface-name", "eth0"),
        ])];
        let observed = compare(&sw, &ethernet, &monitor, &neighbors);
        let seen = observed
            .iter()
            .map(|o| {
                (
                    &*o.port.name,
                    o.port.interface.as_ref().map(|i| i.id),
                    o.far.as_ref().map(|i| i.id),
                    o.from_neighbor,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            seen,
            [
                ("e02-ap", Some(sw2.0), Some(ap1.0), false),
                ("ether3", Some(sw3.0), Some(cam1.0), true),
            ]
        );
    }
}
