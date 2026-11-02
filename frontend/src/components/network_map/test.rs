//! `force_layout` is continuous, not exact, so these check properties (pinned nodes don't move,
//! positions stay in bounds, connected nodes end up closer than unconnected ones), not exact
//! coordinates.

use super::*;

/// Two edges crossing like a square's diagonals: swapping either diagonal's endpoints with the
/// other's untangles them into two non-crossing sides, which the local search should find.
#[test]
fn reduce_crossings_untangles_two_crossing_edges() {
    let mut positions = HashMap::from([
        (Cluster::Site(1), (0.0, 0.0)),
        (Cluster::Site(2), (10.0, 10.0)),
        (Cluster::Site(3), (10.0, 0.0)),
        (Cluster::Site(4), (0.0, 10.0)),
    ]);
    let edges = [(Cluster::Site(1), Cluster::Site(2)), (Cluster::Site(3), Cluster::Site(4))];
    assert_eq!(count_crossings(&positions, &edges), 1, "the two diagonals should start out crossing");
    reduce_crossings(&mut positions, &edges);
    assert_eq!(count_crossings(&positions, &edges), 0, "a swap untangling them exists and should have been found");
}

/// No edges, several nodes: repulsion alone keeps them apart (not all collapsed to one point)
/// and inside the area.
#[test]
fn unconnected_nodes_spread_out_within_bounds() {
    let nodes = [1, 2, 3, 4, 5];
    let side = area_side(nodes.len());
    let pos = force_layout(&nodes, &[], &HashMap::new(), &HashMap::new(), 0.0, side);
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
    let pos = force_layout(&nodes, &edges, &HashMap::new(), &HashMap::new(), 0.0, side);
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
    let pos = force_layout(&nodes, &edges, &pinned, &HashMap::new(), 0.0, side);
    assert_eq!(pos[&2], (side / 2.0 + MARGIN, side / 2.0 + MARGIN));
}

/// Every node pinned: nothing left to move, the layout is exactly the pinned positions.
#[test]
fn every_node_pinned_is_unchanged() {
    let nodes = [1, 2];
    let edges = [(1, 2)];
    let side = area_side(nodes.len());
    let pinned = HashMap::from([(1, (10.0, 10.0)), (2, (200.0, 200.0))]);
    let pos = force_layout(&nodes, &edges, &pinned, &HashMap::new(), 0.0, side);
    assert_eq!(pos[&1], (10.0 + MARGIN, 10.0 + MARGIN));
    assert_eq!(pos[&2], (200.0 + MARGIN, 200.0 + MARGIN));
}

/// Same graph, same result every time: no randomness, `seeded` only.
#[test]
fn the_layout_is_deterministic() {
    let nodes = [10, 20, 30, 40];
    let edges = [(10, 20), (20, 30), (30, 40), (40, 10)];
    let side = area_side(nodes.len());
    let a = force_layout(&nodes, &edges, &HashMap::new(), &HashMap::new(), 0.0, side);
    let b = force_layout(&nodes, &edges, &HashMap::new(), &HashMap::new(), 0.0, side);
    assert_eq!(a, b);
}

fn node(id: i64, site: Option<i64>, site_name: Option<&str>) -> network_map::NetworkMapTopologyNetworkStateNodes {
    network_map::NetworkMapTopologyNetworkStateNodes {
        device: network_map::NetworkMapTopologyNetworkStateNodesDevice { id, name: id.to_string(), site },
        routeros: true,
        state: NodeState::REACHED,
        site_name: site_name.map(str::to_string),
        position: None,
    }
}

fn edge(a: i64, b: i64) -> network_map::NetworkMapTopologyNetworkStateEdges {
    network_map::NetworkMapTopologyNetworkStateEdges {
        kind: MapEdgeKind::CABLE,
        a: network_map::NetworkMapTopologyNetworkStateEdgesA {
            id: a,
            name: "a".into(),
            device: network_map::NetworkMapTopologyNetworkStateEdgesADevice { id: a, name: "a".into() },
        },
        b: network_map::NetworkMapTopologyNetworkStateEdgesB {
            id: b,
            name: "b".into(),
            device: network_map::NetworkMapTopologyNetworkStateEdgesBDevice { id: b, name: "b".into() },
        },
        state: MapEdgeState::UP,
        reason: None,
        reason_device: None,
    }
}

/// Many sites with few devices each, densely cross-cabled (the shape that overlapped before
/// `force_layout`'s meta pass became radius-aware): no two site circles overlap.
#[test]
fn many_small_sites_dont_overlap() {
    let sites = 8;
    let nodes = (0..sites).map(|s| node(s, Some(s), Some(&s.to_string()))).collect();
    // every site cabled to the next two (wrapping), like several real neighbors each
    let edges = (0..sites).flat_map(|s| [edge(s, (s + 1) % sites), edge(s, (s + 2) % sites)]).collect();
    let state = State { read_from: None, age: 0, error: None, outside: 0, outside_at: vec![], nodes, edges };
    let (_, circles) = clustered_layout(&state);
    assert_eq!(circles.len(), sites as usize);
    for i in 0..circles.len() {
        for j in (i + 1)..circles.len() {
            let dist = (circles[i].center.0 - circles[j].center.0).hypot(circles[i].center.1 - circles[j].center.1);
            assert!(
                dist >= circles[i].radius + circles[j].radius,
                "{} and {} overlap: centers {:?}/{:?}, radii {}/{}, dist {dist}",
                circles[i].name,
                circles[j].name,
                circles[i].center,
                circles[j].center,
                circles[i].radius,
                circles[j].radius
            );
        }
    }
}

