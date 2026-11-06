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
            network_map::{
                DevicePositionInput, MapDownReason, MapEdgeKind, MapEdgeState, MapVariables,
                NetworkMap as NetworkMapQuery, NetworkMapDetails as NetworkMapDetailsQuery,
                NetworkState as State, NetworkStateDetails as DetailsState, NodeState,
                PublishLayout as PublishLayoutQuery, PublishLayoutVariables,
            },
            types::PoeProblem,
        },
        request,
    },
    i18n::network_map as t,
    local_storage,
    pages::routes::AppRoute,
    svg_export,
};
use cynic::{MutationBuilder, QueryBuilder};
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
use wasm_bindgen::JsCast;
use yew::{
    Callback, Component, Context, ContextHandle, Html, NodeRef, Properties, html,
    html::Scope,
    platform::{spawn_local, time::sleep},
};
use yew_nested_router::prelude::RouterContext;

/// How often the map asks (the backend reads the LSDB at most this often too, `LSDB_AGE`).
const POLL: Duration = Duration::from_secs(5);
const MARGIN: f64 = 70.0;
/// the node standing for the routers NetBox doesn't know
const OUTSIDE: i32 = -1;

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
    hover: Option<i32>,
    error: Option<FrontendError>,
    alive: Rc<Cell<bool>>,
    router: Option<RouterContext<AppRoute>>,
    _router_handle: Option<ContextHandle<RouterContext<AppRoute>>>,
    allow_netbox_write: bool,
    /// a node the user dragged in this browser (`localStorage`, key [`storage_key`]), on top of
    /// NetBox's published positions; cleared by `ResetOverrides`, written to `localStorage` once
    /// a drag ends. Dragging a whole site's circle writes every one of its members here too, by
    /// device id, same as dragging one of them individually would.
    overridden: HashMap<i32, (f64, f64)>,
    /// a site's circle, dragged to this center instead of wherever `clustered_layout` puts it;
    /// by site id, stored and reset alongside `overridden` (same key, see [`Overrides`]). Devices
    /// have no circle of their own to carry this, so it can't reuse `overridden` the way dragging
    /// a site's members already does.
    site_overrides: HashMap<i32, (f64, f64)>,
    /// what's currently grabbed, and whether it actually moved yet (a plain click, pointer down
    /// and up without a move between, must still navigate as before)
    dragging: Option<(DragTarget, bool)>,
    /// every affected device's position at the moment of `DragStart` (one device, or every
    /// member of a dragged site), and the circle's own center for a dragged site: each
    /// `DragMove` recomputes a fresh absolute position from these plus the total pointer
    /// movement since `drag_origin`, rather than accumulating a delta onto the last message's
    /// own result. The latter looked right but wasn't: the closures in `map()` that compute a
    /// move only see a snapshot taken at the last *render*, and the raw DOM can fire more than
    /// one `pointermove` before Yew gets back around to building a fresh one, so the second of
    /// two such events would add its step onto a position `map()` hadn't caught up to yet —
    /// losing part of the movement. Recomputing from a fixed origin every time is immune to how
    /// many events land between renders, or in what order they're drained.
    drag_base: HashMap<i32, (f64, f64)>,
    drag_base_site: Option<(i32, (f64, f64))>,
    /// the pointer's own SVG-space position at the first `DragMove` after `DragStart`, the
    /// origin `drag_base`'s positions move from; `None` until that first move sets it
    drag_origin: Option<(f64, f64)>,
    /// set by `DragEnd` when the drag moved its target, consumed by the next `Open`: the pointer
    /// up after a drag fires a click too, which must not also navigate away from the map
    suppress_click: bool,
    publish: PublishState,
    /// the rendered `<svg>` itself, for `svg_export` to read on an export click
    svg_ref: NodeRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragTarget {
    Device(i32),
    /// every device of this site moves together, same delta
    Site(i32),
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
    Open(i32, Option<i32>),
    Hover(Option<i32>),
    AllowNetboxWrite(bool),
    /// the target, its affected devices' current positions (one, or every member of a site),
    /// and, dragging a site, its circle's current center: `drag_base`/`drag_base_site`, read
    /// fresh from a render that just happened, before any native event can get ahead of Yew's
    /// own one (unlike a `DragMove` in the middle of a drag, `DragStart` is a single discrete
    /// event, not a rapid-fire stream, so a render-time snapshot taken right here is never stale)
    DragStart(
        DragTarget,
        Vec<(i32, (f64, f64))>,
        Option<(i32, (f64, f64))>,
    ),
    /// the pointer's new SVG-space position, nothing else: every `DragMove` recomputes the full
    /// move from `drag_base`/`drag_origin`, so the closure that sends this needs to carry no
    /// drag-specific state of its own (see `drag_base`'s own doc comment for why)
    DragMove(f64, f64),
    DragEnd,
    ResetOverrides,
    Publish(Vec<(i32, (f64, f64))>),
    Published(Result<u32, FrontendError>),
}

