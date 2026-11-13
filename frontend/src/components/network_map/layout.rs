//! Scope: Where the network map puts its nodes; not drawing them (`network_map`).
//!
//! The layout of the network map: a force-directed layout (Fruchterman–Reingold), and over
//! several sites one circle per site laid out against the others, with fewer crossings.
use super::{MARGIN, OUTSIDE};
use crate::graphql::authenticated::network_map::NetworkState as State;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A position on the map (SVG space). Stored as `[x, y]` (a browser's own arrangement in
/// `localStorage` has it so).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(from = "[f64; 2]", into = "[f64; 2]")]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl From<[f64; 2]> for Point {
    fn from([x, y]: [f64; 2]) -> Self {
        Point { x, y }
    }
}

impl From<Point> for [f64; 2] {
    fn from(point: Point) -> Self {
        [point.x, point.y]
    }
}

/// A line between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Edge<K> {
    pub(super) a: K,
    pub(super) b: K,
}

impl<K: PartialEq> Edge<K> {
    fn touches(&self, node: K) -> bool {
        self.a == node || self.b == node
    }
}

/// The nodes and edges of a map ([`graph`]).
pub(super) struct Graph {
    pub(super) nodes: Vec<i32>,
    pub(super) edges: Vec<Edge<i32>>,
}

/// Where [`clustered_layout`] puts the devices, and the sites' circles.
pub(super) struct Clustered {
    pub(super) positions: HashMap<i32, Point>,
    pub(super) circles: Vec<SiteCircle>,
}

/// `state`'s nodes (plus the outside node, if any, pulled toward the routers it borders) and
/// edges, as plain ids, for [`force_layout`].
pub(super) fn graph(state: &State) -> Graph {
    let mut nodes: Vec<i32> = state.nodes.iter().map(|n| n.device.id).collect();
    let mut edges: Vec<Edge<i32>> = state
        .edges
        .iter()
        .map(|e| Edge {
            a: e.a.device.id,
            b: e.b.device.id,
        })
        .collect();
    if state.outside > 0 {
        nodes.push(OUTSIDE);
        edges.extend(
            state
                .outside_at
                .iter()
                .map(|&id| Edge { a: OUTSIDE, b: id }),
        );
    }
    Graph { nodes, edges }
}

/// A side length that gives `count` nodes roughly as much room as the old column layout did
/// (`COLUMN` × `ROW` per node, in a square-ish area) — big enough that `force_layout`'s
/// repulsion has space to spread nodes out, not a hard limit (nodes may end up outside it; the
/// SVG's `viewBox` is sized from the actual positions, not this).
pub(super) fn area_side(count: usize) -> f64 {
    (count.max(1) as f64 * 170.0 * 80.0).sqrt().max(300.0)
}

/// Fruchterman–Reingold: `nodes` repel each other (`k² / d`), `edges` pull their ends together
/// (`d² / k`), `k = √(area / n)`; a pinned node (the user dragged it, or it's NetBox's published
/// position, or, for [`clustered_layout`]'s per-site pass, a fixed anchor standing for the
/// direction of another site) doesn't move but still pushes and pulls on the others, so they
/// arrange themselves around it. `radii`/`gap` are for [`clustered_layout`]'s meta pass, where a
/// "node" is a whole site: repulsion uses `max(k, radius of a + radius of b + gap)` in place of
/// `k`, so two sites never end up overlapping regardless of `k`, while still being pulled as
/// close as that allows by their edges; empty `radii` and `gap` 0 (every other caller) is plain
/// Fruchterman–Reingold, `max(k, 0) == k`. Displacement per round is capped by a temperature
/// that cools linearly to 0 over `ITERATIONS` rounds, standard to keep the layout from
/// oscillating instead of settling. A node that's neither pinned nor in `seed` starts at a
/// position hashed from its id, not randomly, so the same graph lays out the same way every
/// time; `seed` (typically [`seed_from_graph`]) gives the physics a graph-structure-aware head
/// start instead, still free to move from there, unlike `pinned`.
pub(super) const ITERATIONS: u32 = 300;

