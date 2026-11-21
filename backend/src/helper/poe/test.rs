//! Scope: Tests of `poe`; nothing else.
use super::*;
use crate::topology::test::example_topology;

/// The two ends of a cable.
struct Cable {
    a: InterfaceAccess,
    b: InterfaceAccess,
}

/// Two Ethernet ports of different devices of the export, cabled to each other; the first by
/// id, the topology's come in no order.
fn cabled_ports(topology: &Arc<Topology>) -> Cable {
    let mut ports = topology
        .list_devices()
        .filter(|d| d.has_routeros())
        .flat_map(|d| d.interfaces().into_vec())
        .collect::<Vec<_>>();
    ports.sort_by_key(|port| port.id().0);
    ports
        .into_iter()
        .find_map(|a| {
            let b = a.connected_interfaces().first()?.clone();
            (a.is_ethernet() && b.is_ethernet() && a.poe().is_none() && b.poe().is_none())
                .then_some(Cable { a, b })
        })
        .expect("a cable between Ethernet ports")
}

/// Both ports get their mode, the preview has them; asked from the far device it is the
/// same plan, and once NetBox has the modes nothing is left.
#[test]
fn both_ports_get_their_mode() {
    let topology = example_topology();
    let Cable { a: pse, b: pd } = cabled_ports(&topology);
    let input = |device: &InterfaceAccess| PoeInput {
        device: device.device().unwrap().id().0,
        pse: pse.id().0,
        pd: Some(pd.id().0),
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
        [
            (pse.id().0, Some(PoeMode::Pse)),
            (pd.id().0, Some(PoeMode::Pd))
        ]
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
    let Cable { a: pse, b: cabled } = cabled_ports(&topology);
    let free = topology
        .list_devices()
        .filter(|d| Some(d.id()) != pse.device().map(|d| d.id()))
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| i.is_ethernet() && i.cable().is_none())
        .expect("a free port");
    let input = PoeInput {
        device: pse.device().unwrap().id().0,
        pse: pse.id().0,
        pd: Some(free.id().0),
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
        pd: Some(free.id().0),
    };
    let plan = super::plan(&topology, &input).expect("plan");
    assert!(matches!(&plan.steps[0], PlanStep::CreateCable(c)
            if c.a.id == other_free.id().0 && c.b.id == free.id().0));
    // an update for each port that has another mode
    let updates = usize::from(other_free.poe() != Some(PoeMode::Pse))
        + usize::from(free.poe() != Some(PoeMode::Pd));
    assert_eq!(plan.steps.len(), 1 + updates, "{:?}", plan.steps);
}

/// „Kein PoE“ takes the mode off the port and the counterpart off its far end; asked again
/// on the result nothing is left.
#[test]
fn no_poe_takes_both_ends() {
    let topology = example_topology();
    let Cable { a: pse, b: pd } = cabled_ports(&topology);
    let paired = super::plan(
        &topology,
        &PoeInput {
            device: pse.device().unwrap().id().0,
            pse: pse.id().0,
            pd: Some(pd.id().0),
        },
    )
    .expect("plan")
    .preview_topology()
    .expect("preview");
    let input = |port: &InterfaceAccess| PoeRemoveInput {
        device: port.device().unwrap().id().0,
        port: port.id().0,
    };
    let plan = plan_remove(&paired, &input(&pd)).expect("plan");
    let removed = plan
        .steps
        .iter()
        .map(|step| match step {
            PlanStep::UpdateInterface(update) => {
                assert!(update.poe_removed && update.poe.is_none());
                (update.interface.id, update.poe_before)
            }
            other => panic!("{other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        removed,
        [
            (pd.id().0, Some(PoeMode::Pd)),
            (pse.id().0, Some(PoeMode::Pse))
        ]
    );
    let after = plan.preview_topology().expect("preview");
    for port in [&pse, &pd] {
        assert_eq!(after.get_interface_by_id(port.id()).unwrap().poe(), None);
    }
    assert!(
        plan_remove(&after, &input(&pse))
            .expect("plan")
            .steps
            .is_empty()
    );
}

/// Without the port fed, the feeding one alone gets `pse`, no cable.
#[test]
fn poe_on_before_the_other_end_is_known() {
    let topology = example_topology();
    let port = topology
        .list_devices()
        .filter(|d| d.has_routeros())
        .flat_map(|d| d.interfaces().into_vec())
        .find(|i| i.is_ethernet() && i.cable().is_none() && i.poe().is_none())
        .expect("a free port");
    let plan = super::plan(
        &topology,
        &PoeInput {
            device: port.device().unwrap().id().0,
            pse: port.id().0,
            pd: None,
        },
    )
    .expect("plan");
    assert!(
        matches!(&plan.steps[..], [PlanStep::UpdateInterface(u)]
                if u.interface.id == port.id().0 && u.poe == Some(PoeMode::Pse)),
        "{:?}",
        plan.steps
    );
}
