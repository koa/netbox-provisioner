//! Scope: Tests of the network map's layout; nothing else.
//!
//! `force_layout` is continuous, not exact, so these check properties (pinned nodes don't move,
//! positions stay in bounds, connected nodes end up closer than unconnected ones), not exact
//! coordinates.

use super::*;
use crate::graphql::authenticated::{
    network_map::{MapEdge, MapNode},
    types::{DeviceRef, InterfaceRef},
};
use std::collections::BTreeMap;

fn point(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn line<K>(a: K, b: K) -> Edge<K> {
    Edge { a, b }
}

fn distance(p: Point, q: Point) -> f64 {
    (p.x - q.x).hypot(p.y - q.y)
}

/// Two edges crossing like a square's diagonals: swapping either diagonal's endpoints with the
/// other's untangles them into two non-crossing sides, which the local search should find.
#[test]
fn reduce_crossings_untangles_two_crossing_edges() {
    let mut positions = HashMap::from([
        (Cluster::Site(1), point(0.0, 0.0)),
        (Cluster::Site(2), point(10.0, 10.0)),
        (Cluster::Site(3), point(10.0, 0.0)),
        (Cluster::Site(4), point(0.0, 10.0)),
    ]);
    let edges = [
        line(Cluster::Site(1), Cluster::Site(2)),
        line(Cluster::Site(3), Cluster::Site(4)),
    ];
    assert_eq!(
        count_crossings(&positions, &edges),
        1,
        "the two diagonals should start out crossing"
    );
    reduce_crossings(&mut positions, &edges);
    assert_eq!(
        count_crossings(&positions, &edges),
        0,
        "a swap untangling them exists and should have been found"
    );
}

/// Three sites in a row, L - M - R: a device of L and a device of M are each cabled to one of
/// R's two devices — but to the "wrong" one of the two for their own position, so the lines
/// cross on the way, same shape as the real bug (two different sites' edges converging on a
/// third site's circle, crossing each other, which neither `reduce_crossings` on the collapsed
/// meta graph nor the internal-only per-site search can see, since neither ever looks at both
/// edges together). Swapping M and R (putting the busy site at the end of the row, not pinned
/// between the two others) removes the crossing; no single-site move could.
#[test]
fn reduce_site_crossings_untangles_a_badly_ordered_site() {
    let (l, m, r) = (Cluster::Site(1), Cluster::Site(2), Cluster::Site(3));
    let mut centers = HashMap::from([
        (l, point(0.0, 0.0)),
        (m, point(200.0, 0.0)),
        (r, point(400.0, 0.0)),
    ]);
    let cluster_of_node = HashMap::from([(10, l), (20, m), (30, r), (31, r)]);
    let members = BTreeMap::from([(l, vec![10]), (m, vec![20]), (r, vec![30, 31])]);
    let mut positions = HashMap::from([
        (10, point(0.0, -50.0)),
        (20, point(200.0, 50.0)),
        (30, point(400.0, 50.0)),
        (31, point(400.0, -50.0)),
    ]);
    let edges = [line(10, 30), line(20, 31)];
    assert_eq!(
        count_crossings(&positions, &edges),
        1,
        "10-30 and 20-31 should start out crossing, same shape as the real bug"
    );
    reduce_site_crossings(
        &mut positions,
        &mut centers,
        &cluster_of_node,
        &members,
        &edges,
        &HashMap::new(),
    );
    assert_eq!(
        count_crossings(&positions, &edges),
        0,
        "a site swap untangling them exists (M and R) and should have been found"
    );
}

/// Three sites in a row, L - M - R: a device of L is cabled directly to a device of R, a straight
/// line passing right through M's own circle in between — not a crossing (there's only the one
/// edge, nothing for it to cross), but just as confusing, reading as if it touches M too. Moving M
/// out of the way (a swap with either neighbor) removes that.
#[test]
fn reduce_site_crossings_moves_a_site_out_of_a_through_edges_way() {
    let (l, m, r) = (Cluster::Site(1), Cluster::Site(2), Cluster::Site(3));
    let mut centers = HashMap::from([
        (l, point(0.0, 0.0)),
        (m, point(200.0, 0.0)),
        (r, point(400.0, 0.0)),
    ]);
    let cluster_of_node = HashMap::from([(10, l), (30, r)]);
    let members = BTreeMap::from([(l, vec![10]), (m, vec![]), (r, vec![30])]);
    let mut positions = HashMap::from([(10, point(0.0, 0.0)), (30, point(400.0, 0.0))]);
    let edges = [line(10, 30)];
    let radii = HashMap::from([(l, 15.0), (m, 20.0), (r, 15.0)]);
    assert!(
        segment_intersects_circle(positions[&10], positions[&30], centers[&m], radii[&m]),
        "10-30 should start out passing right through M, same shape as the real bug"
    );
    reduce_site_crossings(
        &mut positions,
        &mut centers,
        &cluster_of_node,
        &members,
        &edges,
        &radii,
    );
    assert_eq!(
        count_intrusions(&positions, &edges, &cluster_of_node, &centers, &radii),
        0,
        "a site swap moving M out of the edge's way exists and should have been found"
    );
}

/// A chain of 4 sites, each cabled only to its next neighbor: seeded one layer per hop, so each
/// directly connected pair starts out exactly one step apart — the head start
/// [`clustered_layout`]'s meta pass needs so a connected pair starts adjacent instead of leaving
/// the physics to find its own way there (and risk a worse local optimum, like the real bug this
/// whole file is about).
#[test]
fn seed_from_graph_lays_out_a_chain_in_order() {
    let nodes = [1, 2, 3, 4];
    let edges = [line(1, 2), line(2, 3), line(3, 4)];
    let side = area_side(nodes.len());
    let seed = seed_from_graph(&nodes, &edges, side);
    assert_eq!(seed.len(), 4);
    let depth = |id: i32| seed[&id].y;
    assert!(
        depth(1) < depth(2) && depth(2) < depth(3) && depth(3) < depth(4),
        "a chain should be seeded in depth order: {seed:?}"
    );
    let step = depth(2) - depth(1);
    assert!(
        step > 0.0,
        "consecutive sites should land in different layers"
    );
    assert!(
        (depth(3) - depth(2) - step).abs() < 1e-9,
        "every hop should be the same one layer, not some skipped"
    );
    assert!(
        (depth(4) - depth(3) - step).abs() < 1e-9,
        "every hop should be the same one layer, not some skipped"
    );
}

/// Two separate components (no edge between them at all): neither's nodes land on top of the
/// other's.
#[test]
fn seed_from_graph_separates_disconnected_components() {
    let nodes = [1, 2, 3, 4];
    let edges = [line(1, 2), line(3, 4)];
    let side = area_side(nodes.len());
    let seed = seed_from_graph(&nodes, &edges, side);
    for &a in &[1, 2] {
        for &b in &[3, 4] {
            assert!(
                distance(seed[&a], seed[&b]) > 1.0,
                "{a} and {b}, in different components, ended up on top of each other"
            );
        }
    }
}

/// No edges, several nodes: repulsion alone keeps them apart (not all collapsed to one point)
/// and inside the area.
#[test]
fn unconnected_nodes_spread_out_within_bounds() {
    let nodes = [1, 2, 3, 4, 5];
    let side = area_side(nodes.len());
    let pos = force_layout(
        &nodes,
        &[],
        &HashMap::new(),
        &HashMap::new(),
        0.0,
        side,
        &HashMap::new(),
    );
    assert_eq!(pos.len(), nodes.len());
    for &a in &nodes {
        for &b in &nodes {
            if a == b {
                continue;
            }
            assert!(
                distance(pos[&a], pos[&b]) > 1.0,
                "{a} and {b} ended up on top of each other"
            );
        }
    }
    for &p in pos.values() {
        assert!(
            (MARGIN..=side + MARGIN).contains(&p.x),
            "{p:?} outside the area"
        );
        assert!(
            (MARGIN..=side + MARGIN).contains(&p.y),
            "{p:?} outside the area"
        );
    }
}

/// A node cabled to the middle of a line ends up closer to its neighbors than to the line's far
/// end, several hops away: attraction along edges, not just repulsion.
#[test]
fn connected_nodes_end_up_closer_than_distant_ones() {
    let nodes = [1, 2, 3, 4, 5];
    let edges = [line(1, 2), line(2, 3), line(3, 4), line(4, 5)];
    let side = area_side(nodes.len());
    let pos = force_layout(
        &nodes,
        &edges,
        &HashMap::new(),
        &HashMap::new(),
        0.0,
        side,
        &HashMap::new(),
    );
    let dist = |a: i32, b: i32| distance(pos[&a], pos[&b]);
    assert!(
        dist(2, 3) < dist(1, 5),
        "neighbors 2-3 should be closer than the line's two ends 1-5"
    );
}

/// A pinned node stays exactly where it's pinned, with or without edges to it.
#[test]
fn a_pinned_node_does_not_move() {
    let nodes = [1, 2, 3];
    let edges = [line(1, 2), line(2, 3)];
    let side = area_side(nodes.len());
    let pinned = HashMap::from([(2, point(side / 2.0, side / 2.0))]);
    let pos = force_layout(
        &nodes,
        &edges,
        &pinned,
        &HashMap::new(),
        0.0,
        side,
        &HashMap::new(),
    );
    assert_eq!(pos[&2], point(side / 2.0 + MARGIN, side / 2.0 + MARGIN));
}

/// Every node pinned: nothing left to move, the layout is exactly the pinned positions.
#[test]
fn every_node_pinned_is_unchanged() {
    let nodes = [1, 2];
    let edges = [line(1, 2)];
    let side = area_side(nodes.len());
    let pinned = HashMap::from([(1, point(10.0, 10.0)), (2, point(200.0, 200.0))]);
    let pos = force_layout(
        &nodes,
        &edges,
        &pinned,
        &HashMap::new(),
        0.0,
        side,
        &HashMap::new(),
    );
    assert_eq!(pos[&1], point(10.0 + MARGIN, 10.0 + MARGIN));
    assert_eq!(pos[&2], point(200.0 + MARGIN, 200.0 + MARGIN));
}

/// Same graph, same result every time: no randomness, `seeded` only.
#[test]
fn the_layout_is_deterministic() {
    let nodes = [10, 20, 30, 40];
    let edges = [line(10, 20), line(20, 30), line(30, 40), line(40, 10)];
    let side = area_side(nodes.len());
    let a = force_layout(
        &nodes,
        &edges,
        &HashMap::new(),
        &HashMap::new(),
        0.0,
        side,
        &HashMap::new(),
    );
    let b = force_layout(
        &nodes,
        &edges,
        &HashMap::new(),
        &HashMap::new(),
        0.0,
        side,
        &HashMap::new(),
    );
    assert_eq!(a, b);
}

fn node(id: i32, site: Option<i32>, site_name: Option<&str>) -> MapNode {
    MapNode {
        device: DeviceRef {
            id,
            name: id.to_string(),
            site,
        },
        routeros: true,
        state: NodeState::Reached,
        site_name: site_name.map(str::to_string),
        position: None,
    }
}

fn edge(a: i32, b: i32) -> MapEdge {
    MapEdge {
        kind: MapEdgeKind::Cable,
        a: InterfaceRef {
            id: a,
            name: "a".into(),
            device: DeviceRef {
                id: a,
                name: "a".into(),
                site: None,
            },
        },
        b: InterfaceRef {
            id: b,
            name: "b".into(),
            device: DeviceRef {
                id: b,
                name: "b".into(),
                site: None,
            },
        },
        state: MapEdgeState::Up,
        reason: None,
        reason_device: None,
    }
}

/// Many sites with few devices each, densely cross-cabled (the shape that overlapped before
/// `force_layout`'s meta pass became radius-aware): no two site circles overlap.
#[test]
fn many_small_sites_dont_overlap() {
    let sites = 8;
    let nodes = (0..sites)
        .map(|s| node(s, Some(s), Some(&s.to_string())))
        .collect();
    // every site cabled to the next two (wrapping), like several real neighbors each
    let edges = (0..sites)
        .flat_map(|s| [edge(s, (s + 1) % sites), edge(s, (s + 2) % sites)])
        .collect();
    let state = State {
        read_from: None,
        age: 0,
        error: None,
        outside: 0,
        outside_at: vec![],
        nodes,
        edges,
    };
    let Clustered { circles, .. } = clustered_layout(&state, &HashMap::new());
    assert_eq!(circles.len(), sites as usize);
    for i in 0..circles.len() {
        for j in (i + 1)..circles.len() {
            let dist = distance(circles[i].center, circles[j].center);
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
    let Clustered { positions, .. } = clustered_layout(&state, &HashMap::new());
    let dist = |a: i32, b: i32| distance(positions[&a], positions[&b]);
    assert!(
        dist(1, 4) < dist(2, 4),
        "1, cabled to 4, should end up closer to it than 2, which isn't"
    );
    assert!(
        dist(1, 4) < dist(3, 4),
        "1, cabled to 4, should end up closer to it than 3, which isn't"
    );
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
        nodes: vec![
            node(1, Some(10), Some("A")),
            node(2, Some(10), Some("A")),
            node(3, Some(20), Some("B")),
            node(4, Some(20), Some("B")),
        ],
        edges: vec![],
    };
    let Clustered { positions, circles } = clustered_layout(&state, &HashMap::new());
    assert_eq!(positions.len(), 4);
    assert_eq!(circles.len(), 2);
    let names = circles.iter().map(|c| &*c.name).collect::<Vec<_>>();
    assert!(names.contains(&"A") && names.contains(&"B"));
    for circle in &circles {
        let members: &[i32] = if circle.name == "A".into() {
            &[1, 2]
        } else {
            &[3, 4]
        };
        for &id in members {
            let dist = distance(positions[&id], circle.center);
            assert!(
                dist <= circle.radius,
                "{id} at {:?} is outside its own site's circle (center {:?}, r {})",
                positions[&id],
                circle.center,
                circle.radius
            );
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
        nodes: vec![
            node(1, Some(10), Some("A")),
            node(2, Some(10), Some("A")),
            node(3, None, None),
        ],
        edges: vec![],
    };
    let Clustered { positions, circles } = clustered_layout(&state, &HashMap::new());
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].name, "A".into());
    assert!(
        positions.contains_key(&3),
        "the siteless device should still be placed"
    );
    assert!(
        positions.contains_key(&OUTSIDE),
        "the outside node should still be placed"
    );
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
    let Clustered { positions, circles } = clustered_layout(&state, &HashMap::new());
    assert_eq!(circles.len(), 2);
    for circle in &circles {
        let id = if circle.name == "A".into() { 1 } else { 2 };
        assert_eq!(positions[&id], circle.center);
        assert!(circle.radius >= SITE_MIN_RADIUS);
    }
}

/// Three devices of one site, all cabled to each other (so nothing favors one over another):
/// pinning one of them away from where it would otherwise land both pulls it noticeably closer
/// to the pin, and moves its two un-pinned siblings elsewhere than they'd land unpinned — they
/// react to the pin instead of it just being stamped on top afterwards.
#[test]
fn a_pinned_device_in_a_clustered_site_lets_others_react_around_it() {
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
        ],
        edges: vec![edge(1, 2), edge(2, 3), edge(1, 3)],
    };
    let Clustered {
        positions: unpinned,
        ..
    } = clustered_layout(&state, &HashMap::new());
    let desired = point(unpinned[&1].x + 500.0, unpinned[&1].y + 500.0);
    let Clustered {
        positions: pinned, ..
    } = clustered_layout(&state, &HashMap::from([(1, desired)]));
    assert!(
        distance(pinned[&1], desired) < distance(unpinned[&1], desired),
        "pinning 1 should pull it noticeably closer to the desired spot: unpinned {:?}, pinned {:?}, desired {:?}",
        unpinned[&1],
        pinned[&1],
        desired
    );
    assert!(
        distance(pinned[&2], unpinned[&2]) > 1.0 || distance(pinned[&3], unpinned[&3]) > 1.0,
        "2 or 3 should have moved in reaction to 1 being pinned, not stayed exactly where they'd be unpinned"
    );
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
        nodes: vec![
            node(1, Some(10), Some("A")),
            node(2, Some(10), Some("A")),
            node(3, Some(20), Some("B")),
        ],
        edges: vec![],
    };
    let Clustered {
        positions: pos_a,
        circles: circ_a,
    } = clustered_layout(&state, &HashMap::new());
    let Clustered {
        positions: pos_b,
        circles: circ_b,
    } = clustered_layout(&state, &HashMap::new());
    assert_eq!(pos_a, pos_b);
    assert_eq!(
        circ_a
            .iter()
            .map(|c| (c.name.clone(), c.center, c.radius))
            .collect::<Vec<_>>(),
        circ_b
            .iter()
            .map(|c| (c.name.clone(), c.center, c.radius))
            .collect::<Vec<_>>()
    );
}

