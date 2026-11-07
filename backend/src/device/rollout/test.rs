use super::{Candidate, order};

fn candidate(name: &str, architecture: &str, depth: u32, leaf: bool) -> Candidate {
    Candidate {
        name: name.into(),
        architecture: architecture.into(),
        depth: Some(depth),
        leaf,
        update: true,
        ..Default::default()
    }
}

fn names(candidates: &[Candidate]) -> Vec<(&str, bool)> {
    order(candidates)
        .into_iter()
        .map(|(index, pilot)| (&*candidates[index].name, pilot))
        .collect()
}

#[test]
fn a_pilot_per_architecture_then_from_the_outside_in() {
    let candidates = [
        candidate("core", "arm64", 0, false),
        candidate("switch", "arm", 1, false),
        candidate("ap-1", "arm", 2, true),
        candidate("ap-2", "arm", 2, true),
        candidate("edge", "mipsbe", 1, false),
        candidate("cap", "mipsbe", 3, true),
    ];
    assert_eq!(
        names(&candidates),
        [
            ("cap", true),
            ("ap-1", true),
            ("core", true),
            ("ap-2", false),
            ("switch", false),
            ("edge", false),
        ]
    );
}

#[test]
fn a_controller_comes_after_its_access_points() {
    let mut controller = candidate("controller", "arm", 3, true);
    controller.caps = ["ap-near".into(), "ap-far".into()].into();
    let candidates = [
        candidate("pilot", "arm", 4, true),
        controller,
        candidate("ap-near", "arm", 1, true),
        candidate("ap-far", "arm", 2, true),
        candidate("other", "arm", 1, true),
    ];
    assert_eq!(
        names(&candidates),
        [
            ("pilot", true),
            ("ap-far", false),
            ("ap-near", false),
            ("controller", false),
            ("other", false),
        ]
    );
}

#[test]
fn devices_left_out_have_no_place() {
    let mut up_to_date = candidate("done", "arm", 5, true);
    up_to_date.update = false;
    let candidates = [up_to_date, candidate("todo", "arm", 1, false)];
    assert_eq!(names(&candidates), [("todo", true)]);
}
