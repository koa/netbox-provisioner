//! `force_layout` is continuous, not exact, so these check properties (pinned nodes don't move,
//! positions stay in bounds, connected nodes end up closer than unconnected ones), not exact
//! coordinates.

use super::*;

/// No edges, several nodes: repulsion alone keeps them apart (not all collapsed to one point)
/// and inside the area.
#[test]
fn unconnected_nodes_spread_out_within_bounds() {
    let nodes = [1, 2, 3, 4, 5];
    let side = area_side(nodes.len());
    let pos = force_layout(&nodes, &[], &HashMap::new(), side);
    assert_eq!(pos.len(), nodes.len());
    for &a in &nodes {
        for &b in &nodes {
            if a == b {
                continue;
            }
            let (ax, ay) = pos[&a];
            let (bx, by) = pos[&b];
            assert!((ax - bx).hypot(ay - by) > 1.0, "{a} and {b} ended up on top of each other");
        }
    }
    for &p in pos.values() {
        assert!((MARGIN..=side + MARGIN).contains(&p.0), "{p:?} outside the area");
        assert!((MARGIN..=side + MARGIN).contains(&p.1), "{p:?} outside the area");
    }
}

/// A node cabled to the middle of a line ends up closer to its neighbors than to the line's far
/// end, several hops away: attraction along edges, not just repulsion.
#[test]
fn connected_nodes_end_up_closer_than_distant_ones() {
    let nodes = [1, 2, 3, 4, 5];
    let edges = [(1, 2), (2, 3), (3, 4), (4, 5)];
    let side = area_side(nodes.len());
    let pos = force_layout(&nodes, &edges, &HashMap::new(), side);
    let dist = |a: i64, b: i64| {
        let (ax, ay) = pos[&a];
        let (bx, by) = pos[&b];
        (ax - bx).hypot(ay - by)
    };
    assert!(dist(2, 3) < dist(1, 5), "neighbors 2-3 should be closer than the line's two ends 1-5");
}

/// A pinned node stays exactly where it's pinned, with or without edges to it.
#[test]
fn a_pinned_node_does_not_move() {
    let nodes = [1, 2, 3];
    let edges = [(1, 2), (2, 3)];
    let side = area_side(nodes.len());
    let pinned = HashMap::from([(2, (side / 2.0, side / 2.0))]);
    let pos = force_layout(&nodes, &edges, &pinned, side);
    assert_eq!(pos[&2], (side / 2.0 + MARGIN, side / 2.0 + MARGIN));
}

/// Every node pinned: nothing left to move, the layout is exactly the pinned positions.
#[test]
fn every_node_pinned_is_unchanged() {
    let nodes = [1, 2];
    let edges = [(1, 2)];
    let side = area_side(nodes.len());
    let pinned = HashMap::from([(1, (10.0, 10.0)), (2, (200.0, 200.0))]);
    let pos = force_layout(&nodes, &edges, &pinned, side);
    assert_eq!(pos[&1], (10.0 + MARGIN, 10.0 + MARGIN));
    assert_eq!(pos[&2], (200.0 + MARGIN, 200.0 + MARGIN));
}

/// Same graph, same result every time: no randomness, `seeded` only.
#[test]
fn the_layout_is_deterministic() {
    let nodes = [10, 20, 30, 40];
    let edges = [(10, 20), (20, 30), (30, 40), (40, 10)];
    let side = area_side(nodes.len());
    let a = force_layout(&nodes, &edges, &HashMap::new(), side);
    let b = force_layout(&nodes, &edges, &HashMap::new(), side);
    assert_eq!(a, b);
}

/// `graph` adds the outside node only when there are routers outside NetBox, cabled to the
/// devices that border them.
#[test]
fn the_outside_node_is_only_added_when_there_is_one() {
    let none = State { read_from: None, age: 0, error: None, nodes: vec![], edges: vec![], outside: 0, outside_at: vec![] };
    let (nodes, edges) = graph(&none);
    assert!(!nodes.contains(&OUTSIDE));
    assert!(edges.is_empty());

    let some = State {
        outside: 3,
        outside_at: vec![42],
        ..none
    };
    let (nodes, edges) = graph(&some);
    assert!(nodes.contains(&OUTSIDE));
    assert_eq!(edges, [(OUTSIDE, 42)]);
}
