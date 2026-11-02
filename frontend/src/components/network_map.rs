//! The network map (`Query.topology.networkState`, `docs/concepts/network-map.md`): devices
//! and NetBox's cables and wireless links as an SVG, coloured by what the OSPF database of one
//! router says; devices without OSPF, and their edges, are left out. Scoped to one site, or
//! without a site to every device of every site. Asked anew every few seconds; reading only.
//! The layout is force-directed (Fruchterman–Reingold, `force_layout`), own implementation, no
//! JS library; the routers NetBox doesn't know are one node, pulled toward the site's routers
//! it borders.
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
            NetworkMap as NetworkMapQuery, NetworkMapDetails as NetworkMapDetailsQuery,
            network_map::{self, MapDownReason, MapEdgeKind, MapEdgeState, NetworkMapTopologyNetworkState as State, NodeState},
            network_map_details::{self, NetworkMapDetailsTopologyNetworkStateDetails as DetailsState, PoeProblem},
        },
        query_authenticated,
    },
    i18n::network_map as t,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner, SpinnerSize};
use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};
use yew::{Component, Context, ContextHandle, Html, Properties, html, html::Scope, platform::time::sleep};
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
}

pub enum NetworkMapMsg {
    Loaded(State),
    DetailsLoaded(DetailsState),
    Error(FrontendError),
    Router(RouterContext<AppRoute>),
    /// the device clicked, and its own site (it may be another one than `site`)
    Open(i64, Option<i64>),
    Hover(Option<i64>),
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
        Self {
            state: None,
            details: None,
            hover: None,
            error: None,
            alive,
            router,
            _router_handle: router_handle,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NetworkMapMsg::Loaded(state) => {
                self.state = Some(state);
                self.error = None;
            }
            NetworkMapMsg::DetailsLoaded(details) => self.details = Some(details),
            NetworkMapMsg::Error(error) => self.error = Some(error),
            NetworkMapMsg::Router(router) => self.router = Some(router),
            NetworkMapMsg::Open(device, site) => {
                if let (Some(router), Some(site)) = (&self.router, site) {
                    router.push(AppRoute::device(site as u32, device as u32));
                }
                return false;
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
                let no_router = if ctx.props().site.is_some() { t::NO_ROUTER } else { t::NO_ROUTER_ALL };
                let source = match (&state.read_from, &state.error) {
                    (Some(device), _) => t::read_from(&device.name, state.age as u32),
                    (None, Some(error)) => t::unreadable(error),
                    (None, None) => no_router.to_string(),
                };
                html! {
                    <>
                        <p>{source}</p>
                        {map(state, &self.details, self.hover, ctx.link())}
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
/// position) doesn't move but still pushes and pulls on the others, so they arrange themselves
/// around it. Displacement per round is capped by a temperature that cools linearly to 0 over
/// `ITERATIONS` rounds, standard to keep the layout from oscillating instead of settling.
/// Deterministic: a node without a pinned position starts at a position hashed from its id, not
/// randomly, so the same graph lays out the same way every time.
const ITERATIONS: u32 = 300;

fn force_layout(nodes: &[i64], edges: &[(i64, i64)], pinned: &HashMap<i64, (f64, f64)>, side: f64) -> HashMap<i64, (f64, f64)> {
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
                let force = k * k / dist;
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

fn map(state: &State, details: &Option<DetailsState>, hover: Option<i64>, scope: &Scope<NetworkMap>) -> Html {
    let (node_ids, edge_pairs) = graph(state);
    let positions = force_layout(&node_ids, &edge_pairs, &HashMap::new(), area_side(node_ids.len()));
    let names = state
        .nodes
        .iter()
        .map(|n| (n.device.id, n.device.name.as_str()))
        .collect::<HashMap<_, _>>();
    let width = positions.values().map(|p| p.0).fold(0.0, f64::max) + MARGIN + 40.0;
    let height = positions.values().map(|p| p.1).fold(0.0, f64::max) + MARGIN;
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
        let shape = if node.routeros {
            html!(<circle cx={p.0.to_string()} cy={p.1.to_string()} r="14"/>)
        } else {
            html!(<rect x={(p.0 - 10.0).to_string()} y={(p.1 - 10.0).to_string()} width="20" height="20"/>)
        };
        Some(html! {
            <g class={format!("map-node {class}")} {onclick} {onmouseenter} {onmouseleave}>
                {shape}
                <text x={p.0.to_string()} y={(p.1 + 30.0).to_string()}>{&node.device.name}</text>
            </g>
        })
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
            <svg viewBox={format!("0 0 {width} {height}")} width={width.to_string()}>
                <defs>
                    <marker id="poe-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--pf-t--global--color--status--info--default)"/>
                    </marker>
                    <marker id="poe-arrow-problem" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--pf-t--global--color--status--warning--default)"/>
                    </marker>
                </defs>
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
