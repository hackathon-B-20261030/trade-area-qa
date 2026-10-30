//! 상황 구성 (C-204의 런타임 쪽, 02d I-204-r).
//!
//! 맥락은 대안 목록과 노출만 정한다(S-R13). 대안의 요구는 노출을 차원이 소유한 집계 규칙으로 모아
//! 만든다(S-R3). 행선지 대안은 고려 집합(인지 집합) 안에서만 만든다(C-214 ①).

use crate::env::EnvState;
use crate::params::RtParams;
use crate::react::Alt;
use crate::route::{EdgeIx, NodeIx, Path};
use crate::space::{PlaceKind, Space};
use crate::state::Body;
use mdsim_core::bundle::{Mode, Profile};
use mdsim_core::defs::{Aggregate, DimDef};
use mdsim_core::dims::{Dim, DimVec, Interest, ND, NI};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    VisitNow,
    GoElsewhere,
    Skip,
    GoTo,
    Leave,
    RestSeat,
    /// 돈을 내고 앉아 쉬기(REST_PAID를 허용하는 장소, S-E53).
    RestPaid,
    Continue,
    Proceed,
    Pace,
    Abandon,
    Buy,
    NoBuy,
    Path,
    Footing,
    LookAt,
    /// 처음 가는 교차점까지 둘러보며 걷기(S-E25).
    Explore,
    /// 행사 보러 가기(event affords WATCH).
    Watch,
    /// 이동을 잠시 미루고 그 자리에서 기다린다(ROUTE 필수 대안, C-204 ③).
    Defer,
    /// 최단이 아닌 경로 후보로 돌아가기(ROUTE, k ≥ 1).
    Detour,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::VisitNow => "VISIT_NOW",
            Action::GoElsewhere => "GO_ELSEWHERE",
            Action::Skip => "SKIP",
            Action::GoTo => "GO_TO",
            Action::Leave => "LEAVE",
            Action::RestSeat => "REST_SEAT",
            Action::RestPaid => "REST_PAID",
            Action::Continue => "CONTINUE",
            Action::Proceed => "PROCEED",
            Action::Pace => "PACE",
            Action::Abandon => "ABANDON",
            Action::Buy => "BUY",
            Action::NoBuy => "NO_BUY",
            Action::Path => "PATH",
            Action::Footing => "FOOTING",
            Action::LookAt => "LOOK_AT",
            Action::Explore => "EXPLORE",
            Action::Watch => "WATCH",
            Action::Defer => "DEFER",
            Action::Detour => "DETOUR",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Context {
    Visit,
    Next,
    Route,
    Rest,
    Purchase,
    Look,
}

