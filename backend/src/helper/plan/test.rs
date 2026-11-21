//! Scope: Tests of `helper::plan`; nothing else.
use super::*;

fn device(id: u32) -> DeviceRef {
    DeviceRef {
        id,
        name: "device".into(),
        site: None,
    }
}

fn interface(id: u32, device_id: u32) -> InterfaceRef {
    InterfaceRef {
        id,
        name: "ether2".into(),
        device: device(device_id),
    }
}

/// The ids a preview gave: `u32::MAX` is NetBox's 500 now, `u32::MAX - 1` isn't written yet.
fn real(id: u32) -> Option<u32> {
    match id {
        u32::MAX => Some(500),
        id if id > u32::MAX - 10 => None,
        id => Some(id),
    }
}

/// A cable the plan deletes and a device it moves get the real ids of objects created before,
/// in every reference.
#[test]
fn deletes_and_rooms_get_the_real_ids() {
    let delete = PlanStep::DeleteCable(DeleteCable {
        id: 7,
        port: interface(u32::MAX, 3),
        far: Some(interface(12, u32::MAX)),
    });
    let Some(PlanStep::DeleteCable(delete)) = delete.with_real_ids(real) else {
        panic!("a cable");
    };
    assert_eq!(delete.id, 7);
    assert_eq!(delete.port.id, 500);
    assert_eq!(delete.far.expect("its far end").device.id, 500);
    let room = PlanStep::UpdateDeviceLocation(UpdateDeviceLocation {
        device: device(u32::MAX - 1),
        location: None,
        location_before: None,
    });
    assert!(room.with_real_ids(real).is_none());
}
