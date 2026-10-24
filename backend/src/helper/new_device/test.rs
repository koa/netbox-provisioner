use super::*;
use crate::topology::test::example_topology;

/// A router of the stored export with a free Ethernet port, and the router's tenant.
fn free_port(topology: &Arc<Topology>) -> (DeviceAccess, InterfaceAccess) {
    topology
        .list_devices()
        .filter(|d| d.has_routeros() && d.credentials().is_some() && d.site().is_some())
        .find_map(|d| {
            let port = d
                .interfaces()
                .into_vec()
                .into_iter()
                .find(|i| i.is_ethernet_port() && i.cable().is_none())?;
            Some((d, port))
        })
        .expect("a router with a free port")
}

fn input(port: &InterfaceAccess, name: &str) -> NewDeviceInput {
    NewDeviceInput {
        port: port.id().0,
        name: name.into(),
        device_type: 7,
        role: 1,
        far_port: "ether1".into(),
        wlan_group: None,
    }
}

fn facts(role_slug: &str) -> Facts {
    Facts {
        device_type: "hAP ac".into(),
        manufacturer: "mikrotik".into(),
        role: role_slug.into(),
        role_slug: role_slug.into(),
        platform_id: 1,
        platform: PLATFORM.into(),
        tenant_id: None,
    }
}

/// The preview has the device at the router's site, with the router's credentials and the port
/// the cable lands on; a router gets no cable yet, „Verbindung“ adds it.
#[test]
fn the_preview_has_the_device_with_its_port() {
    let topology = example_topology();
    let (router, port) = free_port(&topology);
    let plan = base(&topology, &input(&port, " ap-neu "), &facts("router")).expect("plan");
    assert!(
        matches!(&plan.steps[..], [PlanStep::CreateDevice(device), PlanStep::CreateInterface(port)]
            if &*device.name == "ap-neu" && port.device.id == u32::MAX && &*port.name == "ether1"),
        "{:?}",
        plan.steps
    );
    let after = plan.preview_topology().expect("preview");
    let device = after.get_device_by_id(&DeviceId(u32::MAX)).expect("the new device");
    assert_eq!(device.name(), "ap-neu");
    assert!(device.has_routeros());
    assert_eq!(device.credentials(), router.credentials());
    assert_eq!(device.site().map(|s| s.id()), router.site().map(|s| s.id()));
    assert_eq!(device.role(), Some(DeviceRole::Router));
    assert!(device.primary_ip_v4().is_none());
    let names = device.interfaces().iter().map(|i| i.name().to_string()).collect::<Vec<_>>();
    assert_eq!(names, ["ether1"]);
}

/// A bridge device gets its cable in the plan already: no link of its own, the radio link
/// comes from „Funkbrücke“.
#[test]
fn a_bridge_device_is_cabled() {
    let topology = example_topology();
    let (_, port) = free_port(&topology);
    let plan = base(&topology, &input(&port, "bridge-neu"), &facts("wlan-bridge")).expect("plan");
    let after = plan.preview_topology().expect("preview");
    let far = after
        .get_interface_by_id(InterfaceId(port.id().0))
        .expect("the port")
        .connected_interfaces()
        .first()
        .and_then(|far| Some((far.device()?.name().to_string(), far.name().to_string())));
    assert_eq!(far, Some(("bridge-neu".into(), "ether1".into())));
}

/// A name NetBox has (in any case), an empty one and a port cabled to an interface are refused.
#[test]
fn a_taken_name_and_a_cabled_port_are_refused() {
    let topology = example_topology();
    let (router, port) = free_port(&topology);
    let taken = router.name().to_uppercase();
    assert!(matches!(
        base(&topology, &input(&port, &taken), &facts("router")),
        Err(Error::DeviceNameTaken(_))
    ));
    assert!(matches!(
        base(&topology, &input(&port, "  "), &facts("router")),
        Err(Error::NoDeviceName)
    ));
    let cabled = topology
        .list_devices()
        .filter(|d| d.has_routeros())
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| !i.connected_interfaces().is_empty())
        .expect("a port cabled to an interface");
    assert!(matches!(
        base(&topology, &input(&cabled, "neu"), &facts("router")),
        Err(Error::PortCabled { .. })
    ));
}