impl Context {
    pub fn name(self) -> &'static str {
        match self {
            Context::Visit => "VISIT",
            Context::Next => "NEXT",
            Context::Route => "ROUTE",
            Context::Rest => "REST",
            Context::Purchase => "PURCHASE",
            Context::Look => "LOOK",
        }
    }

    /// Body.last_decision 인덱스(VISIT 제외).
    pub fn slot(self) -> usize {
        match self {
            Context::Next | Context::Visit => 0,
            Context::Route => 1,
            Context::Rest => 2,
            Context::Purchase => 3,
            Context::Look => 4,
        }
    }

    /// 맥락마다 항상 넣는 대안(01a §3.5, E204-3).
    pub fn required(self) -> &'static [Action] {
        match self {
            Context::Visit => &[Action::Skip],
            Context::Next => &[Action::Leave],
            Context::Route => &[Action::Proceed, Action::Pace, Action::Defer, Action::Abandon],
            Context::Rest => &[Action::Continue, Action::Leave],
            Context::Purchase => &[Action::NoBuy],
            Context::Look => &[Action::Path],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AltTarget {
    None,
    Slot { minute: f64, mode: Mode },
    Competitor(String),
    Place(u32),
    Portal(u32),
    Route(usize),
    Band(u8),
    /// 노드 인덱스(EXPLORE).
    Node(u32),
}

impl AltTarget {
    pub fn label(&self, space: &Space) -> String {
        match self {
            AltTarget::None => String::new(),
            AltTarget::Slot { minute, mode } => format!("{}@{}", mode.as_str(), mdsim_core::scenario::fmt_hhmm(*minute)),
            AltTarget::Competitor(id) => id.clone(),
            AltTarget::Place(p) => space.places.get(*p as usize).map(|pl| format!("place:{}", pl.id)).unwrap_or_default(),
            AltTarget::Portal(p) => space.portals.get(*p as usize).map(|po| format!("portal:{}", po.id)).unwrap_or_default(),
            AltTarget::Route(k) => format!("route:{k}"),
            AltTarget::Band(b) => format!("band:{b}"),
            AltTarget::Node(n) => space.node_id.get(*n as usize).map(|id| format!("node:{id}")).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub action: Action,
    pub target: AltTarget,
    pub alt: Alt,
    /// 고르면 따를 경로(행선지·떠나기·경로 대안).
    pub path: Option<Path>,
    /// 머무는 시간(분, 구매·쉬기).
    pub dwell_min: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exposure {
    pub demand: DimVec,
    /// 합 집계의 배수(간선 하나를 다 지나면 1).
    pub amount: f64,
    /// 시간 가중 평균의 가중(노출 시간).
    pub weight: f64,
}

/// 차원이 소유한 집계 규칙으로 노출을 모은다(S-R3).
pub fn aggregate(ex: &[Exposure], dims: &[DimDef; ND]) -> DimVec {
    let mut out = [0.0; ND];
    for d in Dim::ALL {
        let i = d.ix();
        out[i] = match dims[i].aggregate {
            Aggregate::Sum => ex.iter().map(|e| e.amount * e.demand[i]).sum(),
            Aggregate::Max => ex.iter().filter(|e| e.amount > 0.0).map(|e| e.demand[i]).fold(0.0, f64::max),
            Aggregate::TimeWeightedMean => {
                let w: f64 = ex.iter().map(|e| e.weight).sum();
                if w > 0.0 {
                    ex.iter().map(|e| e.weight * e.demand[i]).sum::<f64>() / w
                } else {
                    0.0
                }
            }
        };
    }
    out
}

/// 상황을 만들 때 읽는 값들.
pub struct Ctx<'a> {
    pub space: &'a Space,
    pub env: &'a EnvState,
    pub p: &'a RtParams,
    pub dims: &'a [DimDef; ND],
    pub tick: u64,
    pub dt: f64,
    pub prior_density: f64,
}

impl Ctx<'_> {
    pub fn fresh_ticks(&self) -> u64 {
        (self.p.perception.fresh_s / self.dt).round() as u64
    }

    /// 본 밀도(신선) 또는 사전 기대(DR-221 ②).
    pub fn believed_density(&self, b: &Body, e: EdgeIx) -> f64 {
        b.seen_density(e, self.tick, self.fresh_ticks()).unwrap_or(self.prior_density)
    }

    pub fn walk_speed(&self, b: &Body) -> f64 {
        b.base_speed.max(0.3)
    }
}

/// 경로의 노출 목록, 걷는 시간(분), 길이.
pub fn path_exposures(c: &Ctx, b: &Body, path: &Path, pace: f64) -> (Vec<Exposure>, f64, f64) {
    let mut ex = Vec::with_capacity(path.edges.len());
    let mut secs = 0.0;
    let speed = c.walk_speed(b) / pace.max(0.5);
    for &(e, fwd) in &path.edges {
        let es = &c.space.edges[e as usize];
        let mut d = *es.demand_of(fwd, c.env);
        d[Dim::Crowd.ix()] = c.believed_density(b, e);
        let t = es.len / speed;
        let wait = if c.space.node_signal[es.a as usize] || c.space.node_signal[es.b as usize] { c.p.route.crossing_wait_s * 0.5 } else { 0.0 };
        secs += t + wait;
        ex.push(Exposure { demand: d, amount: 1.0, weight: t });
    }
    (ex, secs / 60.0, path.length)
}

/// 몸이 길을 찾을 출발 노드(지금 간선의 가까운 끝, 없으면 가장 가까운 노드).
pub fn start_node(space: &Space, b: &Body) -> NodeIx {
    if let Some(e) = b.cur_edge {
        let es = &space.edges[e as usize];
        let (pa, pb) = (space.graph.node_pos[es.a as usize], space.graph.node_pos[es.b as usize]);
        return if b.pos.dist(pa) <= b.pos.dist(pb) { es.a } else { es.b };
    }
    space.graph.nearest_node(b.pos)
}

/// 한 출발점에서 모든 노드까지의 최단 경로 나무(걷는 시간 기준).
pub struct Tree {
    pub from: NodeIx,
    pub dist: Vec<f64>,
    pub prev: Vec<Option<(EdgeIx, NodeIx)>>,
}

pub fn shortest_tree(space: &Space, from: NodeIx, speed: f64, crossing_wait_s: f64) -> Tree {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;
    #[derive(PartialEq)]
    struct It(f64, NodeIx);
    impl Eq for It {}
    impl Ord for It {
        fn cmp(&self, o: &Self) -> Ordering {
            o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
        }
    }
    impl PartialOrd for It {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }
    let n = space.graph.n_nodes();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev = vec![None; n];
    let mut h = BinaryHeap::new();
    dist[from as usize] = 0.0;
    h.push(It(0.0, from));
    while let Some(It(c, u)) = h.pop() {
        if c > dist[u as usize] {
            continue;
        }
        for &(e, v) in space.graph.neighbors(u) {
            let w = space.walk_cost(e, speed, crossing_wait_s);
            let nc = c + w;
            if nc < dist[v as usize] {
                dist[v as usize] = nc;
                prev[v as usize] = Some((e, u));
                h.push(It(nc, v));
            }
        }
    }
    Tree { from, dist, prev }
}

pub fn tree_path(space: &Space, t: &Tree, to: NodeIx) -> Option<Path> {
    if !t.dist[to as usize].is_finite() {
        return None;
    }
    let mut nodes = vec![to];
    let mut edges = Vec::new();
    let mut cur = to;
    while cur != t.from {
        let (e, p) = t.prev[cur as usize]?;
        edges.push((e, space.graph.edge_a[e as usize] == p));
        nodes.push(p);
        cur = p;
    }
    nodes.reverse();
    edges.reverse();
    let length = edges.iter().map(|(e, _)| space.edges[*e as usize].len).sum();
    Some(Path { nodes, edges, length })
}

/// 장소의 기대 대기(분): 본 줄 길이 ÷ 처리율.
pub fn believed_wait_min(b: &Body, c: &Ctx, place: u32) -> f64 {
    let pl = &c.space.places[place as usize];
    let q = b.aware_of(place).and_then(|a| if a.last_seen_tick.map(|t| c.tick.saturating_sub(t) <= c.fresh_ticks()).unwrap_or(false) { a.seen_queue } else { None }).unwrap_or(0);
    if pl.service_rate > 0.0 {
        q as f64 / pl.service_rate
    } else {
        0.0
    }
}

/// 몸이 그 장소를 영업 중이라고 믿는가(본 적 있으면 본 값, 없으면 열려 있다고 본다).
pub fn believed_open(b: &Body, place: u32) -> bool {
    b.aware_of(place).and_then(|a| a.seen_open).unwrap_or(true)
}

/// 장소의 목적 가치: 그 장소를 목표로 한 목적, 또는 그 업종의 목적(아직 이루지 않은 것).
pub fn purpose_value(prof: &Profile, b: &Body, space: &Space, place: u32) -> f64 {
    let pl = &space.places[place as usize];
    let mut v = 0.0;
    for pu in &prof.purposes {
        if purpose_done(pu, b, space) {
            continue;
        }
        if pu.target_place_id.as_deref() == Some(pl.id.as_str()) {
            v += pu.value;
        } else if let (None, Some(k)) = (&pu.target_place_id, pu.interest) {
            if pl.offer[k.ix()] >= 0.5 {
                v += 0.6 * pu.value;
            }
        }
    }
    v
}

/// 목적을 이뤘는가: 업종 목적은 충족도 0.8 이상, 업종과 무관한 목적(만남·일)은 목적 장소에 들어간 적이 있을 때.
pub fn purpose_done(pu: &mdsim_core::bundle::Purpose, b: &Body, space: &Space) -> bool {
    match pu.interest {
        Some(k) => b.sat[k.ix()] >= 0.8,
        None => match pu.target_place_id.as_deref().and_then(|id| space.place_ix(id)) {
            Some(i) => b.entered.contains(&(i as u32)),
            None => true,
        },
    }
}

pub fn expected_dwell_min(p: &RtParams, kind: PlaceKind) -> f64 {
    match kind {
        PlaceKind::Shop | PlaceKind::Stall => p.purchase.dwell_buy_min.get(1).copied().unwrap_or(15.0),
        PlaceKind::Visit => p.purchase.dwell_buy_min.first().copied().unwrap_or(10.0),
        PlaceKind::Event => 15.0,
        PlaceKind::Bench => p.rest.seat_min,
        PlaceKind::Lodging => 0.0,
    }
}

fn exit_choice(c: &Ctx, b: &Body, tree: &Tree) -> Option<Choice> {
    // 들어온 출입구와 같은 종류 중 가장 가까운 곳
    let kind = c.space.portals[b.portal_in as usize].kind;
    let mut best: Option<(f64, usize)> = None;
    for (i, po) in c.space.portals.iter().enumerate() {
        if po.kind != kind {
            continue;
        }
        let d = tree.dist[po.node as usize];
        if d.is_finite() && best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, i));
        }
    }
    let (_, pi) = best.or_else(|| c.space.portals.iter().enumerate().filter_map(|(i, po)| tree.dist[po.node as usize].is_finite().then_some((tree.dist[po.node as usize], i))).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))))?;
    let path = tree_path(c.space, tree, c.space.portals[pi].node)?;
    let (ex, walk_min, _) = path_exposures(c, b, &path, 1.0);
    let demand = aggregate(&ex, c.dims);
    Some(Choice { action: Action::Leave, target: AltTarget::Portal(pi as u32), alt: Alt { demand, offer: [0.0; NI], purpose: 0.0, achieve: 1.0, dt_min: walk_min, cost_krw: 0.0, blocked: false }, path: Some(path), dwell_min: 0.0 })
}

