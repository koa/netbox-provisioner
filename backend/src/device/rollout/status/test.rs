//! Scope: Tests of `rollout::status` with events built by hand; nothing else.
use super::{RolloutNode, RolloutNodeState as Node, nodes};
use crate::{
    device::rollout::{
        PlannedDevice, RolloutSkip,
        run::{
            RolloutDevice, RolloutDeviceState as State, RolloutPlanned,
            test::{device, plan},
        },
    },
    device_state::UpdateState,
    job::Event,
};

fn told(id: u32, state: State, phase: Option<UpdateState>) -> Event {
    Event::RolloutDevice(RolloutDevice {
        device: id,
        name: format!("d{id}").into(),
        state,
        job: Some(100 + id),
        phase,
        error: None,
    })
}

/// d3 reboots in its update: it and d4 behind it are offline, d2 (which d3 is behind) is not.
/// The device the plan skips isn't on the list.
#[test]
fn each_device_in_its_last_state() {
    let target = PlannedDevice {
        cut_by: [2].into(),
        ..device(3, true)
    };
    let behind_target = PlannedDevice {
        cut_by: [3].into(),
        ..device(4, false)
    };
    let current = PlannedDevice {
        skip: Some(RolloutSkip::UpToDate),
        ..device(1, false)
    };
    let events = [
        Event::RolloutPlanned(RolloutPlanned {
            plan: plan(vec![target, device(2, false), behind_target, current]),
        }),
        told(3, State::Started, Some(UpdateState::Uploading)),
        told(3, State::Started, Some(UpdateState::Rebooting)),
    ];
    assert_eq!(
        nodes(&events),
        [
            RolloutNode {
                device: 3,
                state: Node::Updating,
                offline: true,
            },
            RolloutNode {
                device: 2,
                state: Node::Pending,
                offline: false,
            },
            RolloutNode {
                device: 4,
                state: Node::Pending,
                offline: true,
            },
        ]
    );
}

/// Once its update is through, a device is done and cuts none off.
#[test]
fn a_device_done_cuts_none_off() {
    let events = [
        Event::RolloutPlanned(RolloutPlanned {
            plan: plan(vec![
                device(3, true),
                PlannedDevice {
                    cut_by: [3].into(),
                    ..device(4, false)
                },
            ]),
        }),
        told(3, State::Started, Some(UpdateState::Rebooting)),
        told(3, State::Done, None),
        told(4, State::Failed, None),
    ];
    let got = nodes(&events)
        .into_iter()
        .map(|node| (node.state, node.offline))
        .collect::<Vec<_>>();
    assert_eq!(got, [(Node::Done, false), (Node::Failed, false)]);
}

/// Before the plan is in, there is nothing to show.
#[test]
fn no_plan_no_nodes() {
    assert!(nodes(&[told(3, State::Started, None)]).is_empty());
}