/// `graph` adds the outside node only when there are routers outside NetBox, cabled to the
/// devices that border them.
#[test]
fn the_outside_node_is_only_added_when_there_is_one() {
    let none = State {
        read_from: None,
        age: 0,
        error: None,
        nodes: vec![],
        edges: vec![],
        outside: 0,
        outside_at: vec![],
    };
    let Graph { nodes, edges } = graph(&none);
    assert!(!nodes.contains(&OUTSIDE));
    assert!(edges.is_empty());

    let some = State {
        outside: 3,
        outside_at: vec![42],
        ..none
    };
    let Graph { nodes, edges } = graph(&some);
    assert!(nodes.contains(&OUTSIDE));
    assert_eq!(*edges, [line(OUTSIDE, 42)]);
}

/// The map's graph of the stored export (`docs/netbox-response-example.json`: the devices with
/// OSPF and their cables and radio links, as `network_state::graphql::build` gives them without
/// an LSDB), as (device, site) and edges, numbered anew without names. Site 1 is Berg 1654,
/// with five edges leaving it (E21).
const EXPORT_NODES: &[(i32, i32)] = &[
    (1, 1),
    (2, 1),
    (3, 1),
    (4, 1),
    (5, 1),
    (6, 1),
    (7, 1),
    (8, 1),
    (9, 1),
    (10, 1),
    (11, 1),
    (12, 2),
    (13, 2),
    (14, 2),
    (15, 2),
    (16, 2),
    (17, 3),
    (18, 3),
    (19, 3),
    (20, 3),
    (21, 4),
    (22, 4),
    (23, 4),
    (24, 4),
    (25, 5),
    (26, 5),
    (27, 5),
    (28, 5),
    (29, 6),
    (30, 6),
    (31, 6),
    (32, 7),
    (33, 7),
    (34, 7),
    (35, 8),
    (36, 8),
    (37, 8),
    (38, 9),
    (39, 9),
    (40, 10),
    (41, 10),
    (42, 11),
    (43, 12),
];
const EXPORT_EDGES: &[(i32, i32)] = &[
    (1, 2),
    (1, 6),
    (1, 9),
    (1, 21),
    (2, 3),
    (2, 11),
    (2, 33),
    (3, 4),
    (3, 8),
    (5, 6),
    (5, 8),
    (5, 10),
    (5, 32),
    (5, 42),
    (6, 7),
    (8, 22),
    (12, 13),
    (12, 20),
    (13, 14),
    (13, 15),
    (13, 16),
    (14, 25),
    (15, 36),
    (17, 18),
    (17, 19),
    (17, 20),
    (18, 31),
    (19, 33),
    (21, 22),
    (22, 23),
    (22, 24),
    (25, 26),
    (26, 27),
    (26, 28),
    (28, 39),
    (29, 30),
    (30, 31),
    (32, 34),
    (35, 36),
    (35, 37),
    (35, 43),
    (38, 39),
    (40, 41),
];