/// Three devices of one site, equally cabled to each other (so nothing inside the site favors
/// one of them), only one also cabled to a device of another site: that one ends up closer to
/// the other site's device than its two uncabled siblings, pulled toward it.
#[test]
fn a_device_with_an_external_edge_is_pulled_toward_it() {
    let state = State {
        read_from: None,
        age: 0,
        error: None,
        outside: 0,
        outside_at: vec![],
        nodes: vec![
            node(1, Some(10), Some("A")),
            node(2, Some(10), Some("A")),
            node(3, Some(10), Some("A")),
            node(4, Some(20), Some("B")),
        ],
        edges: vec![edge(1, 2), edge(2, 3), edge(1, 3), edge(1, 4)],
    };
    let (positions, _) = clustered_layout(&state);
    let dist = |a: i64, b: i64| {
        let (ax, ay) = positions[&a];
        let (bx, by) = positions[&b];
        (ax - bx).hypot(ay - by)
    };
    assert!(dist(1, 4) < dist(2, 4), "1, cabled to 4, should end up closer to it than 2, which isn't");
    assert!(dist(1, 4) < dist(3, 4), "1, cabled to 4, should end up closer to it than 3, which isn't");
}

/// Two sites, each two devices cabled to each other, and one cable crossing the sites: one
/// circle per site, named after it, and every device ends up within its own site's circle.
#[test]
fn two_sites_get_their_own_circle() {
    let state = State {
        read_from: None,
        age: 0,
        error: None,
        outside: 0,
        outside_at: vec![],
        nodes: vec![node(1, Some(10), Some("A")), node(2, Some(10), Some("A")), node(3, Some(20), Some("B")), node(4, Some(20), Some("B"))],
        edges: vec![],
    };
    let (positions, circles) = clustered_layout(&state);
    assert_eq!(positions.len(), 4);
    assert_eq!(circles.len(), 2);
    let names = circles.iter().map(|c| c.name.as_str()).collect::<Vec<_>>();
    assert!(names.contains(&"A") && names.contains(&"B"));
    for circle in &circles {
        let members: &[i64] = if circle.name == "A" { &[1, 2] } else { &[3, 4] };
        for &id in members {
            let (x, y) = positions[&id];
            let dist = (x - circle.center.0).hypot(y - circle.center.1);
            assert!(dist <= circle.radius, "{id} at {:?} is outside its own site's circle (center {:?}, r {})", (x, y), circle.center, circle.radius);
        }
    }
}

/// A device without a site (and the outside node, `OUTSIDE` itself siteless) gets no circle, but
/// is still placed.
#[test]
fn a_device_without_a_site_gets_no_circle() {
    let state = State {
        read_from: None,
        age: 0,
        error: None,
        outside: 1,
        outside_at: vec![1],
        nodes: vec![node(1, Some(10), Some("A")), node(2, Some(10), Some("A")), node(3, None, None)],
        edges: vec![],
    };
    let (positions, circles) = clustered_layout(&state);
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].name, "A");
    assert!(positions.contains_key(&3), "the siteless device should still be placed");
    assert!(positions.contains_key(&OUTSIDE), "the outside node should still be placed");
}

/// A site with one device still gets a circle, centered on that device.
#[test]
fn a_lone_device_at_a_site_still_gets_a_circle() {
    let state = State {
        read_from: None,
        age: 0,
        error: None,
        outside: 0,
        outside_at: vec![],
        nodes: vec![node(1, Some(10), Some("A")), node(2, Some(20), Some("B"))],
        edges: vec![],
    };
    let (positions, circles) = clustered_layout(&state);
    assert_eq!(circles.len(), 2);
    for circle in &circles {
        let id = if circle.name == "A" { 1 } else { 2 };
        assert_eq!(positions[&id], circle.center);
        assert!(circle.radius >= SITE_MIN_RADIUS);
    }
}

/// Same graph, same clusters and positions every time.
#[test]
fn clustered_layout_is_deterministic() {
    let state = State {
        read_from: None,
        age: 0,
        error: None,
        outside: 1,
        outside_at: vec![1],
        nodes: vec![node(1, Some(10), Some("A")), node(2, Some(10), Some("A")), node(3, Some(20), Some("B"))],
        edges: vec![],
    };
    let (pos_a, circ_a) = clustered_layout(&state);
    let (pos_b, circ_b) = clustered_layout(&state);
    assert_eq!(pos_a, pos_b);
    assert_eq!(circ_a.iter().map(|c| (c.name.clone(), c.center, c.radius)).collect::<Vec<_>>(), circ_b.iter().map(|c| (c.name.clone(), c.center, c.radius)).collect::<Vec<_>>());
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