pub(super) fn force_layout(
    nodes: &[i32],
    edges: &[Edge<i32>],
    pinned: &HashMap<i32, Point>,
    radii: &HashMap<i32, f64>,
    gap: f64,
    side: f64,
    seed: &HashMap<i32, Point>,
) -> HashMap<i32, Point> {
    if nodes.is_empty() {
        return HashMap::new();
    }
    let k = (side * side / nodes.len() as f64).sqrt();
    let mut pos: HashMap<i32, Point> = nodes
        .iter()
        .map(|&id| {
            (
                id,
                pinned
                    .get(&id)
                    .or_else(|| seed.get(&id))
                    .copied()
                    .unwrap_or_else(|| seeded(id, side)),
            )
        })
        .collect();
    for step in 0..ITERATIONS {
        let mut disp = HashMap::<i32, Point>::new();
        for (i, &a) in nodes.iter().enumerate() {
            for &b in &nodes[i + 1..] {
                let (dx, dy) = (pos[&a].x - pos[&b].x, pos[&a].y - pos[&b].y);
                let dist = dx.hypot(dy).max(0.01);
                let effective_k = (radii.get(&a).copied().unwrap_or(0.0)
                    + radii.get(&b).copied().unwrap_or(0.0)
                    + gap)
                    .max(k);
                let force = effective_k * effective_k / dist;
                let (ux, uy) = (dx / dist * force, dy / dist * force);
                let da = disp.entry(a).or_default();
                da.x += ux;
                da.y += uy;
                let db = disp.entry(b).or_default();
                db.x -= ux;
                db.y -= uy;
            }
        }
        for &Edge { a, b } in edges {
            let (Some(&pa), Some(&pb)) = (pos.get(&a), pos.get(&b)) else {
                continue;
            };
            let (dx, dy) = (pa.x - pb.x, pa.y - pb.y);
            let dist = dx.hypot(dy).max(0.01);
            let force = dist * dist / k;
            let (ux, uy) = (dx / dist * force, dy / dist * force);
            let da = disp.entry(a).or_default();
            da.x -= ux;
            da.y -= uy;
            let db = disp.entry(b).or_default();
            db.x += ux;
            db.y += uy;
        }
        let temperature = side * 0.1 * (1.0 - f64::from(step) / f64::from(ITERATIONS));
        for &id in nodes {
            if pinned.contains_key(&id) {
                continue;
            }
            let Point { x: dx, y: dy } = disp.get(&id).copied().unwrap_or_default();
            let dist = dx.hypot(dy).max(0.01);
            let limited = dist.min(temperature);
            let p = pos.get_mut(&id).expect("every node has a position");
            p.x = (p.x + dx / dist * limited).clamp(0.0, side);
            p.y = (p.y + dy / dist * limited).clamp(0.0, side);
        }
    }
    for p in pos.values_mut() {
        p.x += MARGIN;
        p.y += MARGIN;
    }
    pos
}

/// A position hashed from `id` (MurmurHash3's finalizer), not random, so a layout without a
/// pinned position for every node still comes out the same way every time.
pub(super) fn seeded(id: i32, side: f64) -> Point {
    let mut h = id as u64 ^ 0x9E37_79B9_7F4A_7C15;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    let x = (h & 0xFFFF_FFFF) as f64 / f64::from(u32::MAX);
    let y = (h >> 32) as f64 / f64::from(u32::MAX);
    Point {
        x: x * side,
        y: y * side,
    }
}

/// A graph-structure-aware starting layout for [`force_layout`]'s `seed`, used for
/// [`clustered_layout`]'s meta (site-to-site) pass: layered by BFS depth from each connected
/// component's own lowest-id node — a node's `y` is its depth, `x` spreads same-depth siblings
/// out in BFS order — so a directly connected pair starts out exactly one layer, and nothing
/// else, apart. The later force-refinement and [`reduce_site_crossings`] only have to preserve
/// that, not discover it from scratch; a hash-seeded random start, by contrast, can just as
/// easily begin two directly connected nodes on opposite sides of the whole area and leave the
/// physics to find its way back, sometimes into a local optimum neither pass can fully untangle
/// afterwards. Multiple components (no path between them at all) are laid out side by side, each
/// its own layered block. Not itself crossing-free for anything beyond a tree (a non-tree edge —
/// two nodes in the same or a cousin layer — can still need to travel past an unrelated node) —
/// only a head start for what follows, not a replacement for it.
pub(super) fn seed_from_graph(
    nodes: &[i32],
    edges: &[Edge<i32>],
    side: f64,
) -> HashMap<i32, Point> {
    let mut adjacency: HashMap<i32, Vec<i32>> = HashMap::new();
    for &Edge { a, b } in edges {
        adjacency.entry(a).or_default().push(b);
        adjacency.entry(b).or_default().push(a);
    }
    for neighbors in adjacency.values_mut() {
        neighbors.sort_unstable(); // deterministic BFS traversal order
    }
    let spacing = (side / (nodes.len() as f64).max(1.0).sqrt()).max(40.0);
    let mut sorted_nodes = nodes.to_vec();
    sorted_nodes.sort_unstable();
    let mut visited = HashSet::new();
    let mut pos = HashMap::new();
    let mut origin_x = 0.0;
    for &root in &sorted_nodes {
        if !visited.insert(root) {
            continue;
        }
        let mut layers: Vec<Vec<i32>> = vec![vec![root]];
        loop {
            let next: Vec<i32> = layers
                .last()
                .expect("just pushed a first layer, and only ever push another non-empty one")
                .iter()
                .flat_map(|id| adjacency.get(id).into_iter().flatten())
                .copied()
                .filter(|&id| visited.insert(id))
                .collect();
            if next.is_empty() {
                break;
            }
            layers.push(next);
        }
        let width = layers.iter().map(Vec::len).max().unwrap_or(1) as f64 * spacing;
        for (depth, layer) in layers.iter().enumerate() {
            for (i, &id) in layer.iter().enumerate() {
                let x = origin_x
                    + width / 2.0
                    + (i as f64 - (layer.len() as f64 - 1.0) / 2.0) * spacing;
                pos.insert(
                    id,
                    Point {
                        x,
                        y: depth as f64 * spacing,
                    },
                );
            }
        }
        origin_x += width + spacing;
    }
    pos
}