/// 행선지 NEXT. 고려 집합 안의 장소(목적 장소 + 가까운 순 상한, S-E9), 둘러보기(EXPLORE), 떠나기, 알고 있는 벤치.
/// `u_explore`는 둘러볼 교차점을 고르는 키드 난수(EXPLORE, 몸, 틱).
pub fn next_situation(c: &Ctx, prof: &Profile, b: &Body, current: Option<u32>, avoid: Option<u32>, u_explore: f64) -> Vec<Choice> {
    let speed = c.walk_speed(b);
    let from = start_node(c.space, b);
    let tree = shortest_tree(c.space, from, speed, c.p.route.crossing_wait_s);
    let mut out = Vec::new();
    // 후보: 인지 집합 중 영업 중이라고 믿는 방문 장소
    let mut cands: Vec<(f64, u32, bool)> = b
        .aware
        .iter()
        .filter(|a| {
            let pl = &c.space.places[a.place as usize];
            !matches!(pl.kind, PlaceKind::Bench) && believed_open(b, a.place) && Some(a.place) != avoid
        })
        .map(|a| {
            let pl = &c.space.places[a.place as usize];
            let is_purpose = prof.purposes.iter().any(|pu| pu.target_place_id.as_deref() == Some(pl.id.as_str()));
            (pl.anchor.dist(b.pos), a.place, is_purpose)
        })
        .collect();
    cands.sort_by(|x, y| y.2.cmp(&x.2).then(x.0.total_cmp(&y.0)).then(x.1.cmp(&y.1)));
    let n_purpose = cands.iter().filter(|x| x.2).count();
    cands.truncate(n_purpose + c.p.next_max_candidates);
    // 벤치로 가던 몸의 '현재 계획 유지'는 그 벤치의 쉬기 대안과 같은 값이다(S-E54). 벤치는 방문 후보로 만들지 않는다.
    let cur_bench = current.filter(|&p| c.space.places[p as usize].kind == PlaceKind::Bench);
    if let Some(cur) = current.filter(|_| cur_bench.is_none()) {
        if !cands.iter().any(|x| x.1 == cur) && believed_open(b, cur) {
            cands.push((0.0, cur, false));
        }
    }
    for &(_, pi, _) in &cands {
        let pl = &c.space.places[pi as usize];
        let Some(path) = tree_path(c.space, &tree, pl.node_near) else { continue };
        let (mut ex, walk_min, _) = path_exposures(c, b, &path, 1.0);
        // 장소 자체의 노출(행사 음향의 D-SENSE, 언어 부담 D-WAY)
        let mut d = pl.demand;
        d[Dim::Money.ix()] = 0.0;
        d[Dim::Time.ix()] = 0.0;
        let dwell = expected_dwell_min(c.p, pl.kind);
        ex.push(Exposure { demand: d, amount: 1.0, weight: dwell * 60.0 });
        let demand = aggregate(&ex, c.dims);
        let wait = believed_wait_min(b, c, pi);
        let mut purpose = purpose_value(prof, b, c.space, pi);
        if current.is_some() && current != Some(pi) {
            purpose -= c.p.react.switch_cost;
        }
        let expected_spend = if pl.can_buy { pl.price * c.p.visit.expected_spend_share } else { 0.0 };
        out.push(Choice {
            action: if pl.kind == PlaceKind::Event && c.p.next_watch { Action::Watch } else { Action::GoTo },
            target: AltTarget::Place(pi),
            alt: Alt { demand, offer: pl.offer, purpose, achieve: 1.0, dt_min: walk_min + wait + dwell, cost_krw: expected_spend, blocked: false },
            path: Some(path),
            dwell_min: dwell,
        });
    }
    // 알고 있는 벤치(rest.max_bench_m 안): 정의의 NEXT 맥락이 REST_SEAT를 허용할 때만(v1은 REST 맥락이 맡는다)
    if let Some(cur) = cur_bench {
        let action = if c.p.next_rest_seat { Action::RestSeat } else { Action::GoTo };
        if let Some(ch) = rest_choice(c, prof, b, &tree, cur, action) {
            out.push(ch);
        }
    }
    if c.p.next_rest_seat {
        if let Some(ch) = bench_choice(c, prof, b, &tree).filter(|ch| cur_bench.is_none_or(|p| ch.target != AltTarget::Place(p))) {
            out.push(ch);
        }
    }
    if let Some(ch) = explore_choice(c, prof, b, &tree, u_explore) {
        out.push(ch);
    }
    if let Some(ch) = exit_choice(c, b, &tree) {
        out.push(ch);
    }
    out
}