/// Where a browser's own, unpublished arrangement is kept (`local_storage`): scoped to the page
/// (a site's map, or the all-sites one), since the two use unrelated coordinate spaces.
fn storage_key(site: Option<u32>) -> String {
    format!(
        "network-map-positions:{}",
        site.map(|s| s.to_string()).unwrap_or_else(|| "all".into())
    )
}

/// What [`storage_key`] holds, as one JSON object: a device's own dragged position, and a
/// site's circle's.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Overrides {
    #[serde(default)]
    devices: HashMap<i32, (f64, f64)>,
    #[serde(default)]
    sites: HashMap<i32, (f64, f64)>,
}

fn load_overrides(site: Option<u32>) -> Overrides {
    local_storage::stored(&storage_key(site))
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
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
        let Overrides {
            devices: overridden,
            sites: site_overrides,
        } = load_overrides(ctx.props().site);
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
            site_overrides,
            dragging: None,
            drag_base: HashMap::new(),
            drag_base_site: None,
            drag_origin: None,
            suppress_click: false,
            publish: PublishState::default(),
            svg_ref: NodeRef::default(),
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
            NetworkMapMsg::DragStart(target, bases, site_base) => {
                self.dragging = Some((target, false));
                self.drag_base = bases.into_iter().collect();
                self.drag_base_site = site_base;
                self.drag_origin = None;
            }
            NetworkMapMsg::DragMove(x, y) => {
                let Some((target, _)) = self.dragging else {
                    return false;
                };
                if let DragTarget::Device(id) = target {
                    // a device snaps directly under the pointer (direct following, like
                    // Neo4j's graph view: the node IS the dragged point, no offset to preserve)
                    // rather than keeping wherever on it the drag started; unlike a site's rigid
                    // group move below, there's nothing else that needs a stable delta from a
                    // fixed origin for this.
                    if self.drag_base.get(&id).copied() == Some((x, y)) {
                        return false;
                    }
                    self.dragging = Some((target, true));
                    self.overridden.insert(id, (x, y));
                    return true;
                }
                let Some((ox, oy)) = self.drag_origin else {
                    self.drag_origin = Some((x, y));
                    return false;
                };
                let (dx, dy) = (x - ox, y - oy);
                if dx == 0.0 && dy == 0.0 {
                    return false;
                }
                self.dragging = Some((target, true));
                for (&id, &(bx, by)) in &self.drag_base {
                    self.overridden.insert(id, (bx + dx, by + dy));
                }
                if let Some((site, (bx, by))) = self.drag_base_site {
                    self.site_overrides.insert(site, (bx + dx, by + dy));
                }
            }
            NetworkMapMsg::DragEnd => {
                let Some((_, moved)) = self.dragging.take() else {
                    return false;
                };
                self.drag_base.clear();
                self.drag_base_site = None;
                self.drag_origin = None;
                if moved {
                    self.suppress_click = true;
                    let overrides = Overrides {
                        devices: self.overridden.clone(),
                        sites: self.site_overrides.clone(),
                    };
                    if let Ok(json) = serde_json::to_string(&overrides) {
                        local_storage::store(&storage_key(ctx.props().site), &json);
                    }
                }
            }
            NetworkMapMsg::ResetOverrides => {
                self.overridden.clear();
                self.site_overrides.clear();
                local_storage::remove(&storage_key(ctx.props().site));
            }
            NetworkMapMsg::Publish(positions) => {
                self.publish = PublishState::Publishing;
                let scope = ctx.link().clone();
                spawn_local(async move {
                    let variables = PublishLayoutVariables {
                        positions: positions
                            .into_iter()
                            .map(|(device_id, (x, y))| DevicePositionInput {
                                device_id,
                                x: x.round() as i32,
                                y: y.round() as i32,
                            })
                            .collect(),
                    };
                    let result = request(scope.clone(), PublishLayoutQuery::build(variables))
                        .await
                        .map(|data| data.publish_layout as u32);
                    scope.send_message(NetworkMapMsg::Published(result));
                });
            }
            NetworkMapMsg::Published(Ok(count)) => {
                self.publish = PublishState::Done(count);
                self.overridden.clear();
                self.site_overrides.clear();
                local_storage::remove(&storage_key(ctx.props().site));
            }
            NetworkMapMsg::Published(Err(error)) => {
                self.publish = PublishState::Failed(error.to_string())
            }
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
                let no_router = if ctx.props().site.is_some() {
                    t::NO_ROUTER
                } else {
                    t::NO_ROUTER_ALL
                };
                let source = match (&state.read_from, &state.error) {
                    (Some(device), _) => t::read_from(&device.name, state.age as u32),
                    (None, Some(error)) => t::unreadable(error),
                    (None, None) => no_router.to_string(),
                };
                let interaction = Interaction {
                    overridden: &self.overridden,
                    site_overrides: &self.site_overrides,
                    allow_netbox_write: self.allow_netbox_write,
                    publish: &self.publish,
                    svg_ref: &self.svg_ref,
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
        let intro = if ctx.props().site.is_some() {
            t::INTRO
        } else {
            t::INTRO_ALL
        };
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
            let variables = MapVariables {
                site: site.map(|s| s as i32),
            };
            match request(scope.clone(), NetworkMapQuery::build(variables)).await {
                Ok(data) => {
                    failed = 0;
                    let state = data.topology.network_state;
                    if alive.get() {
                        scope
                            .send_message(NetworkMapMsg::AllowNetboxWrite(data.allow_netbox_write));
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
            let variables = MapVariables {
                site: site.map(|s| s as i32),
            };
            match request(scope.clone(), NetworkMapDetailsQuery::build(variables)).await {
                Ok(data) => {
                    failed = 0;
                    if alive.get() {
                        scope.send_message(NetworkMapMsg::DetailsLoaded(
                            data.topology.network_state_details,
                        ));
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
fn graph(state: &State) -> (Vec<i32>, Vec<(i32, i32)>) {
    let mut nodes: Vec<i32> = state.nodes.iter().map(|n| n.device.id).collect();
    let mut edges: Vec<(i32, i32)> = state
        .edges
        .iter()
        .map(|e| (e.a.device.id, e.b.device.id))
        .collect();
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
/// oscillating instead of settling. A node that's neither pinned nor in `seed` starts at a
/// position hashed from its id, not randomly, so the same graph lays out the same way every
/// time; `seed` (typically [`seed_from_graph`]) gives the physics a graph-structure-aware head
/// start instead, still free to move from there, unlike `pinned`.
const ITERATIONS: u32 = 300;

fn force_layout(
    nodes: &[i32],
    edges: &[(i32, i32)],
    pinned: &HashMap<i32, (f64, f64)>,
    radii: &HashMap<i32, f64>,
    gap: f64,
    side: f64,
    seed: &HashMap<i32, (f64, f64)>,
) -> HashMap<i32, (f64, f64)> {
    if nodes.is_empty() {
        return HashMap::new();
    }
    let k = (side * side / nodes.len() as f64).sqrt();
    let mut pos: HashMap<i32, (f64, f64)> = nodes
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
        let mut disp = HashMap::<i32, (f64, f64)>::new();
        for (i, &a) in nodes.iter().enumerate() {
            for &b in &nodes[i + 1..] {
                let (dx, dy) = (pos[&a].0 - pos[&b].0, pos[&a].1 - pos[&b].1);
                let dist = dx.hypot(dy).max(0.01);
                let effective_k = (radii.get(&a).copied().unwrap_or(0.0)
                    + radii.get(&b).copied().unwrap_or(0.0)
                    + gap)
                    .max(k);
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
            let (Some(&pa), Some(&pb)) = (pos.get(&a), pos.get(&b)) else {
                continue;
            };
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
fn seeded(id: i32, side: f64) -> (f64, f64) {
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
fn seed_from_graph(nodes: &[i32], edges: &[(i32, i32)], side: f64) -> HashMap<i32, (f64, f64)> {
    let mut adjacency: HashMap<i32, Vec<i32>> = HashMap::new();
    for &(a, b) in edges {
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
                pos.insert(id, (x, depth as f64 * spacing));
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
enum Cluster {
    Site(i32),
    Alone(i32),
}

/// A site's devices, grouped into their own circle.
struct SiteCircle {
    site: i32,
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
    let (o1, o2, o3, o4) = (
        orientation(p1, p2, p3),
        orientation(p1, p2, p4),
        orientation(p3, p4, p1),
        orientation(p3, p4, p2),
    );
    o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0 && o1 != o2 && o3 != o4
}

fn count_crossings<K: Eq + std::hash::Hash + Copy>(
    positions: &HashMap<K, (f64, f64)>,
    edges: &[(K, K)],
) -> usize {
    let mut n = 0;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let (a1, b1) = edges[i];
            let (a2, b2) = edges[j];
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
fn reduce_crossings<K: Eq + std::hash::Hash + Ord + Copy>(
    positions: &mut HashMap<K, (f64, f64)>,
    edges: &[(K, K)],
) {
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

/// The point on segment `p1`-`p2` closest to `center` is nearer than `radius`: the segment passes
/// through the circle, not just near it — touching the boundary exactly (`==`) doesn't count, same
/// convention as [`segments_cross`] treating a shared endpoint as "meeting there," not a crossing.
fn segment_intersects_circle(
    p1: (f64, f64),
    p2: (f64, f64),
    center: (f64, f64),
    radius: f64,
) -> bool {
    let (dx, dy) = (p2.0 - p1.0, p2.1 - p1.1);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((center.0 - p1.0) * dx + (center.1 - p1.1) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (p1.0 + t * dx, p1.1 + t * dy);
    (cx - center.0).hypot(cy - center.1) < radius
}

/// How many of `edges`, drawn at `positions`, pass through a site circle neither of their own two
/// endpoints belongs to — reads as if it connects to that site too, confusing regardless of
/// whether it also crosses another edge on the way, so [`reduce_site_crossings`] weighs it the
/// same as an actual crossing.
fn count_intrusions(
    positions: &HashMap<i32, (f64, f64)>,
    edges: &[(i32, i32)],
    cluster_of_node: &HashMap<i32, Cluster>,
    centers: &HashMap<Cluster, (f64, f64)>,
    radii: &HashMap<Cluster, f64>,
) -> usize {
    let mut n = 0;
    for &(a, b) in edges {
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
fn reduce_site_crossings(
    positions: &mut HashMap<i32, (f64, f64)>,
    centers: &mut HashMap<Cluster, (f64, f64)>,
    cluster_of_node: &HashMap<i32, Cluster>,
    members: &BTreeMap<Cluster, Vec<i32>>,
    edges: &[(i32, i32)],
    radii: &HashMap<Cluster, f64>,
) {
    let offsets: HashMap<i32, (f64, f64)> = positions
        .iter()
        .map(|(&id, &(x, y))| {
            let center = centers[&cluster_of_node[&id]];
            (id, (x - center.0, y - center.1))
        })
        .collect();
    let badness = |positions: &HashMap<i32, (f64, f64)>, centers: &HashMap<Cluster, (f64, f64)>| {
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
                    positions.insert(
                        id,
                        (center_j.0 + offsets[&id].0, center_j.1 + offsets[&id].1),
                    );
                }
                for &id in &members[&cj] {
                    positions.insert(
                        id,
                        (center_i.0 + offsets[&id].0, center_i.1 + offsets[&id].1),
                    );
                }
                centers.insert(ci, center_j);
                centers.insert(cj, center_i);
                if badness(positions, centers) < before {
                    improved = true;
                } else {
                    for &id in &members[&ci] {
                        positions.insert(
                            id,
                            (center_i.0 + offsets[&id].0, center_i.1 + offsets[&id].1),
                        );
                    }
                    for &id in &members[&cj] {
                        positions.insert(
                            id,
                            (center_j.0 + offsets[&id].0, center_j.1 + offsets[&id].1),
                        );
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
fn clustered_layout(
    state: &State,
    pinned: &HashMap<i32, (f64, f64)>,
) -> (HashMap<i32, (f64, f64)>, Vec<SiteCircle>) {
    let (node_ids, edge_pairs) = graph(state);
    if node_ids.is_empty() {
        return (HashMap::new(), Vec::new());
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
    let meta_edges: Vec<(i32, i32)> = edge_pairs
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
    let mut meta_positions: HashMap<Cluster, (f64, f64)> = clusters
        .iter()
        .map(|&c| (c, meta_positions_by_id[&meta_id_of[&c]]))
        .collect();
    reduce_crossings(
        &mut meta_positions,
        &meta_edges
            .iter()
            .map(|&(a, b)| (clusters[a as usize], clusters[b as usize]))
            .collect::<Vec<_>>(),
    );

    let mut positions = HashMap::new();
    let mut circles = Vec::new();
    let mut circle_members: Vec<Vec<i32>> = Vec::new();
    for (&cluster, ids) in &groups {
        let center = meta_positions[&cluster];
        let promised_radius = radius_of(cluster);
        // the recenter+shrink step turns a local layout (around `local_center`, in `side`-sized
        // local coordinates) into the final, absolute one (around `center`, within
        // `promised_radius`): called once below for the plain case, and, where a device of this
        // site is pinned (dragged, live or settled), a second time so the others can be laid out
        // reacting to it rather than just having it stamped on top afterwards.
        let recenter =
            |local: &HashMap<i32, (f64, f64)>| -> (HashMap<i32, (f64, f64)>, f64, f64, f64) {
                let (sx, sy) = local
                    .values()
                    .fold((0.0, 0.0), |(sx, sy), p| (sx + p.0, sy + p.1));
                let n = local.len() as f64;
                let (dx, dy) = (center.0 - sx / n, center.1 - sy / n);
                let mut radius = 0.0f64;
                let mut recentered = HashMap::with_capacity(local.len());
                for (&id, &(x, y)) in local {
                    let p = (x + dx, y + dy);
                    radius = radius.max((p.0 - center.0).hypot(p.1 - center.1));
                    recentered.insert(id, p);
                }
                // the anchors below pull devices toward wherever their external neighbors are, which
                // can spread them wider than a plain internal-only layout would — further than
                // `promised_radius` (what the meta layout, above, already reserved this site against
                // every other site's own radius): shrink evenly around the center to fit back within
                // it, rather than let two sites whose circles the meta layout kept apart end up
                // overlapping anyway once each is actually drawn at its real size.
                let shrink = if radius > promised_radius {
                    promised_radius / radius
                } else {
                    1.0
                };
                let out = recentered
                    .into_iter()
                    .map(|(id, (x, y))| {
                        (
                            id,
                            (
                                center.0 + (x - center.0) * shrink,
                                center.1 + (y - center.1) * shrink,
                            ),
                        )
                    })
                    .collect();
                (out, dx, dy, shrink)
            };
        let final_local = if ids.len() > 1 {
            let internal_edges = edge_pairs
                .iter()
                .copied()
                .filter(|&(a, b)| cluster_of_node[&a] == cluster && cluster_of_node[&b] == cluster)
                .collect::<Vec<_>>();
            // a device cabled to another site is pulled toward that site's direction too, by a
            // fixed point well outside the local area in that direction (pinned, so it never
            // moves itself) — otherwise devices would lay out from their internal cables alone,
            // regardless of which side of the circle actually faces the site they also connect
            // to, making that connection longer than it has to be and more likely to cross
            // another site's circle on the way.
            let side = area_side(ids.len());
            let local_center = (side / 2.0, side / 2.0);
            let mut anchor_of_cluster = HashMap::<Cluster, i32>::new();
            let mut anchors = HashMap::<i32, (f64, f64)>::new();
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
                let Some(&other_center) = meta_positions.get(&other) else {
                    continue;
                };
                let anchor_id = *anchor_of_cluster
                    .entry(other)
                    .or_insert_with(|| -1_000_000 - anchors.len() as i32);
                anchors.entry(anchor_id).or_insert_with(|| {
                    let (dx, dy) = (other_center.0 - center.0, other_center.1 - center.1);
                    let dist = dx.hypot(dy).max(0.01);
                    (
                        local_center.0 + dx / dist * side,
                        local_center.1 + dy / dist * side,
                    )
                });
                anchor_edges.push((local_end, anchor_id));
            }
            let all_ids: Vec<i32> = ids.iter().copied().chain(anchors.keys().copied()).collect();
            let all_edges: Vec<(i32, i32)> =
                internal_edges.iter().copied().chain(anchor_edges).collect();
            let compute_local =
                |extra_pins: &HashMap<i32, (f64, f64)>| -> HashMap<i32, (f64, f64)> {
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
                    let mut local: HashMap<i32, (f64, f64)> =
                        ids.iter().map(|&id| (id, raw[&id])).collect();
                    // untangle the devices' own cabling within the circle too, same local search as
                    // the meta layout above — the anchors already bias which side of the circle a
                    // device ends up on, this only straightens out crossings among the internal
                    // cables
                    reduce_crossings(&mut local, &internal_edges);
                    local
                };
            let (local, dx, dy, shrink) = recenter(&compute_local(&HashMap::new()));
            let pinned_here: HashMap<i32, (f64, f64)> = ids
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
                let local_pins: HashMap<i32, (f64, f64)> = pinned_here
                    .iter()
                    .map(|(&id, &(x, y))| {
                        (
                            id,
                            (
                                (x - center.0) / shrink + center.0 - dx,
                                (y - center.1) / shrink + center.1 - dy,
                            ),
                        )
                    })
                    .collect();
                recenter(&compute_local(&local_pins)).0
            }
        } else {
            recenter(&HashMap::from([(ids[0], center)])).0
        };
        for (id, pos) in final_local {
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
    for c in &mut circles {
        c.center = meta_positions[&Cluster::Site(c.site)];
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
                    circles[j].center.0 - circles[i].center.0,
                    circles[j].center.1 - circles[i].center.1,
                );
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
    let min_x = positions
        .values()
        .map(|p| p.0)
        .chain(circles.iter().map(|c| c.center.0 - c.radius))
        .fold(f64::INFINITY, f64::min);
    let min_y = positions
        .values()
        .map(|p| p.1)
        .chain(circles.iter().map(|c| c.center.1 - c.radius))
        .fold(f64::INFINITY, f64::min);
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
    overridden: &'a HashMap<i32, (f64, f64)>,
    site_overrides: &'a HashMap<i32, (f64, f64)>,
    allow_netbox_write: bool,
    publish: &'a PublishState,
    svg_ref: &'a NodeRef,
}

fn map(
    state: &State,
    details: &Option<DetailsState>,
    hover: Option<i32>,
    interaction: &Interaction,
    scope: &Scope<NetworkMap>,
) -> Html {
    let overridden = interaction.overridden;
    let site_overrides = interaction.site_overrides;
    let allow_netbox_write = interaction.allow_netbox_write;
    let publish = interaction.publish;
    let svg_ref = interaction.svg_ref;
    // NetBox's published layout, overlaid by whatever the user dragged in this browser (wins
    // where both exist); fed into the layout below as pinned positions, so other nodes still
    // arrange themselves around a pinned one ([`force_layout`]'s own doc comment).
    let mut pinned: HashMap<i32, (f64, f64)> = state
        .nodes
        .iter()
        .filter_map(|n| {
            n.position
                .as_ref()
                .map(|p| (n.device.id, (p.x as f64, p.y as f64)))
        })
        .collect();
    pinned.extend(overridden.iter());
    let site_count = state
        .nodes
        .iter()
        .filter_map(|n| n.device.site)
        .collect::<HashSet<_>>()
        .len();
    let (mut positions, mut circles) = if site_count > 1 {
        clustered_layout(state, &pinned)
    } else {
        let (node_ids, edge_pairs) = graph(state);
        // `pinned` holds the desired final rendered position, but `force_layout` adds `MARGIN`
        // to every position, pinned ones included, right before returning it; undo that here so
        // a pin actually lands where it says, rather than MARGIN further out.
        let pinned_for_layout: HashMap<i32, (f64, f64)> = pinned
            .iter()
            .map(|(&id, &(x, y))| (id, (x - MARGIN, y - MARGIN)))
            .collect();
        (
            force_layout(
                &node_ids,
                &edge_pairs,
                &pinned_for_layout,
                &HashMap::new(),
                0.0,
                area_side(node_ids.len()),
                &HashMap::new(),
            ),
            Vec::new(),
        )
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
        // the circle itself has no device id to carry an override on, so unlike a device's it
        // can't ride along in `pinned`/`positions` above; applied here instead, straight onto
        // the one place its position is actually drawn from.
        for c in &mut circles {
            if let Some(&pos) = site_overrides.get(&c.site) {
                c.center = pos;
            }
        }
    }
    let names = state
        .nodes
        .iter()
        .map(|n| (n.device.id, n.device.name.as_str()))
        .collect::<HashMap<_, _>>();
    let width = positions
        .values()
        .map(|p| p.0)
        .chain(circles.iter().map(|c| c.center.0 + c.radius))
        .fold(0.0, f64::max)
        + MARGIN
        + 40.0;
    let height = positions
        .values()
        .map(|p| p.1)
        .chain(circles.iter().map(|c| c.center.1 + c.radius))
        .fold(0.0, f64::max)
        + MARGIN;
    // every device of a site, for a dragged site's circle to carry them all along as its
    // `DragStart` baseline
    let mut site_members = HashMap::<i32, Vec<i32>>::new();
    for n in &state.nodes {
        if let Some(site) = n.device.site {
            site_members.entry(site).or_default().push(n.device.id);
        }
    }
    let site_circles = circles.iter().map(|c| {
        let site = c.site;
        let center = c.center;
        let bases: Vec<(i32, (f64, f64))> =
            site_members.get(&site).into_iter().flatten().filter_map(|&id| Some((id, positions.get(&id).copied()?))).collect();
        let onpointerdown = scope.callback(move |e: web_sys::PointerEvent| {
            e.prevent_default();
            NetworkMapMsg::DragStart(DragTarget::Site(site), bases.clone(), Some((site, center)))
        });
        html! {
            <g class="map-site" {onpointerdown}>
                <circle cx={c.center.0.to_string()} cy={c.center.1.to_string()} r={c.radius.to_string()}/>
                <text x={c.center.0.to_string()} y={(c.center.1 - c.radius - 8.0).to_string()}>{&c.name}</text>
            </g>
        }
    });
    // parallel edges between the same two devices side by side
    let mut parallel = HashMap::<(i32, i32), f64>::new();
    let edges = state.edges.iter().filter_map(|edge| {
        let (a, b) = (edge.a.device.id, edge.b.device.id);
        let (pa, pb) = (positions.get(&a)?, positions.get(&b)?);
        let index = parallel.entry((a.min(b), a.max(b))).or_default();
        let offset = *index * 6.0;
        *index += 1.0;
        let class = match edge.state {
            MapEdgeState::Up => "up",
            MapEdgeState::V3Down => "v3-down",
            MapEdgeState::Down => "down",
            _ => "not-ospf",
        };
        let radio = if edge.kind == MapEdgeKind::Radio {
            " radio"
        } else {
            ""
        };
        let name = |id| names.get(&id).copied().unwrap_or_default();
        let state_text = match edge.state {
            MapEdgeState::Up => t::EDGE_UP.to_string(),
            MapEdgeState::V3Down => t::EDGE_V3_DOWN.to_string(),
            MapEdgeState::Down => {
                let device = edge.reason_device.map(name).unwrap_or_default();
                match edge.reason {
                    Some(MapDownReason::RouterMissing) => t::router_missing(device),
                    Some(MapDownReason::NotOnSegment) => t::not_on_segment(device),
                    Some(MapDownReason::NoSegment) => t::NO_SEGMENT.to_string(),
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
            if radio.is_empty() {
                String::new()
            } else {
                format!(" ({})", t::RADIO)
            },
        );
        Some(html! {
            <line
                class={format!("map-edge {class}{radio}")}
                x1={(pa.0 + offset).to_string()} y1={(pa.1 + offset).to_string()}
                x2={(pb.0 + offset).to_string()} y2={(pb.1 + offset).to_string()}
            ><title>{title}</title></line>
        })
    });
    let mut parallel_poe = HashMap::<(i32, i32), f64>::new();
    let poe_edges = details.iter().flat_map(|d| &d.poe_edges).filter_map(|edge| {
        let (a, b) = (edge.pse.device.id, edge.pd.device.id);
        let (pa, pb) = (positions.get(&a)?, positions.get(&b)?);
        let index = parallel_poe.entry((a.min(b), a.max(b))).or_default();
        let offset = *index * 6.0;
        *index += 1.0;
        let problem_text = match &edge.problem {
            Some(PoeProblem::NoPoeOut) => Some(t::POE_NO_POE_OUT),
            Some(PoeProblem::NoLoad) => Some(t::POE_NO_LOAD),
            Some(PoeProblem::PdMissing) => Some(t::POE_PD_MISSING),
            Some(PoeProblem::FeedsInstead) => Some(t::POE_FEEDS_INSTEAD),
            Some(PoeProblem::Undocumented) => Some(t::POE_UNDOCUMENTED),
            Some(PoeProblem::Other) | None => None,
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
            NodeState::Reached => "reached",
            NodeState::Isolated => "isolated",
            NodeState::NotOspf => "not-ospf",
            _ => "missing",
        };
        let id = node.device.id;
        let site = node.device.site;
        let onclick = scope.callback(move |_| NetworkMapMsg::Open(id, site));
        let onmouseenter = scope.callback(move |_| NetworkMapMsg::Hover(Some(id)));
        let onmouseleave = scope.callback(move |_| NetworkMapMsg::Hover(None));
        let base = *p;
        let onpointerdown = scope.callback(move |e: web_sys::PointerEvent| {
            e.prevent_default();
            NetworkMapMsg::DragStart(DragTarget::Device(id), vec![(id, base)], None)
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
    // Only resolves the pointer's own SVG-space position; `update()` does the rest (see
    // `drag_base`'s doc comment for why the delta math itself doesn't belong in this closure).
    let onpointermove = scope.batch_callback(move |e: web_sys::PointerEvent| {
        // `current_target()` is useless here: Yew delegates listeners to a shared root rather
        // than attaching one per element, so it's some ancestor spanning far more than the
        // svg. `target()` plus `closest("svg")` finds the actual element the viewBox is on.
        let element = e.target()?.dyn_into::<web_sys::Element>().ok()?;
        let svg = element.closest("svg").ok()??;
        let rect = svg.get_bounding_client_rect();
        let scale = if rect.width() > 0.0 {
            width / rect.width()
        } else {
            1.0
        };
        let x = (f64::from(e.client_x()) - rect.left()) * scale;
        let y = (f64::from(e.client_y()) - rect.top()) * scale;
        Some(NetworkMapMsg::DragMove(x, y))
    });
    let onpointerup = scope.callback(move |_: web_sys::PointerEvent| NetworkMapMsg::DragEnd);
    let onpointerleave = onpointerup.clone();
    let publish_positions: Vec<(i32, (f64, f64))> = state
        .nodes
        .iter()
        .filter_map(|n| positions.get(&n.device.id).map(|&p| (n.device.id, p)))
        .collect();
    let onclick_publish =
        scope.callback(move |_| NetworkMapMsg::Publish(publish_positions.clone()));
    let onclick_reset = scope.callback(|_| NetworkMapMsg::ResetOverrides);
    let publish_status = match publish {
        PublishState::Idle => None,
        PublishState::Publishing => Some(html!(<span>{t::PUBLISHING}</span>)),
        PublishState::Done(count) => Some(html!(<span>{t::published(*count)}</span>)),
        PublishState::Failed(error) => {
            Some(html!(<span class="pf-v6-u-color-danger">{t::publish_failed(error)}</span>))
        }
    };
    let actions = allow_netbox_write.then(|| {
        html! {
            <div class="map-actions">
                <p>{t::DRAG_HINT}</p>
                <Button variant={ButtonVariant::Secondary} onclick={onclick_reset} disabled={overridden.is_empty() && site_overrides.is_empty()}>{t::RESET}</Button>
                {" "}
                <Button variant={ButtonVariant::Primary} onclick={onclick_publish} disabled={matches!(publish, PublishState::Publishing)}>{t::PUBLISH}</Button>
                {" "}
                {publish_status}
            </div>
        }
    });
    // reads the already-rendered `<svg>` (`svg_ref`) straight off the DOM, so this needs no
    // state or message of its own, unlike the drag/publish actions above — just a side effect
    // on click, same way a plain HTML page would wire a download button.
    let onclick_export_svg = {
        let svg_ref = svg_ref.clone();
        Callback::from(move |_| svg_export::download_svg(&svg_ref, "netzwerkkarte.svg"))
    };
    let onclick_export_png = {
        let svg_ref = svg_ref.clone();
        Callback::from(move |_| {
            let svg_ref = svg_ref.clone();
            spawn_local(
                async move { svg_export::download_png(&svg_ref, "netzwerkkarte.png").await },
            );
        })
    };
    let export_actions = html! {
        <div class="map-actions">
            <Button variant={ButtonVariant::Secondary} onclick={onclick_export_svg}>{t::EXPORT_SVG}</Button>
            {" "}
            <Button variant={ButtonVariant::Secondary} onclick={onclick_export_png}>{t::EXPORT_PNG}</Button>
        </div>
    };
    let popup = hover.and_then(|id| {
        let node = state.nodes.iter().find(|n| n.device.id == id)?;
        let p = positions.get(&id)?;
        let text = match node.state {
            NodeState::Reached => t::NODE_REACHED,
            NodeState::Isolated => t::NODE_ISOLATED,
            NodeState::NotOspf => t::NODE_NOT_OSPF,
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
                        <div>{t::resources(
                            &r.uptime,
                            r.cpu_load,
                            r.free_memory.0,
                            r.total_memory.0,
                            r.free_hdd_space.0,
                            &r.version,
                        )}</div>
                    }
                    { for detail.health.iter().map(|h| html!(<div>{t::health_row(&h.name, &h.value, h.unit.as_deref())}</div>)) }
                }
            </div>
        })
    });
    html! {
        <div class="network-map">
            {actions}
            {export_actions}
            <svg ref={svg_ref.clone()} viewBox={format!("0 0 {width} {height}")} width={width.to_string()} {onpointermove} {onpointerup} {onpointerleave}>
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
