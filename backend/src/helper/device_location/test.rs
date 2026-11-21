//! Scope: Tests of `helper::device_location`; nothing else.
use super::*;
use crate::topology::{
    access::AccessTopology,
    test::{example_response, topology_from},
};
use serde_json::json;

/// A topology and the device a test moves.
struct Moving {
    topology: Arc<Topology>,
    device: DeviceAccess,
}

/// A RouterOS device of the example whose tenant is that of its location now: two locations
/// at its site with that tenant (1, where it is, and 3), one with a tenant of other
/// credentials (2).
fn topology() -> Moving {
    let mut raw = example_response();
    let plain = topology_from(raw.clone());
    // the first in NetBox's order whose credentials come from its own tenant: the topology's
    // devices come in no order
    let device = raw["data"]["device_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| !entry["tenant"].is_null())
        .filter_map(|entry| entry["id"].as_str()?.parse().ok().map(DeviceId))
        .filter_map(|id| plain.get_device_by_id(&id))
        .find(|device| device.has_routeros() && device.credentials().is_some())
        .expect("a RouterOS device with credentials of its tenant");
    let site = AccessTopology::id(&device.site().expect("its site"))
        .0
        .to_string();
    let data = &mut raw["data"];
    let entry = data["device_list"]
        .as_array_mut()
        .expect("devices")
        .iter_mut()
        .find(|entry| entry["id"] == device.id().0.to_string().as_str())
        .expect("its entry");
    let tenant = entry["tenant"].clone();
    entry["tenant"] = serde_json::Value::Null;
    entry["location"] = json!({ "id": "1", "tenant": tenant });
    data["tenant_list"]
        .as_array_mut()
        .expect("tenants")
        .push(json!({
            "id": "9001",
            "custom_field_data": { "mikrotik_credentials": "other" },
        }));
    data["location_list"] = json!([
        { "id": "1", "name": "Wohnung", "parent": null, "site": { "id": site.clone() },
          "tenant": tenant },
        { "id": "3", "name": "Keller", "parent": null, "site": { "id": site.clone() },
          "tenant": tenant },
        { "id": "2", "name": "Fremd", "parent": null, "site": { "id": site.clone() },
          "tenant": { "id": "9001" } },
    ]);
    let topology = topology_from(raw);
    let device = topology.get_device_by_id(&device.id()).expect("the device");
    Moving { topology, device }
}

/// A device goes to a location that keeps its credentials; one whose tenant has others is
/// refused, the same location is nothing to change.
#[test]
fn a_room_keeps_the_credentials() {
    let Moving { topology, device } = topology();
    assert_eq!(device.location_id(), Some(1));
    let input = |location| DeviceLocationInput {
        device: device.id().0,
        location,
    };
    let moved = plan(&topology, &input(Some(3))).expect("a plan");
    let after = moved.preview_topology().expect("a preview");
    let moved = after.get_device_by_id(&device.id()).expect("the device");
    assert_eq!(moved.location_id(), Some(3));
    assert_eq!(moved.location(), ["Keller".into()]);
    assert_eq!(moved.credentials(), device.credentials());
    assert!(matches!(
        plan(&topology, &input(Some(2))),
        Err(Error::OtherCredentials(_))
    ));
    assert!(matches!(
        plan(&topology, &input(Some(1))),
        Err(Error::NothingToChange)
    ));
    assert!(matches!(
        plan(&topology, &input(Some(7))),
        Err(Error::UnknownLocation(7))
    ));
}