/// EXPLORE(S-E25): 걸어서 반경 [min, max] m 안의 처음 가는 교차점 하나(키드 난수로 고른다)까지 둘러보며 걷는다.
/// 제공 = 탐색 성향 × 발견 몫 × 갈 수 있는 장소의 평균 제공, 목적 가치 = 같은 비율 × 아직 이루지 못한 업종 목적의 0.6배.
fn explore_choice(c: &Ctx, prof: &Profile, b: &Body, tree: &Tree, u: f64) -> Option<Choice> {
    if !c.p.next_explore {
        return None;
    }
    let speed = c.walk_speed(b);
    let (rmin, rmax) = (c.p.explore.radius_min_m, c.p.explore.radius_max_m);
    let cands: Vec<u32> = (0..c.space.graph.node_pos.len() as u32)
        .filter(|&n| {
            let d = tree.dist[n as usize] * speed;
            d.is_finite() && d >= rmin && d <= rmax && c.space.graph.neighbors(n).len() != 2 && !b.explored.contains(&n)
        })
        .collect();
    if cands.is_empty() {
        return None;
    }
    let node = cands[((u * cands.len() as f64).floor() as usize).min(cands.len() - 1)];
    let path = tree_path(c.space, tree, node)?;
    let (ex, walk_min, _) = path_exposures(c, b, &path, 1.0);
    let demand = aggregate(&ex, c.dims);
    let k = prof.exploration.clamp(0.0, 1.0) * c.p.explore.discovery_share;
    let offer: [f64; NI] = std::array::from_fn(|i| k * c.space.mean_offer[i]);
    let unmet: f64 = prof.purposes.iter().filter(|pu| pu.interest.is_some() && pu.target_place_id.is_none() && !purpose_done(pu, b, c.space)).map(|pu| pu.value).sum();
    Some(Choice { action: Action::Explore, target: AltTarget::Node(node), alt: Alt { demand, offer, purpose: k * 0.6 * unmet, achieve: 1.0, dt_min: walk_min, cost_krw: 0.0, blocked: false }, path: Some(path), dwell_min: 0.0 })
}

