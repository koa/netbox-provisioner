//! Scope: The network map as SVG with its nodes, edges and dragging; not its layout (`layout`).
//!
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
    error::FrontendError,
    graphql::{
        RETRY,
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
    routes::AppRoute,
    svg_export,
};
use cynic::{MutationBuilder, QueryBuilder};
use layout::*;
use patternfly_yew::prelude::{
    Button, ButtonVariant, Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize,
};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
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

mod layout;
#[cfg(test)]
mod test;