/// A node's site, for [`clustered_layout`]: `Site` for a device with one, `Alone` (keyed by the
/// node's own id, so two sideless nodes never share one) for the outside node or a device
/// without a site. `Ord` only so [`clustered_layout`] can put clusters in a `BTreeMap` and get
/// the same iteration order every time (a `HashMap`'s is randomized per instance, which would
/// make `force_layout`'s floating-point sums round differently from one poll to the next and
/// the whole map jitter, even with nothing changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Cluster {
    Site(i32),
    Alone(i32),
}

/// A site's devices, grouped into their own circle.
pub(super) struct SiteCircle {
    pub(super) site: i32,
    pub(super) name: String,
    pub(super) center: Point,
    pub(super) radius: f64,
}

pub(super) const SITE_PADDING: f64 = 35.0;
/// where a device cabled to another site sits, as part of its circle's radius
pub(super) const RIM: f64 = 0.95;
pub(super) const SITE_MIN_RADIUS: f64 = 50.0;
/// minimum gap [`force_layout`]'s meta pass keeps between two site circles, so they end up
/// visibly separate, not just touching
pub(super) const SITE_GAP: f64 = 25.0;

/// Whether segments `p1`-`p2` and `p3`-`p4` cross in their interior (touching at an endpoint,
/// or being collinear, doesn't count — good enough for a visual heuristic over lines that share
/// no endpoint, which is the only way [`reduce_crossings`] calls this).
pub(super) fn segments_cross(p1: Point, p2: Point, p3: Point, p4: Point) -> bool {
    fn orientation(p: Point, q: Point, r: Point) -> i8 {
        let val = (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y);
        if val.abs() < 1e-9 {
            0
        } else if val > 0.0 {
            1
        } else {
            -1
        }
    }
    let (o1, o2, o3, o4) = (
        orientation(p1, p2, p3),
        orientation(p1, p2, p4),
        orientation(p3, p4, p1),
        orientation(p3, p4, p2),
    );
    o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0 && o1 != o2 && o3 != o4
}

pub(super) fn count_crossings<K: Eq + std::hash::Hash + Copy>(
    positions: &HashMap<K, Point>,
    edges: &[Edge<K>],
) -> usize {
    let mut n = 0;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let Edge { a: a1, b: b1 } = edges[i];
            let Edge { a: a2, b: b2 } = edges[j];
            if a1 == a2 || a1 == b2 || b1 == a2 || b1 == b2 {
                continue; // sharing a site isn't a crossing, the lines are meant to meet there
            }
            if segments_cross(
                positions[&a1],
                positions[&b1],
                positions[&a2],
                positions[&b2],
            ) {
                n += 1;
            }
        }
    }
    n
}

/// 2-opt-style local search: try swapping two nodes' positions outright, keep the swap where it
/// reduces the number of edges crossing each other, repeat until nothing helps or a round limit
/// is hit (a pathological graph could otherwise keep trading one crossing for another forever).
/// Used both over the meta layout's site positions (one meta-edge per cross-site device edge)
/// and, within a site of more than a few devices, over its own devices' positions (the internal
/// edges that stay inside one circle) — the same local search either way, just at a different
/// scale and with different node ids. Position swaps, not nudges: a crossing is a discrete,
/// all-or-nothing thing an edge's endpoint moving a little practically never removes, unlike
/// edge length. Approximate — true crossing minimization is NP-hard — and only a well-known
/// heuristic for it, not a novel one; it only ever accepts a strict improvement, so it can only
/// make the force layout's drawing better, never worse, though a swap may lengthen some edges to
/// remove a crossing, and at the site level a swap's effect on radius-aware repulsion's promise
/// of non-overlap is restored by the hard backstop in [`clustered_layout`] afterwards, not by
/// this (a device-level swap, within one circle's own local coordinates, needs no such backstop:
/// it only ever swaps two positions already inside the circle, so it can't push one out of it).
pub(super) fn reduce_crossings<K: Eq + std::hash::Hash + Ord + Copy>(
    positions: &mut HashMap<K, Point>,
    edges: &[Edge<K>],
) {
    let mut nodes: Vec<K> = positions.keys().copied().collect();
    nodes.sort(); // deterministic order: a HashMap's own would make the swap sequence, and so
    // the result, depend on this process's hashing, not just the graph (see `force_layout`'s own
    // doc comment on `BTreeMap` for why that matters here)
    reduce_by_swaps(positions, &nodes, |positions| {
        count_crossings(positions, edges)
    });
}