/// 남은 걷기 기대(m): 남은 시간의 절반을 걷는다고 보고 1km로 자른다.
fn remaining_walk_m(c: &Ctx, b: &Body) -> f64 {
    (b.minutes_left(c.tick, c.dt) * 60.0 * c.walk_speed(b) * 0.5).min(1000.0)
}

/// `rest.max_bench_m` 안의 가장 가까운 아는 쉼터. 무료(REST_SEAT 허용) 또는 열려 있다고 믿는 유료(REST_PAID만 허용, S-E53).
fn nearest_rest(c: &Ctx, b: &Body, paid: bool) -> Option<u32> {
    let mut best: Option<(f64, u32)> = None;
    for a in &b.aware {
        let pl = &c.space.places[a.place as usize];
        let ok = if paid { pl.rest_paid && !pl.rest_seat && believed_open(b, a.place) } else { pl.rest_seat };
        if !ok {
            continue;
        }
        let d = pl.anchor.dist(b.pos);
        if d <= c.p.rest.max_bench_m && best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, a.place));
        }
    }
    best.map(|(_, p)| p)
}

/// 쉼터 pi로 가서 `rest.seat_min` 쉬는 대안. 쉰 뒤의 남은 걷기 부담은 피로가 앉아 쉰 만큼 준다(S-E24).
/// 유료 쉼터는 장소 가격 × 기대 지출 몫을 내고 믿는 줄 대기를 더한다(S-E53).
fn rest_choice(c: &Ctx, prof: &Profile, b: &Body, tree: &Tree, pi: u32, action: Action) -> Option<Choice> {
    let pl = &c.space.places[pi as usize];
    let path = tree_path(c.space, tree, pl.node_near)?;
    let (mut ex, walk_min, _) = path_exposures(c, b, &path, 1.0);
    let rec = c.p.state.recover_seated_per_min * c.p.rest.seat_min;
    let mut d = [0.0; ND];
    d[Dim::Walk.ix()] = remaining_walk_m(c, b) * (1.0 + (b.fatigue - rec).max(0.0));
    ex.push(Exposure { demand: d, amount: 1.0, weight: 0.0 });
    let demand = aggregate(&ex, c.dims);
    let unmet: f64 = prof.purposes.iter().filter(|pu| !purpose_done(pu, b, c.space)).map(|pu| pu.value).sum();
    let paid = pl.rest_paid && !pl.rest_seat;
    let (cost_krw, wait) = if paid { (pl.price * c.p.visit.expected_spend_share, believed_wait_min(b, c, pi)) } else { (0.0, 0.0) };
    Some(Choice { action, target: AltTarget::Place(pi), alt: Alt { demand, offer: [0.0; NI], purpose: 0.5 * unmet, achieve: 1.0, dt_min: walk_min + wait + c.p.rest.seat_min, cost_krw, blocked: false }, path: Some(path), dwell_min: c.p.rest.seat_min })
}