/// An access point gets the WLAN group chosen; one without controller is refused.
#[test]
fn an_access_point_gets_its_wlan_group() {
    let topology = example_topology();
    let (_, port) = free_port(&topology);
    let group = topology
        .list_wlan_groups()
        .find(|g| g.controller().is_some())
        .expect("a WLAN group");
    let mut input = input(&port, "ap-neu");
    input.wlan_group = Some(group.id().0);
    let plan = base(&topology, &input, &facts("router")).expect("plan");
    let after = plan.preview_topology().expect("preview");
    let device = after.get_device_by_id(&DeviceId(u32::MAX)).expect("the new device");
    assert_eq!(device.wlan_ap_of().map(|g| g.id()), Some(group.id()));
    input.wlan_group = Some(u32::MAX);
    assert!(matches!(
        base(&topology, &input, &facts("router")),
        Err(Error::UnknownWlanGroup(_))
    ));
}

/// Steps planned on the preview refer to created objects by their preview ids: written, they
/// get NetBox's, an object not written yet stops the step.
#[test]
fn preview_ids_become_netbox_ids() {
    let device = DeviceRef {
        id: u32::MAX,
        name: "ap-neu".into(),
        site: None,
    };
    let step = PlanStep::CreateCable(CreateCable {
        a: InterfaceRef {
            id: 12,
            name: "ether5".into(),
            device: DeviceRef {
                id: 3,
                name: "router".into(),
                site: None,
            },
        },
        b: InterfaceRef {
            id: u32::MAX - 1,
            name: "ether1".into(),
            device,
        },
        a_end: CableEnd::Interface,
    });
    let real = |id: u32| match id {
        u32::MAX => Some(40),
        id if id == u32::MAX - 1 => Some(400),
        id => Some(id),
    };
    let Some(PlanStep::CreateCable(cable)) = step.with_real_ids(real) else {
        panic!("translated");
    };
    assert_eq!((cable.a.id, cable.a.device.id), (12, 3));
    assert_eq!((cable.b.id, cable.b.device.id), (400, 40));
    assert!(step.with_real_ids(|id| (id < 1000).then_some(id)).is_none());
}

/// A router port whose cable ends open at a patch panel: the new cable starts there, for a
/// router too, and on the preview the port reaches the new device through the panels (so
/// „Verbindung“ adds no cable of its own). One patched through to an interface has none.
#[test]
fn an_open_patch_panel_gets_the_cable() {
    let topology = example_topology();
    let routers = || topology.list_devices().filter(|d| d.has_routeros() && d.site().is_some());
    let port = routers()
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| i.is_ethernet_port() && matches!(landing(i), Some(Landing::Panel(..))))
        .expect("a port patched to an open end");
    let Some(Landing::Panel(end, panel)) = landing(&port) else { unreachable!() };
    let plan = base(&topology, &input(&port, "ap-neu"), &facts("router")).expect("plan");
    assert!(
        matches!(&plan.steps[2], PlanStep::CreateCable(cable)
            if cable.a == panel && cable.a_end == end && cable.b.id == new_port_id()),
        "{:?}",
        plan.steps
    );
    let after = plan.preview_topology().expect("preview");
    let reached = after
        .get_interface_by_id(port.id())
        .expect("the port")
        .connected_interfaces()
        .iter()
        .map(|far| far.id().0)
        .collect::<Vec<_>>();
    assert_eq!(reached, [new_port_id()]);
    let through = routers()
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| {
            i.cable().is_some_and(|cable| {
                cable
                    .port_a()
                    .chain(cable.port_b())
                    .any(|end| !matches!(end, crate::topology::access::cable::CablePortAccess::Interface(_)))
            }) && !i.connected_interfaces().is_empty()
        })
        .expect("a port patched through to an interface");
    assert_eq!(landing(&through), None);
}