/// The local search of [`reduce_crossings`] over `movable` (the others keep their place, an
/// anchor standing for another site's direction), keeping a swap where `badness` drops.
pub(super) fn reduce_by_swaps<K: Eq + std::hash::Hash + Copy>(
    positions: &mut HashMap<K, Point>,
    nodes: &[K],
    badness: impl Fn(&HashMap<K, Point>) -> usize,
) {
    for _ in 0..nodes.len().min(30) {
        let mut improved = false;
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let before = badness(positions);
                let (pi, pj) = (positions[&nodes[i]], positions[&nodes[j]]);
                positions.insert(nodes[i], pj);
                positions.insert(nodes[j], pi);
                if badness(positions) < before {
                    improved = true;
                } else {
                    positions.insert(nodes[i], pi);
                    positions.insert(nodes[j], pj);
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// How close an edge may pass a device it doesn't end at before it reads as going through it:
/// the symbol's radius (14) and a little.
pub(super) const DEVICE_CLEARANCE: f64 = 16.0;

/// How many times one of `edges` passes through one of `devices` it doesn't end at (closer than
/// [`DEVICE_CLEARANCE`]): it reads as connecting to that device too.
pub(super) fn count_through_devices<K: Eq + std::hash::Hash + Copy>(
    positions: &HashMap<K, Point>,
    edges: &[Edge<K>],
    devices: &[K],
) -> usize {
    edges
        .iter()
        .map(|&Edge { a, b }| {
            devices
                .iter()
                .filter(|&&id| {
                    id != a
                        && id != b
                        && segment_intersects_circle(
                            positions[&a],
                            positions[&b],
                            positions[&id],
                            DEVICE_CLEARANCE,
                        )
                })
                .count()
        })
        .sum()
}

/// The faults [`reduce_faults_within_sites`] weighs that involve the devices `at`: crossings of
/// an edge at one of them with any other edge (each pair once), edges at them passing through a
/// device, and other edges passing through one of them. A swap of two devices changes only
/// these, so comparing them before and after is the whole change.
fn faults_at(
    positions: &HashMap<i32, Point>,
    edges: &[Edge<i32>],
    devices: &[i32],
    at: [i32; 2],
) -> usize {
    let touches = |e: &Edge<i32>| at.contains(&e.a) || at.contains(&e.b);
    let mut n = 0;
    for (i, e) in edges.iter().enumerate() {
        if !touches(e) {
            continue;
        }
        for (j, f) in edges.iter().enumerate() {
            // a pair of edges both at `at` counts once
            if (touches(f) && j <= i) || f.touches(e.a) || f.touches(e.b) {
                continue;
            }
            if segments_cross(
                positions[&e.a],
                positions[&e.b],
                positions[&f.a],
                positions[&f.b],
            ) {
                n += 1;
            }
        }
    }
    n += count_through_devices(
        positions,
        &edges.iter().copied().filter(touches).collect::<Vec<_>>(),
        devices,
    );
    let others: Vec<Edge<i32>> = edges.iter().copied().filter(|e| !touches(e)).collect();
    n + count_through_devices(positions, &others, &at)
}

/// The last pass of [`clustered_layout`]: swaps two devices of a site where that lowers the
/// faults of the edges at them, at the final positions and with the real far ends. The search
/// within a site before only knows the direction of another site, so two lines to the same site
/// could still cross on the way, or one leave through a device (E21). A swap stays within a
/// circle, so it can't push a device out of it.
pub(super) fn reduce_faults_within_sites(
    positions: &mut HashMap<i32, Point>,
    edges: &[Edge<i32>],
    members: &[Vec<i32>],
) {
    let mut devices: Vec<i32> = positions.keys().copied().collect();
    devices.sort_unstable();
    for ids in members {
        let mut ids = ids.clone();
        ids.sort_unstable();
        for _ in 0..ids.len().min(30) {
            let mut improved = false;
            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    let at = [ids[i], ids[j]];
                    let before = faults_at(positions, edges, &devices, at);
                    let (pi, pj) = (positions[&at[0]], positions[&at[1]]);
                    positions.insert(at[0], pj);
                    positions.insert(at[1], pi);
                    if faults_at(positions, edges, &devices, at) < before {
                        improved = true;
                    } else {
                        positions.insert(at[0], pi);
                        positions.insert(at[1], pj);
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }
}

/// The point on segment `p1`-`p2` closest to `center` is nearer than `radius`: the segment passes
/// through the circle, not just near it — touching the boundary exactly (`==`) doesn't count, same
/// convention as [`segments_cross`] treating a shared endpoint as "meeting there," not a crossing.
pub(super) fn segment_intersects_circle(p1: Point, p2: Point, center: Point, radius: f64) -> bool {
    let (dx, dy) = (p2.x - p1.x, p2.y - p1.y);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((center.x - p1.x) * dx + (center.y - p1.y) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (p1.x + t * dx, p1.y + t * dy);
    (cx - center.x).hypot(cy - center.y) < radius
}

/// How many of `edges`, drawn at `positions`, pass through a site circle neither of their own two
/// endpoints belongs to — reads as if it connects to that site too, confusing regardless of
/// whether it also crosses another edge on the way, so [`reduce_site_crossings`] weighs it the
/// same as an actual crossing.
pub(super) fn count_intrusions(
    positions: &HashMap<i32, Point>,
    edges: &[Edge<i32>],
    cluster_of_node: &HashMap<i32, Cluster>,
    centers: &HashMap<Cluster, Point>,
    radii: &HashMap<Cluster, f64>,
) -> usize {
    let mut n = 0;
    for &Edge { a, b } in edges {
        let (ca, cb) = (cluster_of_node[&a], cluster_of_node[&b]);
        for (&site, &radius) in radii {
            if site == ca || site == cb {
                continue; // the edge is allowed to touch a circle it actually belongs to
            }
            if segment_intersects_circle(positions[&a], positions[&b], centers[&site], radius) {
                n += 1;
            }
        }
    }
    n
}

/// Like [`reduce_crossings`], but at the site level: swaps two entire sites' circles outright —
/// every member moving with its own circle, its offset from `centers` preserved — rather than one
/// device's own position. [`reduce_crossings`], run over the meta layout's one-meta-edge-per-
/// site-pair graph, can't find this kind of improvement: every real edge between two sites
/// collapses to an edge sharing that site's single meta-point, which `count_crossings` treats as
/// "meeting there," not a crossing — even when two such edges, arriving from different other
/// sites, land on the wrong side of that shared site's circle for each other (which device ends
/// up where within a circle is decided by the anchor-biased per-site layout, not by this), or an
/// edge between two OTHER sites happens to pass straight through a third site's circle on the way
/// ([`count_intrusions`]). A pass over the real, now-final, device-level edges and positions
/// catches both. Mutates `positions` (every member of a swapped site) and `centers` (that site's
/// own, so its circle follows); a swap between differently-sized circles can leave them
/// overlapping (this only moves centers, blind to radius) — left for the caller's own radius-aware
/// cleanup, same as any other cause of overlap.
pub(super) fn reduce_site_crossings(
    positions: &mut HashMap<i32, Point>,
    centers: &mut HashMap<Cluster, Point>,
    cluster_of_node: &HashMap<i32, Cluster>,
    members: &BTreeMap<Cluster, Vec<i32>>,
    edges: &[Edge<i32>],
    radii: &HashMap<Cluster, f64>,
) {
    let offsets: HashMap<i32, Point> = positions
        .iter()
        .map(|(&id, &p)| {
            let center = centers[&cluster_of_node[&id]];
            (
                id,
                Point {
                    x: p.x - center.x,
                    y: p.y - center.y,
                },
            )
        })
        .collect();
    let at = |center: Point, id: i32| Point {
        x: center.x + offsets[&id].x,
        y: center.y + offsets[&id].y,
    };
    let badness = |positions: &HashMap<i32, Point>, centers: &HashMap<Cluster, Point>| {
        count_crossings(positions, edges)
            + count_intrusions(positions, edges, cluster_of_node, centers, radii)
    };
    let mut site_clusters: Vec<Cluster> = members
        .keys()
        .copied()
        .filter(|c| matches!(c, Cluster::Site(_)))
        .collect();
    site_clusters.sort();
    for _ in 0..site_clusters.len().min(30) {
        let mut improved = false;
        for i in 0..site_clusters.len() {
            for j in (i + 1)..site_clusters.len() {
                let (ci, cj) = (site_clusters[i], site_clusters[j]);
                let (center_i, center_j) = (centers[&ci], centers[&cj]);
                let before = badness(positions, centers);
                for &id in &members[&ci] {
                    positions.insert(id, at(center_j, id));
                }
                for &id in &members[&cj] {
                    positions.insert(id, at(center_i, id));
                }
                centers.insert(ci, center_j);
                centers.insert(cj, center_i);
                if badness(positions, centers) < before {
                    improved = true;
                } else {
                    for &id in &members[&ci] {
                        positions.insert(id, at(center_i, id));
                    }
                    for &id in &members[&cj] {
                        positions.insert(id, at(center_j, id));
                    }
                    centers.insert(ci, center_i);
                    centers.insert(cj, center_j);
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Two-level layout for the all-sites map: a site's devices lay out among themselves as
/// [`force_layout`] always has, then the sites lay out against each other the same way (one
/// meta-node per site, one meta-edge per device edge crossing sites), and each site's devices
/// are shifted onto its meta position. The outside node, and a device without a site, each stay
/// their own single-node "site" in the meta layout (pulled toward what they're cabled to) but
/// get no circle. Edges between devices are drawn at the devices' own, now clustered, positions
/// — only where a node ends up, not what it's grouped under, changes.
pub(super) fn clustered_layout(state: &State, pinned: &HashMap<i32, Point>) -> Clustered {
    let Graph {
        nodes: node_ids,
        edges: edge_pairs,
    } = graph(state);
    if node_ids.is_empty() {
        return Clustered {
            positions: HashMap::new(),
            circles: Vec::new(),
        };
    }
    let site_of: HashMap<i32, Option<i32>> = state
        .nodes
        .iter()
        .map(|n| (n.device.id, n.device.site))
        .collect();
    let site_name_of: HashMap<i32, String> = state
        .nodes
        .iter()
        .filter_map(|n| Some((n.device.site?, n.site_name.clone()?)))
        .collect();
    let cluster_of = |id: i32| match site_of.get(&id).copied().flatten() {
        Some(site) => Cluster::Site(site),
        None => Cluster::Alone(id),
    };

    let mut groups = BTreeMap::<Cluster, Vec<i32>>::new();
    for &id in &node_ids {
        groups.entry(cluster_of(id)).or_default().push(id);
    }
    let cluster_of_node: HashMap<i32, Cluster> = groups
        .iter()
        .flat_map(|(&c, ids)| ids.iter().map(move |&id| (id, c)))
        .collect();

    // the meta layout: one node per cluster (force_layout's ids are plain i64s, so clusters get
    // synthetic ones by their position in the sorted, deterministic `clusters` list), sites
    // repelling each other by their estimated size (`radius_estimate`, before the real per-site
    // layout below — that would need to know where the site ends up first) so two sites can
    // never end up overlapping regardless of how strongly their edges pull them together, and
    // an edge between two sites is as short as that allows: unlike a plain point repulsion
    // where "no overlap" would have to be bolted on afterwards (pushing circles apart without
    // regard for their edges, undoing some of the force layout's own shortening), the two
    // objectives are balanced by the same simulation here.
    let clusters: Vec<Cluster> = groups.keys().copied().collect();
    let meta_id_of: HashMap<Cluster, i32> = clusters
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, i as i32))
        .collect();
    let meta_ids: Vec<i32> = (0..clusters.len() as i32).collect();
    let meta_edges: Vec<Edge<i32>> = edge_pairs
        .iter()
        .filter_map(|&Edge { a, b }| {
            let (ca, cb) = (cluster_of_node[&a], cluster_of_node[&b]);
            (ca != cb).then(|| Edge {
                a: meta_id_of[&ca],
                b: meta_id_of[&cb],
            })
        })
        .collect();
    // reused, unchanged, for both the meta layout's repulsion and the final circle below: the
    // meta layout must promise each site at least the radius it actually ends up drawn with,
    // or sites sized by one number and drawn by another could still end up overlapping despite
    // the radius-aware repulsion
    let radius_of = |cluster: Cluster| match cluster {
        Cluster::Alone(_) => 0.0,
        Cluster::Site(_) => (area_side(groups[&cluster].len()) * 0.5).max(SITE_MIN_RADIUS),
    };
    let meta_radii: HashMap<i32, f64> = clusters
        .iter()
        .map(|&c| (meta_id_of[&c], radius_of(c)))
        .collect();
    let meta_area: f64 = meta_radii
        .values()
        .map(|r| std::f64::consts::PI * r * r)
        .sum();
    let meta_side = (meta_area * 2.0).sqrt().max(300.0);
    let meta_seed = seed_from_graph(&meta_ids, &meta_edges, meta_side);
    let meta_positions_by_id = force_layout(
        &meta_ids,
        &meta_edges,
        &HashMap::new(),
        &meta_radii,
        SITE_GAP,
        meta_side,
        &meta_seed,
    );
    let mut meta_positions: HashMap<Cluster, Point> = clusters
        .iter()
        .map(|&c| (c, meta_positions_by_id[&meta_id_of[&c]]))
        .collect();
    reduce_crossings(
        &mut meta_positions,
        &meta_edges
            .iter()
            .map(|&Edge { a, b }| Edge {
                a: clusters[a as usize],
                b: clusters[b as usize],
            })
            .collect::<Vec<_>>(),
    );

    // each site's devices around its center, facing the sites they are cabled to
    let lay_out_sites = |meta_positions: &HashMap<Cluster, Point>| {
        let mut positions = HashMap::new();
        let mut circles = Vec::new();
        let mut circle_members: Vec<Vec<i32>> = Vec::new();
        for (&cluster, ids) in &groups {
            let center = meta_positions[&cluster];
            let promised_radius = radius_of(cluster);
            // the recenter+shrink step turns a local layout (around `local_center`, in `side`-sized
            // local coordinates) into the final, absolute one (around `center`, within
            // `promised_radius`): called once below for the plain case, and, where a device of this
            // site is pinned (dragged, live or settled), a second time so the others can be laid
            // out reacting to it rather than just having it stamped on top afterwards.
            let recenter = |local: &HashMap<i32, Point>| -> Recentered {
                let (sx, sy) = local
                    .values()
                    .fold((0.0, 0.0), |(sx, sy), p| (sx + p.x, sy + p.y));
                let n = local.len() as f64;
                let (dx, dy) = (center.x - sx / n, center.y - sy / n);
                let mut radius = 0.0f64;
                let mut recentered = HashMap::with_capacity(local.len());
                for (&id, &p) in local {
                    let p = Point {
                        x: p.x + dx,
                        y: p.y + dy,
                    };
                    radius = radius.max((p.x - center.x).hypot(p.y - center.y));
                    recentered.insert(id, p);
                }
                // the anchors below pull devices toward wherever their external neighbors are,
                // which can spread them wider than a plain internal-only layout would — further
                // than `promised_radius` (what the meta layout, above, already reserved this
                // site against every other site's own radius): shrink evenly around the center
                // to fit back within it, rather than let two sites whose circles the meta
                // layout kept apart end up overlapping anyway once each is actually drawn at
                // its real size.
                let shrink = if radius > promised_radius {
                    promised_radius / radius
                } else {
                    1.0
                };
                let positions = recentered
                    .into_iter()
                    .map(|(id, p)| {
                        (
                            id,
                            Point {
                                x: center.x + (p.x - center.x) * shrink,
                                y: center.y + (p.y - center.y) * shrink,
                            },
                        )
                    })
                    .collect();
                Recentered {
                    positions,
                    dx,
                    dy,
                    shrink,
                }
            };
            let final_local = if ids.len() > 1 {
                let internal_edges = edge_pairs
                    .iter()
                    .copied()
                    .filter(|&Edge { a, b }| {
                        cluster_of_node[&a] == cluster && cluster_of_node[&b] == cluster
                    })
                    .collect::<Vec<_>>();
                // a device cabled to another site is pulled toward that site's direction too, by a
                // fixed point well outside the local area in that direction (pinned, so it never
                // moves itself) — otherwise devices would lay out from their internal cables alone,
                // regardless of which side of the circle actually faces the site they also connect
                // to, making that connection longer than it has to be and more likely to cross
                // another site's circle on the way.
                let side = area_side(ids.len());
                let local_center = Point {
                    x: side / 2.0,
                    y: side / 2.0,
                };
                let mut anchor_of_cluster = HashMap::<Cluster, i32>::new();
                let mut anchors = HashMap::<i32, Point>::new();
                let mut anchor_edges = Vec::new();
                for &Edge { a, b } in &edge_pairs {
                    let (ca, cb) = (cluster_of_node[&a], cluster_of_node[&b]);
                    let (local_end, other) = if ca == cluster && cb != cluster {
                        (a, cb)
                    } else if cb == cluster && ca != cluster {
                        (b, ca)
                    } else {
                        continue;
                    };
                    let Some(&other_center) = meta_positions.get(&other) else {
                        continue;
                    };
                    let anchor_id = *anchor_of_cluster
                        .entry(other)
                        .or_insert_with(|| -1_000_000 - anchors.len() as i32);
                    anchors.entry(anchor_id).or_insert_with(|| {
                        let (dx, dy) = (other_center.x - center.x, other_center.y - center.y);
                        let dist = dx.hypot(dy).max(0.01);
                        Point {
                            x: local_center.x + dx / dist * side,
                            y: local_center.y + dy / dist * side,
                        }
                    });
                    anchor_edges.push(Edge {
                        a: local_end,
                        b: anchor_id,
                    });
                }
                let all_ids: Vec<i32> =
                    ids.iter().copied().chain(anchors.keys().copied()).collect();
                let all_edges: Vec<Edge<i32>> =
                    internal_edges.iter().copied().chain(anchor_edges).collect();
                let compute_local = |extra_pins: &HashMap<i32, Point>| -> HashMap<i32, Point> {
                    let mut pins = anchors.clone();
                    pins.extend(extra_pins);
                    let raw = force_layout(
                        &all_ids,
                        &all_edges,
                        &pins,
                        &HashMap::new(),
                        0.0,
                        side,
                        &HashMap::new(),
                    );
                    // untangle the devices' cabling within the circle too, by swapping devices
                    // as the meta layout above does: the anchors bias which side of the circle
                    // a device ends up on, but a device cabled outside could still sit behind
                    // the others, its line leaving across the circle (E21). So a line to an
                    // anchor counts like a cable, it stands for the way out toward that site,
                    // and so does a line passing through a device it doesn't end at.
                    let mut local = raw;
                    let mut movable = ids.clone();
                    movable.sort_unstable();
                    reduce_by_swaps(&mut local, &movable, |local| {
                        count_crossings(local, &all_edges)
                            + count_through_devices(local, &all_edges, &movable)
                    });
                    local.retain(|id, _| !anchors.contains_key(id));
                    local
                };
                let Recentered {
                    positions: local,
                    dx,
                    dy,
                    shrink,
                } = recenter(&compute_local(&HashMap::new()));
                let pinned_here: HashMap<i32, Point> = ids
                    .iter()
                    .filter_map(|id| Some((*id, *pinned.get(id)?)))
                    .collect();
                if pinned_here.is_empty() {
                    local
                } else {
                    // a pinned device's desired position is in the final, absolute space (same as
                    // every other override): invert the recenter this site just did (above, without
                    // knowing about any pin yet) to place it in local space instead, pin it there,
                    // and lay the site out again so the rest reacts to it. Approximate, not exact —
                    // the second pass's own recenter/shrink can differ slightly from the first's,
                    // which this inversion still assumes — but close enough for a live, reacting
                    // picture; the exact position is guaranteed separately, by `map()`'s own
                    // post-override of `positions` after this returns.
                    let local_pins: HashMap<i32, Point> = pinned_here
                        .iter()
                        .map(|(&id, &p)| {
                            (
                                id,
                                Point {
                                    x: (p.x - center.x) / shrink + center.x - dx,
                                    y: (p.y - center.y) / shrink + center.y - dy,
                                },
                            )
                        })
                        .collect();
                    recenter(&compute_local(&local_pins)).positions
                }
            } else {
                recenter(&HashMap::from([(ids[0], center)])).positions
            };
            // a device cabled to another site to the rim, at the angle the anchors gave it: its
            // lines leave the circle without crossing it (E21)
            for (id, p) in final_local {
                let external = edge_pairs.iter().any(|&Edge { a, b }| {
                    (a == id && cluster_of_node[&b] != cluster)
                        || (b == id && cluster_of_node[&a] != cluster)
                });
                let (dx, dy) = (p.x - center.x, p.y - center.y);
                let dist = dx.hypot(dy);
                let pos = if external && ids.len() > 1 && dist > 0.01 && !pinned.contains_key(&id) {
                    let rim = promised_radius * RIM;
                    Point {
                        x: center.x + dx / dist * rim,
                        y: center.y + dy / dist * rim,
                    }
                } else {
                    p
                };
                positions.insert(id, pos);
            }
            if let Cluster::Site(site) = cluster {
                let name = site_name_of.get(&site).cloned().unwrap_or_default();
                circles.push(SiteCircle {
                    site,
                    name,
                    center,
                    radius: promised_radius + SITE_PADDING,
                });
                circle_members.push(ids.clone());
            }
        }
        SitesLaidOut {
            positions,
            circles,
            circle_members,
        }
    };
    let SitesLaidOut {
        mut positions,
        mut circles,
        mut circle_members,
    } = lay_out_sites(&meta_positions);

    let site_radii: HashMap<Cluster, f64> = circles
        .iter()
        .map(|c| (Cluster::Site(c.site), c.radius))
        .collect();
    reduce_site_crossings(
        &mut positions,
        &mut meta_positions,
        &cluster_of_node,
        &groups,
        &edge_pairs,
        &site_radii,
    );
    // swapped sites leave their devices facing where the others were: again from the new centers
    if circles
        .iter()
        .any(|c| c.center != meta_positions[&Cluster::Site(c.site)])
    {
        SitesLaidOut {
            positions,
            circles,
            circle_members,
        } = lay_out_sites(&meta_positions);
    }

    // a hard backstop, not the primary mechanism: the meta layout's radius-aware repulsion
    // above, now promising each site exactly the radius it's shrunk to fit, keeps circles
    // apart as part of laying them out well (short cross-site edges and no overlap come from
    // the same simulation, not two conflicting passes) — but `force_layout` is a fixed number
    // of iterations, not a solver run to convergence, so an unusual graph could in principle
    // still leave a pair closer than their radii demand. Push such a pair fully apart, keeping
    // their members with them: a visual guarantee of no overlap at all, whatever the physics
    // above did or didn't fully resolve.
    for _ in 0..200 {
        let mut moved = false;
        for i in 0..circles.len() {
            for j in (i + 1)..circles.len() {
                let (dx, dy) = (
                    circles[j].center.x - circles[i].center.x,
                    circles[j].center.y - circles[i].center.y,
                );
                let dist = dx.hypot(dy).max(0.01);
                let min_dist = circles[i].radius + circles[j].radius + SITE_GAP;
                if dist >= min_dist {
                    continue;
                }
                moved = true;
                let push = (min_dist - dist) / 2.0;
                let (ux, uy) = (dx / dist * push, dy / dist * push);
                circles[i].center.x -= ux;
                circles[i].center.y -= uy;
                circles[j].center.x += ux;
                circles[j].center.y += uy;
                for &id in &circle_members[i] {
                    if let Some(p) = positions.get_mut(&id) {
                        p.x -= ux;
                        p.y -= uy;
                    }
                }
                for &id in &circle_members[j] {
                    if let Some(p) = positions.get_mut(&id) {
                        p.x += ux;
                        p.y += uy;
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }

    reduce_faults_within_sites(&mut positions, &edge_pairs, &circle_members);

    // keep the same "everything at MARGIN or more" convention plain force_layout gives: the
    // recentering above can have pushed a cluster's devices, or a circle's edge, past it
    let min_x = positions
        .values()
        .map(|p| p.x)
        .chain(circles.iter().map(|c| c.center.x - c.radius))
        .fold(f64::INFINITY, f64::min);
    let min_y = positions
        .values()
        .map(|p| p.y)
        .chain(circles.iter().map(|c| c.center.y - c.radius))
        .fold(f64::INFINITY, f64::min);
    let (shift_x, shift_y) = (MARGIN - min_x, MARGIN - min_y);
    for p in positions.values_mut() {
        p.x += shift_x;
        p.y += shift_y;
    }
    for c in &mut circles {
        c.center.x += shift_x;
        c.center.y += shift_y;
    }
    Clustered { positions, circles }
}

/// A site's devices laid out around a center (`clustered_layout`'s `recenter`), with the shift
/// and shrink that took them there.
struct Recentered {
    positions: HashMap<i32, Point>,
    dx: f64,
    dy: f64,
    shrink: f64,
}

/// Every site laid out around its center (`clustered_layout`'s `lay_out_sites`).
struct SitesLaidOut {
    positions: HashMap<i32, Point>,
    circles: Vec<SiteCircle>,
    /// the devices of each of `circles`
    circle_members: Vec<Vec<i32>>,
}
