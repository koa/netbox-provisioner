//! The network map (`Query.topology.networkState`, `docs/concepts/network-map.md`): devices
//! and NetBox's cables and wireless links as an SVG, coloured by what the OSPF database of one
//! router says; devices without OSPF, and their edges, are left out. Scoped to one site, or
//! without a site to every device of every site. Asked anew every few seconds; reading only.
//! The layout is force-directed (Fruchterman–Reingold, `force_layout`), own implementation, no
//! JS library; the routers NetBox doesn't know are one node, pulled toward the site's routers
//! it borders. Over every site (more than one site among the nodes), each site's devices are
//! further grouped into their own dashed circle (`clustered_layout`): the circles lay out
//! against each other like a layout of their own (an edge crossing sites pulls the two sites
//! together, same as a device edge pulls two devices together within one; a site's circle never
//! overlaps another's), and a device cabled to another site is pulled toward that site's
//! direction within its own circle, so the connection is as short as the circles' layout allows
//! and less likely to cross a site that isn't part of it.
//!
//! The overview (nodes, edges, OSPF state) is `networkState`, asked and polled on its own so it
//! renders fast; health, resources and PoE edges are `networkStateDetails` (one round trip per
//! device, slower), asked and polled independently and shown in a popup on hovering a node,
//! with a spinner until the first answer of its own poll arrives.
use crate::{
    components::job::RETRY,
    error::FrontendError,
    graphql::{
        authenticated::{
            NetworkMap as NetworkMapQuery, NetworkMapDetails as NetworkMapDetailsQuery, PublishLayout as PublishLayoutQuery,
            network_map::{self, MapDownReason, MapEdgeKind, MapEdgeState, NetworkMapTopologyNetworkState as State, NodeState},
            network_map_details::{self, NetworkMapDetailsTopologyNetworkStateDetails as DetailsState, PoeProblem},
            publish_layout,
        },
        query_authenticated,
    },
    i18n::network_map as t,
    local_storage,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
use wasm_bindgen::JsCast;
use yew::{Component, Context, ContextHandle, Html, Properties, html, html::Scope, platform::{spawn_local, time::sleep}};
use yew_nested_router::prelude::RouterContext;

/// How often the map asks (the backend reads the LSDB at most this often too, `LSDB_AGE`).
const POLL: Duration = Duration::from_secs(5);
const MARGIN: f64 = 70.0;
/// the node standing for the routers NetBox doesn't know
const OUTSIDE: i64 = -1;

#[derive(Properties, PartialEq)]
pub struct NetworkMapProps {
    /// without one, every device of every site
    #[prop_or_default]
    pub site: Option<u32>,
}

pub struct NetworkMap {
    state: Option<State>,
    /// none until the details poll's first answer; a node not in it then has none, for good
    details: Option<DetailsState>,
    /// the device of the node under the pointer, for the popup
    hover: Option<i64>,
    error: Option<FrontendError>,
    alive: Rc<Cell<bool>>,
    router: Option<RouterContext<AppRoute>>,
    _router_handle: Option<ContextHandle<RouterContext<AppRoute>>>,
    allow_netbox_write: bool,
    /// a node the user dragged in this browser (`localStorage`, key [`storage_key`]), on top of
    /// NetBox's published positions; cleared by `ResetOverrides`, written to `localStorage` once
    /// a drag ends
    overridden: HashMap<i64, (f64, f64)>,
    /// the device currently being dragged, and whether it actually moved yet (a plain click,
    /// pointer down and up without a move between, must still navigate as before)
    dragging: Option<(i64, bool)>,
    /// set by `DragEnd` when the drag moved the node, consumed by the next `Open`: the pointer
    /// up after a drag fires a click too, which must not also navigate away from the map
    suppress_click: bool,
    publish: PublishState,
}

#[derive(Default)]
enum PublishState {
    #[default]
    Idle,
    Publishing,
    Done(u32),
    Failed(String),
}

pub enum NetworkMapMsg {
    Loaded(State),
    DetailsLoaded(DetailsState),
    Error(FrontendError),
    Router(RouterContext<AppRoute>),
    /// the device clicked, and its own site (it may be another one than `site`)
    Open(i64, Option<i64>),
    Hover(Option<i64>),
    AllowNetboxWrite(bool),
    DragStart(i64),
    /// already mapped into the SVG's own coordinates, not the raw pointer event's
    DragMove(f64, f64),
    DragEnd,
    ResetOverrides,
    Publish(Vec<(i64, (f64, f64))>),
    Published(Result<u32, FrontendError>),
}

/// Where a browser's own, unpublished arrangement is kept (`local_storage`): scoped to the page
/// (a site's map, or the all-sites one), since the two use unrelated coordinate spaces.
fn storage_key(site: Option<u32>) -> String {
    format!("network-map-positions:{}", site.map(|s| s.to_string()).unwrap_or_else(|| "all".into()))
}

impl Component for NetworkMap {
    type Message = NetworkMapMsg;
    type Properties = NetworkMapProps;

    fn create(ctx: &Context<Self>) -> Self {
        let (router, router_handle) = ctx
            .link()
            .context::<RouterContext<AppRoute>>(ctx.link().callback(NetworkMapMsg::Router))
            .unzip();
        let alive = Rc::new(Cell::new(true));
        poll(ctx.link().clone(), ctx.props().site, alive.clone());
        poll_details(ctx.link().clone(), ctx.props().site, alive.clone());
        let overridden = local_storage::stored(&storage_key(ctx.props().site))
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            state: None,
            details: None,
            hover: None,
            error: None,
            alive,
            router,
            _router_handle: router_handle,
            allow_netbox_write: false,
            overridden,
            dragging: None,
            suppress_click: false,
            publish: PublishState::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NetworkMapMsg::Loaded(state) => {
                self.state = Some(state);
                self.error = None;
            }
            NetworkMapMsg::DetailsLoaded(details) => self.details = Some(details),
            NetworkMapMsg::Error(error) => self.error = Some(error),
            NetworkMapMsg::Router(router) => self.router = Some(router),
            NetworkMapMsg::AllowNetboxWrite(allowed) => self.allow_netbox_write = allowed,
            NetworkMapMsg::Open(device, site) => {
                if self.suppress_click {
                    self.suppress_click = false;
                    return false;
                }
                if let (Some(router), Some(site)) = (&self.router, site) {
                    router.push(AppRoute::device(site as u32, device as u32));
                }
                return false;
            }
            NetworkMapMsg::DragStart(id) => self.dragging = Some((id, false)),
            NetworkMapMsg::DragMove(x, y) => {
                let Some((id, _)) = self.dragging else { return false };
                self.dragging = Some((id, true));
                self.overridden.insert(id, (x, y));
            }
            NetworkMapMsg::DragEnd => {
                let Some((_, moved)) = self.dragging.take() else { return false };
                if moved {
                    self.suppress_click = true;
                    if let Ok(json) = serde_json::to_string(&self.overridden) {
                        local_storage::store(&storage_key(ctx.props().site), &json);
                    }
                }
            }
            NetworkMapMsg::ResetOverrides => {
                self.overridden.clear();
                local_storage::remove(&storage_key(ctx.props().site));
            }
            NetworkMapMsg::Publish(positions) => {
                self.publish = PublishState::Publishing;
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let variables = publish_layout::Variables {
                        positions: positions
                            .into_iter()
                            .map(|(device_id, (x, y))| publish_layout::DevicePositionInput {
                                device_id,
                                x: x.round() as i64,
                                y: y.round() as i64,
                            })
                            .collect(),
                    };
                    let result = query_authenticated::<PublishLayoutQuery, _>(scope.clone(), variables)
                        .await
                        .map(|data| data.publish_layout as u32);
                    scope.send_message(NetworkMapMsg::Published(result));
                });
            }
            NetworkMapMsg::Published(Ok(count)) => {
                self.publish = PublishState::Done(count);
                self.overridden.clear();
                local_storage::remove(&storage_key(ctx.props().site));
            }
            NetworkMapMsg::Published(Err(error)) => self.publish = PublishState::Failed(error.to_string()),
            NetworkMapMsg::Hover(device) => self.hover = device,
        }
        true
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        self.alive.set(false);
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let error = self.error.as_ref().map(FrontendError::to_html);
        let body = match &self.state {
            None if self.error.is_none() => html!(<Spinner/>),
            None => Html::default(),
            Some(state) => {
                let no_router = if ctx.props().site.is_some() { t::NO_ROUTER } else { t::NO_ROUTER_ALL };
                let source = match (&state.read_from, &state.error) {
                    (Some(device), _) => t::read_from(&device.name, state.age as u32),
                    (None, Some(error)) => t::unreadable(error),
                    (None, None) => no_router.to_string(),
                };
                let interaction = Interaction {
                    overridden: &self.overridden,
                    dragging: self.dragging.is_some(),
                    allow_netbox_write: self.allow_netbox_write,
                    publish: &self.publish,
                };
                html! {
                    <>
                        <p>{source}</p>
                        {map(state, &self.details, self.hover, &interaction, ctx.link())}
                        {legend()}
                    </>
                }
            }
        };
        let intro = if ctx.props().site.is_some() { t::INTRO } else { t::INTRO_ALL };
        html! {
            <Card>
                <CardHeader><CardTitle>{t::TITLE}</CardTitle></CardHeader>
                <CardBody>
                    <p>{intro}</p>
                    {error}
                    {body}
                </CardBody>
            </Card>
        }
    }
}