fn bench_choice(c: &Ctx, prof: &Profile, b: &Body, tree: &Tree) -> Option<Choice> {
    rest_choice(c, prof, b, tree, nearest_rest(c, b, false)?, Action::RestSeat)
}

fn paid_rest_choice(c: &Ctx, prof: &Profile, b: &Body, tree: &Tree) -> Option<Choice> {
    rest_choice(c, prof, b, tree, nearest_rest(c, b, true)?, Action::RestPaid)
}

/// 휴식 REST: 앉아 쉬기(알고 있는 벤치), 계속, 떠나기.
pub fn rest_situation(c: &Ctx, prof: &Profile, b: &Body) -> Vec<Choice> {
    let from = start_node(c.space, b);
    let tree = shortest_tree(c.space, from, c.walk_speed(b), c.p.route.crossing_wait_s);
    let unmet: f64 = prof.purposes.iter().filter(|pu| !purpose_done(pu, b, c.space)).map(|pu| pu.value).sum();
    let mut out = Vec::new();
    let mut d = [0.0; ND];
    d[Dim::Walk.ix()] = remaining_walk_m(c, b) * (1.0 + b.fatigue);
    out.push(Choice { action: Action::Continue, target: AltTarget::None, alt: Alt { demand: d, offer: [0.0; NI], purpose: 0.5 * unmet, achieve: 1.0, dt_min: 0.0, cost_krw: 0.0, blocked: false }, path: None, dwell_min: 0.0 });
    if let Some(ch) = bench_choice(c, prof, b, &tree) {
        out.push(ch);
    }
    if c.p.rest_paid {
        if let Some(ch) = paid_rest_choice(c, prof, b, &tree) {
            out.push(ch);
        }
    }
    if let Some(ch) = exit_choice(c, b, &tree) {
        out.push(ch);
    }
    out
}

