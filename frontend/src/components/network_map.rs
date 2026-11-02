//! The network map (`Query.topology.networkState`, `docs/concepts/network-map.md`): devices
//! and NetBox's cables and wireless links as an SVG, coloured by what the OSPF database of one
//! router says; devices without OSPF, and their edges, are left out. Scoped to one site, or
//! without a site to every device of every site. Asked anew every few seconds; reading only.
//! The layout is fixed for now: columns by hops from the router read, the routers NetBox
//! doesn't know as one node before the routers they come in at.
use crate::{
    components::job::RETRY,
    error::FrontendError,
    graphql::{
        authenticated::{
            NetworkMap as NetworkMapQuery,
            network_map::{
                self, MapDownReason, MapEdgeKind, MapEdgeState, NetworkMapTopologyNetworkState as State, NodeState,
            },
        },
        query_authenticated,
    },
    i18n::network_map as t,
    pages::routes::AppRoute,
};
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, VecDeque},
    rc::Rc,
    time::Duration,
};
use yew::{Component, Context, ContextHandle, Html, Properties, html, html::Scope, platform::time::sleep};
use yew_nested_router::prelude::RouterContext;

/// How often the map asks (the backend reads the LSDB at most this often too, `LSDB_AGE`).
const POLL: Duration = Duration::from_secs(5);
const COLUMN: f64 = 170.0;
const ROW: f64 = 80.0;
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
    error: Option<FrontendError>,
    alive: Rc<Cell<bool>>,
    router: Option<RouterContext<AppRoute>>,
    _router_handle: Option<ContextHandle<RouterContext<AppRoute>>>,
}

pub enum NetworkMapMsg {
    Loaded(State),
    Error(FrontendError),
    Router(RouterContext<AppRoute>),
    /// the device clicked, and its own site (it may be another one than `site`)
    Open(i64, Option<i64>),
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
        Self {
            state: None,
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
            NetworkMapMsg::Error(error) => self.error = Some(error),
            NetworkMapMsg::Router(router) => self.router = Some(router),
            NetworkMapMsg::Open(device, site) => {
                if let (Some(router), Some(site)) = (&self.router, site) {
                    router.push(AppRoute::device(site as u32, device as u32));
                }
                return false;
            }
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
                        {map(state, ctx.link())}
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

/// Columns by hops from the router read (or the first node), the unreached last; the outside
/// node in a column of its own before all.
fn layout(state: &State) -> HashMap<i64, (f64, f64)> {
    let mut neighbors = HashMap::<i64, Vec<i64>>::new();
    for edge in &state.edges {
        let (a, b) = (edge.a.device.id, edge.b.device.id);
        neighbors.entry(a).or_default().push(b);
        neighbors.entry(b).or_default().push(a);
    }
    let root = state
        .read_from
        .as_ref()
        .map(|d| d.id)
        .or_else(|| state.nodes.first().map(|n| n.device.id));
    let mut column = HashMap::new();
    let mut queue = VecDeque::new();
    if let Some(root) = root {
        column.insert(root, 0usize);
        queue.push_back(root);
    }
    while let Some(id) = queue.pop_front() {
        let next = column[&id] + 1;
        for far in neighbors.get(&id).into_iter().flatten() {
            if !column.contains_key(far) {
                column.insert(*far, next);
                queue.push_back(*far);
            }
        }
    }
    let last = column.values().max().map_or(0, |c| c + 1);
    let shift = usize::from(state.outside > 0);
    let mut columns = BTreeMap::<usize, Vec<i64>>::new();
    for node in &state.nodes {
        let c = column.get(&node.device.id).copied().unwrap_or(last);
        columns.entry(c + shift).or_default().push(node.device.id);
    }
    if shift == 1 {
        columns.entry(0).or_default().push(OUTSIDE);
    }
    let mut positions = HashMap::new();
    for (c, ids) in columns {
        for (row, id) in ids.into_iter().enumerate() {
            positions.insert(id, (MARGIN + c as f64 * COLUMN, MARGIN / 2.0 + 20.0 + row as f64 * ROW));
        }
    }
    positions
}

fn map(state: &State, scope: &Scope<NetworkMap>) -> Html {
    let positions = layout(state);
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
        let (class, text) = match node.state {
            NodeState::REACHED => ("reached", t::NODE_REACHED),
            NodeState::ISOLATED => ("isolated", t::NODE_ISOLATED),
            NodeState::NOT_OSPF => ("not-ospf", t::NODE_NOT_OSPF),
            _ => ("missing", t::NODE_MISSING),
        };
        let id = node.device.id;
        let site = node.device.site;
        let onclick = scope.callback(move |_| NetworkMapMsg::Open(id, site));
        let shape = if node.routeros {
            html!(<circle cx={p.0.to_string()} cy={p.1.to_string()} r="14"/>)
        } else {
            html!(<rect x={(p.0 - 10.0).to_string()} y={(p.1 - 10.0).to_string()} width="20" height="20"/>)
        };
        Some(html! {
            <g class={format!("map-node {class}")} {onclick}>
                <title>{format!("{}: {text}", node.device.name)}</title>
                {shape}
                <text x={p.0.to_string()} y={(p.1 + 30.0).to_string()}>{&node.device.name}</text>
            </g>
        })
    });
    html! {
        <div class="network-map">
            <svg viewBox={format!("0 0 {width} {height}")} width={width.to_string()}>
                {for edges}
                {outside}
                {for nodes}
            </svg>
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
            </ul>
        </details>
    }
}