fn poll(scope: Scope<NetworkMap>, site: Option<u32>, alive: Rc<Cell<bool>>) {
    yew::platform::spawn_local(async move {
        let mut failed = 0;
        while alive.get() {
            let variables = network_map::Variables { site: site.map(|s| s as i64) };
            match query_authenticated::<NetworkMapQuery, _>(scope.clone(), variables).await {
                Ok(data) => {
                    failed = 0;
                    let state = data.topology.network_state;
                    if alive.get() {
                        scope.send_message(NetworkMapMsg::AllowNetboxWrite(data.allow_netbox_write));
                        scope.send_message(NetworkMapMsg::Loaded(state));
                    }
                    sleep(POLL).await;
                }
                Err(error) => {
                    if alive.get() {
                        scope.send_message(NetworkMapMsg::Error(error));
                    }
                    sleep(RETRY[failed.min(RETRY.len() - 1)]).await;
                    failed += 1;
                }
            }
        }
    });
}

/// Health, resources and PoE edges, independently of [`poll`] so a slow round trip to every
/// device never holds up the overview; a failure here stays silent (no page-level error), the
/// popup just keeps its spinner until the next try.
fn poll_details(scope: Scope<NetworkMap>, site: Option<u32>, alive: Rc<Cell<bool>>) {
    yew::platform::spawn_local(async move {
        let mut failed = 0;
        while alive.get() {
            let variables = network_map_details::Variables { site: site.map(|s| s as i64) };
            match query_authenticated::<NetworkMapDetailsQuery, _>(scope.clone(), variables).await {
                Ok(data) => {
                    failed = 0;
                    if alive.get() {
                        scope.send_message(NetworkMapMsg::DetailsLoaded(data.topology.network_state_details));
                    }
                    sleep(POLL).await;
                }
                Err(_) => {
                    sleep(RETRY[failed.min(RETRY.len() - 1)]).await;
                    failed += 1;
                }
            }
        }
    });
}