/// 경로 ROUTE: 경로 후보 k개(PROCEED), 1번 경로를 쉬어 가며(PACE), 포기(ABANDON).
/// `fallback_u`는 행선지를 고를 때의 차선 효용이다. 포기는 그 계획으로 돌아가며 전환 비용을 치른다(S-E23).
pub fn route_situation(c: &Ctx, b: &Body, target_node: NodeIx, final_pos: mdsim_core::geom::V2, purpose: f64, offer: [f64; NI], fallback_u: f64) -> Vec<Choice> {
    let from = start_node(c.space, b);
    let speed = c.walk_speed(b);
    let paths = c.space.graph.k_paths(from, target_node, c.p.route.k, c.p.route.penalty, &|e, _| c.space.walk_cost(e, speed, c.p.route.crossing_wait_s));
    let mut out = Vec::new();
    for (k, path) in paths.iter().enumerate() {
        let (ex, walk_min, _) = path_exposures(c, b, path, 1.0);
        let mut demand = aggregate(&ex, c.dims);
        demand[Dim::Walk.ix()] += final_pos.dist(c.space.graph.node_pos[target_node as usize]);
        // 최단 경로는 PROCEED, 그 밖 후보는 우회 DETOUR(02c GS-1의 '평지 우회'와 같은 뜻)
        let action = if k == 0 { Action::Proceed } else { Action::Detour };
        out.push(Choice { action, target: AltTarget::Route(k), alt: Alt { demand, offer, purpose, achieve: 1.0, dt_min: walk_min, cost_krw: 0.0, blocked: false }, path: Some(path.clone()), dwell_min: 0.0 });
    }
    if let Some(first) = paths.first() {
        let (ex, walk_min, _) = path_exposures(c, b, first, c.p.route.pace_time);
        let mut demand = aggregate(&ex, c.dims);
        demand[Dim::Climb.ix()] *= c.p.route.pace_exertion;
        demand[Dim::Fall.ix()] *= c.p.route.pace_exertion;
        out.push(Choice { action: Action::Pace, target: AltTarget::Route(0), alt: Alt { demand, offer, purpose, achieve: 1.0, dt_min: walk_min, cost_krw: 0.0, blocked: false }, path: Some(first.clone()), dwell_min: 0.0 });
    }
    // 연기(S-E36): route.defer_min 동안 그 자리에서 기다린 뒤 1번 경로로 간다. 지금 본 혼잡 대신
    // 사전 기대 혼잡을 쓰므로, 앞길이 기대보다 붐빌 때 이긴다. 비용은 기다리는 시간(D-TIME)과
    // 계획을 바꾸는 현상 유지 상수(react.status_quo, 포기와 같음)다.
    if let Some(first) = paths.first() {
        let (ex, walk_min, _) = path_exposures(c, b, first, 1.0);
        let mut demand = aggregate(&ex, c.dims);
        demand[Dim::Walk.ix()] += final_pos.dist(c.space.graph.node_pos[target_node as usize]);
        demand[Dim::Crowd.ix()] = demand[Dim::Crowd.ix()].min(c.prior_density);
        out.push(Choice { action: Action::Defer, target: AltTarget::Route(0), alt: Alt { demand, offer, purpose: purpose - c.p.react.status_quo, achieve: 1.0, dt_min: walk_min + c.p.route.defer_min, cost_krw: 0.0, blocked: false }, path: Some(first.clone()), dwell_min: c.p.route.defer_min });
    }
    // 포기: 목적지를 버리고 차선 계획(NEXT의 차선 효용)으로 돌아가며 전환 비용을 치른다(S-E23).
    out.push(Choice { action: Action::Abandon, target: AltTarget::None, alt: Alt { demand: [0.0; ND], offer: [0.0; NI], purpose: fallback_u - c.p.react.switch_cost - c.p.react.status_quo, achieve: 1.0, dt_min: 0.0, cost_krw: 0.0, blocked: false }, path: None, dwell_min: 0.0 });
    out
}

