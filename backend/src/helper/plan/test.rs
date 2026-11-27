//! Scope: Tests of `helper::plan`; nothing else.
use super::*;
use crate::{
    helper::NetboxPlan,
    topology::{InterfaceId, test::example_topology},
};

/// NetBox's topology holds NetBox ids only, even at the top of the range; a preview of two
/// steps turns their ids back into the steps, and the rest stays NetBox's.
#[test]
fn a_preview_gives_its_steps_objects_back() {
    let netbox = example_topology();
    assert_eq!(
        ObjectId::in_topology(&netbox, u32::MAX),
        ObjectId::Netbox(u32::MAX)
    );
    let preview = netbox
        .rebuilt_with(2, |_| {})
        .expect("a source")
        .expect("built");
    for step in [0, 1] {
        let id = ObjectId::Planned(step);
        assert_eq!(ObjectId::in_topology(&preview, id.preview_id()), id);
    }
    assert_eq!(
        ObjectId::in_topology(&preview, u32::MAX - 2),
        ObjectId::Netbox(u32::MAX - 2)
    );
    assert_eq!(ObjectId::in_topology(&preview, 7), ObjectId::Netbox(7));
}

/// A part computed on a plan's preview refers to the interface the plan creates as that plan's
/// step and goes behind it; one computed on NetBox as it is counts its steps from 0 again and
/// is refused.
#[test]
fn a_part_on_the_preview_continues_the_plan() {
    let topology = example_topology();
    let device = topology
        .list_devices()
        .find(|d| d.has_routeros())
        .expect("a router");
    let mut plan = NetboxPlan::new(&device, None);
    plan.steps.push(PlanStep::CreateInterface(CreateInterface {
        device: plan_device(&device),
        name: "lo-test".into(),
        kind: "virtual".into(),
        vlan: None,
        tags: Box::default(),
        mac: None,
    }));
    let preview = plan.preview_topology().expect("a preview");
    let created = preview
        .get_interface_by_id(InterfaceId(ObjectId::Planned(0).preview_id()))
        .expect("the new interface");
    let on_preview = preview.get_device_by_id(&device.id()).expect("the router");
    let address = |device: &DeviceAccess, on: ObjectId| {
        let mut part = NetboxPlan::new(device, None);
        part.steps.push(PlanStep::CreateAddress(CreateAddress {
            address: "192.0.2.1/32".into(),
            device: plan_device(device),
            interface: "lo-test".into(),
            prefix: "192.0.2.0/24".into(),
            on,
        }));
        part
    };
    let mut part = address(&on_preview, plan_interface(&created).id);
    assert_eq!(part.created_by(0), ObjectId::Planned(1));
    plan.append(part).expect("on the preview");
    assert!(matches!(
        &plan.steps[1],
        PlanStep::CreateAddress(CreateAddress {
            on: ObjectId::Planned(0),
            ..
        })
    ));
    part = address(&device, ObjectId::Planned(0));
    assert!(plan.append(part).is_err());
}