/// `state`'s nodes (plus the outside node, if any, pulled toward the routers it borders) and
/// edges, as plain ids, for [`force_layout`].
fn graph(state: &State) -> (Vec<i64>, Vec<(i64, i64)>) {
    let mut nodes: Vec<i64> = state.nodes.iter().map(|n| n.device.id).collect();
    let mut edges: Vec<(i64, i64)> = state.edges.iter().map(|e| (e.a.device.id, e.b.device.id)).collect();
    if state.outside > 0 {
        nodes.push(OUTSIDE);
        edges.extend(state.outside_at.iter().map(|&id| (OUTSIDE, id)));
    }
    (nodes, edges)
}

/// A side length that gives `count` nodes roughly as much room as the old column layout did
/// (`COLUMN` × `ROW` per node, in a square-ish area) — big enough that `force_layout`'s
/// repulsion has space to spread nodes out, not a hard limit (nodes may end up outside it; the
/// SVG's `viewBox` is sized from the actual positions, not this).
fn area_side(count: usize) -> f64 {
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
/// oscillating instead of settling. Deterministic: a node without a pinned position starts at a
/// position hashed from its id, not randomly, so the same graph lays out the same way every
/// time.
const ITERATIONS: u32 = 300;

fn force_layout(
    nodes: &[i64],
    edges: &[(i64, i64)],
    pinned: &HashMap<i64, (f64, f64)>,
    radii: &HashMap<i64, f64>,
    gap: f64,
    side: f64,
) -> HashMap<i64, (f64, f64)> {
    if nodes.is_empty() {
        return HashMap::new();
    }
    let k = (side * side / nodes.len() as f64).sqrt();
    let mut pos: HashMap<i64, (f64, f64)> =
        nodes.iter().map(|&id| (id, pinned.get(&id).copied().unwrap_or_else(|| seeded(id, side)))).collect();
    for step in 0..ITERATIONS {
        let mut disp = HashMap::<i64, (f64, f64)>::new();
        for (i, &a) in nodes.iter().enumerate() {
            for &b in &nodes[i + 1..] {
                let (dx, dy) = (pos[&a].0 - pos[&b].0, pos[&a].1 - pos[&b].1);
                let dist = dx.hypot(dy).max(0.01);
                let effective_k = (radii.get(&a).copied().unwrap_or(0.0) + radii.get(&b).copied().unwrap_or(0.0) + gap).max(k);
                let force = effective_k * effective_k / dist;
                let (ux, uy) = (dx / dist * force, dy / dist * force);
                let da = disp.entry(a).or_default();
                da.0 += ux;
                da.1 += uy;
                let db = disp.entry(b).or_default();
                db.0 -= ux;
                db.1 -= uy;
            }
        }
        for &(a, b) in edges {
            let (Some(&pa), Some(&pb)) = (pos.get(&a), pos.get(&b)) else { continue };
            let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
            let dist = dx.hypot(dy).max(0.01);
            let force = dist * dist / k;
            let (ux, uy) = (dx / dist * force, dy / dist * force);
            let da = disp.entry(a).or_default();
            da.0 -= ux;
            da.1 -= uy;
            let db = disp.entry(b).or_default();
            db.0 += ux;
            db.1 += uy;
        }
        let temperature = side * 0.1 * (1.0 - f64::from(step) / f64::from(ITERATIONS));
        for &id in nodes {
            if pinned.contains_key(&id) {
                continue;
            }
            let (dx, dy) = disp.get(&id).copied().unwrap_or_default();
            let dist = dx.hypot(dy).max(0.01);
            let limited = dist.min(temperature);
            let p = pos.get_mut(&id).expect("every node has a position");
            p.0 = (p.0 + dx / dist * limited).clamp(0.0, side);
            p.1 = (p.1 + dy / dist * limited).clamp(0.0, side);
        }
    }
    for p in pos.values_mut() {
        p.0 += MARGIN;
        p.1 += MARGIN;
    }
    pos
}

/// A position hashed from `id` (MurmurHash3's finalizer), not random, so a layout without a
/// pinned position for every node still comes out the same way every time.
fn seeded(id: i64, side: f64) -> (f64, f64) {
    let mut h = id as u64 ^ 0x9E37_79B9_7F4A_7C15;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    let x = (h & 0xFFFF_FFFF) as f64 / f64::from(u32::MAX);
    let y = (h >> 32) as f64 / f64::from(u32::MAX);
    (x * side, y * side)
}

/// A node's site, for [`clustered_layout`]: `Site` for a device with one, `Alone` (keyed by the
/// node's own id, so two sideless nodes never share one) for the outside node or a device
/// without a site. `Ord` only so [`clustered_layout`] can put clusters in a `BTreeMap` and get
/// the same iteration order every time (a `HashMap`'s is randomized per instance, which would
/// make `force_layout`'s floating-point sums round differently from one poll to the next and
/// the whole map jitter, even with nothing changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Cluster {
    Site(i64),
    Alone(i64),
}