/// How badly a layout draws: edges crossing each other, edges passing through a device that
/// isn't one of their ends (closer than its symbol), and edges through a site circle neither
/// end belongs to.
#[derive(Debug)]
struct Faults {
    crossings: usize,
    through_devices: usize,
    through_circles: usize,
}

fn faults(state: &State, positions: &HashMap<i32, Point>, circles: &[SiteCircle]) -> Faults {
    let edges: Vec<Edge<i32>> = state
        .edges
        .iter()
        .map(|e| line(e.a.device.id, e.b.device.id))
        .collect();
    let through_devices = edges
        .iter()
        .map(|&Edge { a, b }| {
            positions
                .iter()
                .filter(|&(&id, &p)| {
                    id != a
                        && id != b
                        && segment_intersects_circle(positions[&a], positions[&b], p, 16.0)
                })
                .count()
        })
        .sum();
    let cluster_of_node = state
        .nodes
        .iter()
        .map(|n| {
            let id = n.device.id;
            (id, n.device.site.map_or(Cluster::Alone(id), Cluster::Site))
        })
        .collect();
    let centers = circles
        .iter()
        .map(|c| (Cluster::Site(c.site), c.center))
        .collect();
    let radii = circles
        .iter()
        .map(|c| (Cluster::Site(c.site), c.radius))
        .collect();
    Faults {
        crossings: count_crossings(positions, &edges),
        through_devices,
        through_circles: count_intrusions(positions, &edges, &cluster_of_node, &centers, &radii),
    }
}

fn export_state() -> State {
    State {
        read_from: None,
        age: 0,
        error: None,
        outside: 0,
        outside_at: vec![],
        nodes: EXPORT_NODES
            .iter()
            .map(|&(id, site)| node(id, Some(site), Some(&format!("site {site}"))))
            .collect(),
        edges: EXPORT_EDGES.iter().map(|&(a, b)| edge(a, b)).collect(),
    }
}

/// The measure of E21 on the stored export's map: what the layout leaves at most. Before E21
/// 4 crossings and 3 edges through a device (two lines leaving Berg 1654 across it); devices
/// cabled outside at the rim, the sites laid out again after their swaps and the last swaps on
/// the real edges leave one crossing inside Berg 1654.
#[test]
fn the_exports_map_draws_with_few_faults() {
    let state = export_state();
    let Clustered { positions, circles } = clustered_layout(&state, &HashMap::new());
    let faults = faults(&state, &positions, &circles);
    assert!(faults.crossings <= 1, "{faults:?}");
    assert_eq!(
        (faults.through_devices, faults.through_circles),
        (0, 0),
        "{faults:?}"
    );
}