/// 구매 PURCHASE: 구매 안 함과 금액대별 구매(음식 위주 장소는 한 가지).
pub fn purchase_situation(c: &Ctx, b: &Body, place: u32) -> Vec<Choice> {
    let pl = &c.space.places[place as usize];
    let p = &c.p.purchase;
    let mut out = Vec::new();
    let browse_offer: [f64; NI] = std::array::from_fn(|k| pl.offer[k] * 0.3);
    let _ = b;
    out.push(Choice { action: Action::NoBuy, target: AltTarget::Band(0), alt: Alt { demand: pl.demand_zero_money(), offer: browse_offer, purpose: 0.0, achieve: 1.0, dt_min: p.dwell_no_buy_min, cost_krw: 0.0, blocked: false }, path: None, dwell_min: p.dwell_no_buy_min });
    if !pl.can_buy || pl.price <= 0.0 {
        return out;
    }
    let main = (0..NI).max_by(|&i, &j| pl.offer[i].total_cmp(&pl.offer[j]).then(j.cmp(&i))).unwrap_or(0);
    let food = main == Interest::Food.ix();
    let bands: Vec<usize> = if food { vec![1] } else { (0..p.band_mult.len()).collect() };
    for k in bands {
        let mult = p.band_mult[k];
        let gain = p.band_gain.get(k).copied().unwrap_or(1.0);
        let dwell = p.dwell_buy_min.get(k).copied().unwrap_or(15.0);
        let offer: [f64; NI] = std::array::from_fn(|i| pl.offer[i] * gain);
        out.push(Choice { action: Action::Buy, target: AltTarget::Band(k as u8 + 1), alt: Alt { demand: pl.demand_zero_money(), offer, purpose: 0.0, achieve: 1.0, dt_min: dwell, cost_krw: pl.price * mult, blocked: false }, path: None, dwell_min: dwell });
    }
    out
}

impl crate::space::PlaceInst {
    /// 장소의 요구에서 돈·시간을 뺀 것(돈·시간은 대안의 cost·dt로 따로 들어간다).
    pub fn demand_zero_money(&self) -> DimVec {
        let mut d = self.demand;
        d[Dim::Money.ix()] = 0.0;
        d[Dim::Time.ix()] = 0.0;
        d
    }
}