/// A site's devices, grouped into their own circle.
struct SiteCircle {
    name: String,
    center: (f64, f64),
    radius: f64,
}

const SITE_PADDING: f64 = 35.0;
const SITE_MIN_RADIUS: f64 = 50.0;
/// minimum gap [`force_layout`]'s meta pass keeps between two site circles, so they end up
/// visibly separate, not just touching
const SITE_GAP: f64 = 25.0;

/// Whether segments `p1`-`p2` and `p3`-`p4` cross in their interior (touching at an endpoint,
/// or being collinear, doesn't count — good enough for a visual heuristic over lines that share
/// no endpoint, which is the only way [`reduce_crossings`] calls this).
fn segments_cross(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> bool {
    fn orientation(p: (f64, f64), q: (f64, f64), r: (f64, f64)) -> i8 {
        let val = (q.1 - p.1) * (r.0 - q.0) - (q.0 - p.0) * (r.1 - q.1);
        if val.abs() < 1e-9 {
            0
        } else if val > 0.0 {
            1
        } else {
            -1
        }
    }
    let (o1, o2, o3, o4) = (orientation(p1, p2, p3), orientation(p1, p2, p4), orientation(p3, p4, p1), orientation(p3, p4, p2));
    o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0 && o1 != o2 && o3 != o4
}

fn count_crossings<K: Eq + std::hash::Hash + Copy>(positions: &HashMap<K, (f64, f64)>, edges: &[(K, K)]) -> usize {
    let mut n = 0;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let (a1, b1) = edges[i];
            let (a2, b2) = edges[j];
            if a1 == a2 || a1 == b2 || b1 == a2 || b1 == b2 {
                continue; // sharing a site isn't a crossing, the lines are meant to meet there
            }
            if segments_cross(positions[&a1], positions[&b1], positions[&a2], positions[&b2]) {
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
fn reduce_crossings<K: Eq + std::hash::Hash + Ord + Copy>(positions: &mut HashMap<K, (f64, f64)>, edges: &[(K, K)]) {
    let mut nodes: Vec<K> = positions.keys().copied().collect();
    nodes.sort(); // deterministic order: a HashMap's own would make the swap sequence, and so
    // the result, depend on this process's hashing, not just the graph (see `force_layout`'s own
    // doc comment on `BTreeMap` for why that matters here)
    for _ in 0..nodes.len().min(30) {
        let mut improved = false;
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let before = count_crossings(positions, edges);
                let (pi, pj) = (positions[&nodes[i]], positions[&nodes[j]]);
                positions.insert(nodes[i], pj);
                positions.insert(nodes[j], pi);
                if count_crossings(positions, edges) < before {
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

/// Two-level layout for the all-sites map: a site's devices lay out among themselves as
/// [`force_layout`] always has, then the sites lay out against each other the same way (one
/// meta-node per site, one meta-edge per device edge crossing sites), and each site's devices
/// are shifted onto its meta position. The outside node, and a device without a site, each stay
/// their own single-node "site" in the meta layout (pulled toward what they're cabled to) but
/// get no circle. Edges between devices are drawn at the devices' own, now clustered, positions
/// — only where a node ends up, not what it's grouped under, changes.
fn clustered_layout(state: &State) -> (HashMap<i64, (f64, f64)>, Vec<SiteCircle>) {
    let (node_ids, edge_pairs) = graph(state);
    if node_ids.is_empty() {
        return (HashMap::new(), Vec::new());
    }
    let site_of: HashMap<i64, Option<i64>> = state.nodes.iter().map(|n| (n.device.id, n.device.site)).collect();
    let site_name_of: HashMap<i64, String> =
        state.nodes.iter().filter_map(|n| Some((n.device.site?, n.site_name.clone()?))).collect();
    let cluster_of = |id: i64| match site_of.get(&id).copied().flatten() {
        Some(site) => Cluster::Site(site),
        None => Cluster::Alone(id),
    };

    let mut groups = BTreeMap::<Cluster, Vec<i64>>::new();
    for &id in &node_ids {
        groups.entry(cluster_of(id)).or_default().push(id);
    }
    let cluster_of_node: HashMap<i64, Cluster> =
        groups.iter().flat_map(|(&c, ids)| ids.iter().map(move |&id| (id, c))).collect();

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
    let meta_id_of: HashMap<Cluster, i64> = clusters.iter().enumerate().map(|(i, &c)| (c, i as i64)).collect();
    let meta_ids: Vec<i64> = (0..clusters.len() as i64).collect();
    let meta_edges: Vec<(i64, i64)> = edge_pairs
        .iter()
        .filter_map(|&(a, b)| {
            let (ca, cb) = (cluster_of_node[&a], cluster_of_node[&b]);
            (ca != cb).then(|| (meta_id_of[&ca], meta_id_of[&cb]))
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
    let meta_radii: HashMap<i64, f64> = clusters.iter().map(|&c| (meta_id_of[&c], radius_of(c))).collect();
    let meta_area: f64 = meta_radii.values().map(|r| std::f64::consts::PI * r * r).sum();
    let meta_side = (meta_area * 2.0).sqrt().max(300.0);
    let meta_positions_by_id = force_layout(&meta_ids, &meta_edges, &HashMap::new(), &meta_radii, SITE_GAP, meta_side);
    let mut meta_positions: HashMap<Cluster, (f64, f64)> = clusters.iter().map(|&c| (c, meta_positions_by_id[&meta_id_of[&c]])).collect();
    reduce_crossings(&mut meta_positions, &meta_edges.iter().map(|&(a, b)| (clusters[a as usize], clusters[b as usize])).collect::<Vec<_>>());

    let mut positions = HashMap::new();
    let mut circles = Vec::new();
    let mut circle_members: Vec<Vec<i64>> = Vec::new();
    for (&cluster, ids) in &groups {
        let center = meta_positions[&cluster];
        let promised_radius = radius_of(cluster);
        let local = if ids.len() > 1 {
            let internal_edges =
                edge_pairs.iter().copied().filter(|&(a, b)| cluster_of_node[&a] == cluster && cluster_of_node[&b] == cluster).collect::<Vec<_>>();
            // a device cabled to another site is pulled toward that site's direction too, by a
            // fixed point well outside the local area in that direction (pinned, so it never
            // moves itself) — otherwise devices would lay out from their internal cables alone,
            // regardless of which side of the circle actually faces the site they also connect
            // to, making that connection longer than it has to be and more likely to cross
            // another site's circle on the way.
            let side = area_side(ids.len());
            let local_center = (side / 2.0, side / 2.0);
            let mut anchor_of_cluster = HashMap::<Cluster, i64>::new();
            let mut anchors = HashMap::<i64, (f64, f64)>::new();
            let mut anchor_edges = Vec::new();
            for &(a, b) in &edge_pairs {
                let (ca, cb) = (cluster_of_node[&a], cluster_of_node[&b]);
                let (local_end, other) = if ca == cluster && cb != cluster {
                    (a, cb)
                } else if cb == cluster && ca != cluster {
                    (b, ca)
                } else {
                    continue;
                };
                let Some(&other_center) = meta_positions.get(&other) else { continue };
                let anchor_id = *anchor_of_cluster.entry(other).or_insert_with(|| -1_000_000 - i64::from(anchors.len() as u32));
                anchors.entry(anchor_id).or_insert_with(|| {
                    let (dx, dy) = (other_center.0 - center.0, other_center.1 - center.1);
                    let dist = dx.hypot(dy).max(0.01);
                    (local_center.0 + dx / dist * side, local_center.1 + dy / dist * side)
                });
                anchor_edges.push((local_end, anchor_id));
            }
            let all_ids: Vec<i64> = ids.iter().copied().chain(anchors.keys().copied()).collect();
            let all_edges: Vec<(i64, i64)> = internal_edges.iter().copied().chain(anchor_edges).collect();
            let raw = force_layout(&all_ids, &all_edges, &anchors, &HashMap::new(), 0.0, side);
            let mut local: HashMap<i64, (f64, f64)> = ids.iter().map(|&id| (id, raw[&id])).collect();
            // untangle the devices' own cabling within the circle too, same local search as the
            // meta layout above — the anchors already bias which side of the circle a device
            // ends up on, this only straightens out crossings among the internal cables
            reduce_crossings(&mut local, &internal_edges);
            local
        } else {
            HashMap::from([(ids[0], center)])
        };
        let (sx, sy) = local.values().fold((0.0, 0.0), |(sx, sy), p| (sx + p.0, sy + p.1));
        let n = local.len() as f64;
        let (dx, dy) = (center.0 - sx / n, center.1 - sy / n);
        let mut radius = 0.0f64;
        let mut recentered = HashMap::with_capacity(local.len());
        for (&id, &(x, y)) in &local {
            let p = (x + dx, y + dy);
            radius = radius.max((p.0 - center.0).hypot(p.1 - center.1));
            recentered.insert(id, p);
        }
        // the anchors above pull devices toward wherever their external neighbors are, which
        // can spread them wider than a plain internal-only layout would — further than
        // `promised_radius` (what the meta layout, above, already reserved this site against
        // every other site's own radius): shrink evenly around the center to fit back within
        // it, rather than let two sites whose circles the meta layout kept apart end up
        // overlapping anyway once each is actually drawn at its real size.
        let shrink = if radius > promised_radius { promised_radius / radius } else { 1.0 };
        for (id, (x, y)) in recentered {
            positions.insert(id, (center.0 + (x - center.0) * shrink, center.1 + (y - center.1) * shrink));
        }
        if let Cluster::Site(site) = cluster {
            let name = site_name_of.get(&site).cloned().unwrap_or_default();
            circles.push(SiteCircle { name, center, radius: promised_radius + SITE_PADDING });
            circle_members.push(ids.clone());
        }
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
                let (dx, dy) = (circles[j].center.0 - circles[i].center.0, circles[j].center.1 - circles[i].center.1);
                let dist = dx.hypot(dy).max(0.01);
                let min_dist = circles[i].radius + circles[j].radius + SITE_GAP;
                if dist >= min_dist {
                    continue;
                }
                moved = true;
                let push = (min_dist - dist) / 2.0;
                let (ux, uy) = (dx / dist * push, dy / dist * push);
                circles[i].center.0 -= ux;
                circles[i].center.1 -= uy;
                circles[j].center.0 += ux;
                circles[j].center.1 += uy;
                for &id in &circle_members[i] {
                    if let Some(p) = positions.get_mut(&id) {
                        p.0 -= ux;
                        p.1 -= uy;
                    }
                }
                for &id in &circle_members[j] {
                    if let Some(p) = positions.get_mut(&id) {
                        p.0 += ux;
                        p.1 += uy;
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }

    // keep the same "everything at MARGIN or more" convention plain force_layout gives: the
    // recentering above can have pushed a cluster's devices, or a circle's edge, past it
    let min_x = positions.values().map(|p| p.0).chain(circles.iter().map(|c| c.center.0 - c.radius)).fold(f64::INFINITY, f64::min);
    let min_y = positions.values().map(|p| p.1).chain(circles.iter().map(|c| c.center.1 - c.radius)).fold(f64::INFINITY, f64::min);
    let (shift_x, shift_y) = (MARGIN - min_x, MARGIN - min_y);
    for p in positions.values_mut() {
        p.0 += shift_x;
        p.1 += shift_y;
    }
    for c in &mut circles {
        c.center.0 += shift_x;
        c.center.1 += shift_y;
    }
    (positions, circles)
}

/// The interaction state [`map`] needs beyond the polled graph itself: dragging, the browser's
/// own overrides and publishing them (`docs/concepts/network-map.md`, "Positionen speichern").
struct Interaction<'a> {
    overridden: &'a HashMap<i64, (f64, f64)>,
    dragging: bool,
    allow_netbox_write: bool,
    publish: &'a PublishState,
}

fn map(state: &State, details: &Option<DetailsState>, hover: Option<i64>, interaction: &Interaction, scope: &Scope<NetworkMap>) -> Html {
    let overridden = interaction.overridden;
    let dragging = interaction.dragging;
    let allow_netbox_write = interaction.allow_netbox_write;
    let publish = interaction.publish;
    // NetBox's published layout, overlaid by whatever the user dragged in this browser (wins
    // where both exist); fed into the layout below as pinned positions, so other nodes still
    // arrange themselves around a pinned one ([`force_layout`]'s own doc comment).
    let mut pinned: HashMap<i64, (f64, f64)> = state
        .nodes
        .iter()
        .filter_map(|n| n.position.as_ref().map(|p| (n.device.id, (p.x as f64, p.y as f64))))
        .collect();
    pinned.extend(overridden.iter());
    let site_count = state.nodes.iter().filter_map(|n| n.device.site).collect::<HashSet<_>>().len();
    let (mut positions, circles) = if site_count > 1 {
        clustered_layout(state)
    } else {
        let (node_ids, edge_pairs) = graph(state);
        // `pinned` holds the desired final rendered position, but `force_layout` adds `MARGIN`
        // to every position, pinned ones included, right before returning it; undo that here so
        // a pin actually lands where it says, rather than MARGIN further out.
        let pinned_for_layout: HashMap<i64, (f64, f64)> =
            pinned.iter().map(|(&id, &(x, y))| (id, (x - MARGIN, y - MARGIN))).collect();
        (force_layout(&node_ids, &edge_pairs, &pinned_for_layout, &HashMap::new(), 0.0, area_side(node_ids.len())), Vec::new())
    };
    if site_count > 1 {
        // clustered_layout's two-level physics can't take a pin without first inverting its
        // per-site recenter/shrink transform, which isn't known until it has already run,
        // circular; so a drag here instead just overrides the final spot, same as the
        // published layout would once a browser without its own override reads it back
        for (&id, &pos) in &pinned {
            if positions.contains_key(&id) {
                positions.insert(id, pos);
            }
        }
    }
    let names = state
        .nodes
        .iter()
        .map(|n| (n.device.id, n.device.name.as_str()))
        .collect::<HashMap<_, _>>();
    let width =
        positions.values().map(|p| p.0).chain(circles.iter().map(|c| c.center.0 + c.radius)).fold(0.0, f64::max) + MARGIN + 40.0;
    let height = positions.values().map(|p| p.1).chain(circles.iter().map(|c| c.center.1 + c.radius)).fold(0.0, f64::max) + MARGIN;
    let site_circles = circles.iter().map(|c| {
        html! {
            <g class="map-site">
                <circle cx={c.center.0.to_string()} cy={c.center.1.to_string()} r={c.radius.to_string()}/>
                <text x={c.center.0.to_string()} y={(c.center.1 - c.radius - 8.0).to_string()}>{&c.name}</text>
            </g>
        }
    });
    // parallel edges between the same two devices side by side
    let mut parallel = HashMap::<(i64, i64), f64>::new();
    let edges = state.edges.iter().filter_map(|edge| {
        let (a, b) = (edge.a.device.id, edge.b.device.id);
        let (pa, pb) = (positions.get(&a)?, positions.get(&b)?);
        let index = parallel.entry((a.min(b), a.max(b))).or_default();
        let offset = *index * 6.0;
        *index += 1.0;
        let class = match edge.state {
            MapEdgeState::UP => "up",
            MapEdgeState::V3_DOWN => "v3-down",
            MapEdgeState::DOWN => "down",
            _ => "not-ospf",
        };
        let radio = if edge.kind == MapEdgeKind::RADIO { " radio" } else { "" };
        let name = |id| names.get(&id).copied().unwrap_or_default();
        let state_text = match edge.state {
            MapEdgeState::UP => t::EDGE_UP.to_string(),
            MapEdgeState::V3_DOWN => t::EDGE_V3_DOWN.to_string(),
            MapEdgeState::DOWN => {
                let device = edge.reason_device.map(name).unwrap_or_default();
                match edge.reason {
                    Some(MapDownReason::ROUTER_MISSING) => t::router_missing(device),
                    Some(MapDownReason::NOT_ON_SEGMENT) => t::not_on_segment(device),
                    Some(MapDownReason::NO_SEGMENT) => t::NO_SEGMENT.to_string(),
                    _ => t::EDGE_DOWN.to_string(),
                }
            }
            _ => t::EDGE_NOT_OSPF.to_string(),
        };
        let title = format!(
            "{} {} – {} {}{}: {state_text}",
            edge.a.device.name,
            edge.a.name,
            edge.b.device.name,
            edge.b.name,
            if radio.is_empty() { String::new() } else { format!(" ({})", t::RADIO) },
        );
        Some(html! {
            <line
                class={format!("map-edge {class}{radio}")}
                x1={(pa.0 + offset).to_string()} y1={(pa.1 + offset).to_string()}
                x2={(pb.0 + offset).to_string()} y2={(pb.1 + offset).to_string()}
            ><title>{title}</title></line>
        })
    });
    let mut parallel_poe = HashMap::<(i64, i64), f64>::new();
    let poe_edges = details.iter().flat_map(|d| &d.poe_edges).filter_map(|edge| {
        let (a, b) = (edge.pse.device.id, edge.pd.device.id);
        let (pa, pb) = (positions.get(&a)?, positions.get(&b)?);
        let index = parallel_poe.entry((a.min(b), a.max(b))).or_default();
        let offset = *index * 6.0;
        *index += 1.0;
        let problem_text = match &edge.problem {
            Some(PoeProblem::NO_POE_OUT) => Some(t::POE_NO_POE_OUT),
            Some(PoeProblem::NO_LOAD) => Some(t::POE_NO_LOAD),
            Some(PoeProblem::PD_MISSING) => Some(t::POE_PD_MISSING),
            Some(PoeProblem::FEEDS_INSTEAD) => Some(t::POE_FEEDS_INSTEAD),
            Some(PoeProblem::UNDOCUMENTED) => Some(t::POE_UNDOCUMENTED),
            Some(PoeProblem::Other(_)) | None => None,
        };
        let class = if problem_text.is_some() { "map-edge poe problem" } else { "map-edge poe" };
        let title = t::poe_edge(
            &format!("{} {}", edge.pse.device.name, edge.pse.name),
            &format!("{} {}", edge.pd.device.name, edge.pd.name),
            edge.status.as_deref(),
            edge.power,
            problem_text,
        );
        Some(html! {
            <line
                class={class}
                x1={(pa.0 + offset).to_string()} y1={(pa.1 + offset).to_string()}
                x2={(pb.0 + offset).to_string()} y2={(pb.1 + offset).to_string()}
                marker-end={if problem_text.is_some() { "url(#poe-arrow-problem)" } else { "url(#poe-arrow)" }}
            ><title>{title}</title></line>
        })
    });
    let outside = (state.outside > 0)
        .then(|| positions.get(&OUTSIDE))
        .flatten()
        .map(|p| {
            let lines = state.outside_at.iter().filter_map(|id| positions.get(id)).map(|q| {
                html!(<line class="map-edge outside" x1={p.0.to_string()} y1={p.1.to_string()} x2={q.0.to_string()} y2={q.1.to_string()}/>)
            });
            html! {
                <>
                    {for lines}
                    <g class="map-node outside">
                        <circle cx={p.0.to_string()} cy={p.1.to_string()} r="14"/>
                        <text x={p.0.to_string()} y={(p.1 + 30.0).to_string()}>{t::outside(state.outside as u32)}</text>
                    </g>
                </>
            }
        });
    let nodes = state.nodes.iter().filter_map(|node| {
        let p = positions.get(&node.device.id)?;
        let class = match node.state {
            NodeState::REACHED => "reached",
            NodeState::ISOLATED => "isolated",
            NodeState::NOT_OSPF => "not-ospf",
            _ => "missing",
        };
        let id = node.device.id;
        let site = node.device.site;
        let onclick = scope.callback(move |_| NetworkMapMsg::Open(id, site));
        let onmouseenter = scope.callback(move |_| NetworkMapMsg::Hover(Some(id)));
        let onmouseleave = scope.callback(move |_| NetworkMapMsg::Hover(None));
        let onpointerdown = scope.callback(move |e: web_sys::PointerEvent| {
            e.prevent_default();
            NetworkMapMsg::DragStart(id)
        });
        let shape = if node.routeros {
            html!(<circle cx={p.0.to_string()} cy={p.1.to_string()} r="14"/>)
        } else {
            html!(<rect x={(p.0 - 10.0).to_string()} y={(p.1 - 10.0).to_string()} width="20" height="20"/>)
        };
        Some(html! {
            <g class={format!("map-node {class}")} {onclick} {onmouseenter} {onmouseleave} {onpointerdown}>
                {shape}
                <text x={p.0.to_string()} y={(p.1 + 30.0).to_string()}>{&node.device.name}</text>
            </g>
        })
    });
    let onpointermove = scope.batch_callback(move |e: web_sys::PointerEvent| {
        if !dragging {
            return None;
        }
        // `current_target()` is useless here: Yew delegates listeners to a shared root rather
        // than attaching one per element, so it's some ancestor spanning far more than the
        // svg. `target()` plus `closest("svg")` finds the actual element the viewBox is on.
        let target = e.target()?.dyn_into::<web_sys::Element>().ok()?;
        let svg = target.closest("svg").ok()??;
        let rect = svg.get_bounding_client_rect();
        let scale = if rect.width() > 0.0 { width / rect.width() } else { 1.0 };
        let x = (f64::from(e.client_x()) - rect.left()) * scale;
        let y = (f64::from(e.client_y()) - rect.top()) * scale;
        Some(NetworkMapMsg::DragMove(x, y))
    });
    let onpointerup = scope.batch_callback(move |_: web_sys::PointerEvent| dragging.then_some(NetworkMapMsg::DragEnd));
    let onpointerleave = onpointerup.clone();
    let publish_positions: Vec<(i64, (f64, f64))> =
        state.nodes.iter().filter_map(|n| positions.get(&n.device.id).map(|&p| (n.device.id, p))).collect();
    let onclick_publish = scope.callback(move |_| NetworkMapMsg::Publish(publish_positions.clone()));
    let onclick_reset = scope.callback(|_| NetworkMapMsg::ResetOverrides);
    let publish_status = match publish {
        PublishState::Idle => None,
        PublishState::Publishing => Some(html!(<span>{t::PUBLISHING}</span>)),
        PublishState::Done(count) => Some(html!(<span>{t::published(*count)}</span>)),
        PublishState::Failed(error) => Some(html!(<span class="pf-v6-u-color-danger">{t::publish_failed(error)}</span>)),
    };
    let actions = allow_netbox_write.then(|| {
        html! {
            <div class="map-actions">
                <p>{t::DRAG_HINT}</p>
                <Button variant={ButtonVariant::Secondary} onclick={onclick_reset} disabled={overridden.is_empty()}>{t::RESET}</Button>
                {" "}
                <Button variant={ButtonVariant::Primary} onclick={onclick_publish} disabled={matches!(publish, PublishState::Publishing)}>{t::PUBLISH}</Button>
                {" "}
                {publish_status}
            </div>
        }
    });
    let popup = hover.and_then(|id| {
        let node = state.nodes.iter().find(|n| n.device.id == id)?;
        let p = positions.get(&id)?;
        let text = match node.state {
            NodeState::REACHED => t::NODE_REACHED,
            NodeState::ISOLATED => t::NODE_ISOLATED,
            NodeState::NOT_OSPF => t::NODE_NOT_OSPF,
            _ => t::NODE_MISSING,
        };
        let detail = match details {
            None => return Some(html! {
                <div class="map-popup" style={format!("left:{}px; top:{}px", p.0 + 18.0, p.1 - 10.0)}>
                    <div>{format!("{}: {text}", node.device.name)}</div>
                    <Spinner size={SpinnerSize::Sm} aria_label={t::LOADING.to_string()}/>
                </div>
            }),
            Some(d) => d.nodes.iter().find(|n| n.device.id == id),
        };
        Some(html! {
            <div class="map-popup" style={format!("left:{}px; top:{}px", p.0 + 18.0, p.1 - 10.0)}>
                <div>{format!("{}: {text}", node.device.name)}</div>
                if let Some(detail) = detail {
                    if let Some(r) = &detail.resources {
                        <div>{t::resources(&r.uptime, r.cpu_load, r.free_memory, r.total_memory, r.free_hdd_space, &r.version)}</div>
                    }
                    { for detail.health.iter().map(|h| html!(<div>{t::health_row(&h.name, &h.value, h.unit.as_deref())}</div>)) }
                }
            </div>
        })
    });
    html! {
        <div class="network-map">
            {actions}
            <svg viewBox={format!("0 0 {width} {height}")} width={width.to_string()} {onpointermove} {onpointerup} {onpointerleave}>
                <defs>
                    <marker id="poe-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--pf-t--global--color--status--info--default)"/>
                    </marker>
                    <marker id="poe-arrow-problem" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--pf-t--global--color--status--warning--default)"/>
                    </marker>
                </defs>
                {for site_circles}
                {for edges}
                {for poe_edges}
                {outside}
                {for nodes}
            </svg>
            {popup}
        </div>
    }
}

fn legend() -> Html {
    let edge = |class: &str, text: &str| {
        html! {
            <li>
                <svg width="30" height="10"><line class={format!("map-edge {class}")} x1="0" y1="5" x2="30" y2="5"/></svg>
                {" "}{text.to_string()}
            </li>
        }
    };
    let node = |class: &str, text: &str| {
        html! {
            <li>
                <svg width="30" height="20"><g class={format!("map-node {class}")}><circle cx="15" cy="10" r="7"/></g></svg>
                {" "}{text.to_string()}
            </li>
        }
    };
    html! {
        <details class="network-map-legend">
            <summary>{t::LEGEND}</summary>
            <ul>
                {node("reached", t::NODE_REACHED)}
                {node("isolated", t::NODE_ISOLATED)}
                {node("missing", t::NODE_MISSING)}
                {node("not-ospf", t::NODE_NOT_OSPF)}
                {edge("up", t::EDGE_UP)}
                {edge("v3-down", t::EDGE_V3_DOWN)}
                {edge("down", t::EDGE_DOWN)}
                {edge("not-ospf", t::EDGE_NOT_OSPF)}
                {edge("up radio", t::RADIO)}
                {edge("poe", t::EDGE_POE)}
                {edge("poe problem", t::EDGE_POE_PROBLEM)}
            </ul>
        </details>
    }
}

#[cfg(test)]
mod test;
