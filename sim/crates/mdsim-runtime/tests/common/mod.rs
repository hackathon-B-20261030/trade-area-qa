//! 통합 테스트 공용: 합성 고정 자료를 한 번 만들어 같이 쓴다.
//!
//! 실행 준비물(World·도착 계획)은 정적 수명으로 새어 나가게 해(`leak`) 테스트 사이에 그대로 나눠 쓴다.
//! 각 테스트 파일은 따로 컴파일되므로 `OnceLock` 값도 파일(테스트 바이너리)마다 한 번씩 만들어진다.
#![allow(dead_code)]

use mdsim_core::bundle::{Bundle, Profile};
use mdsim_core::defs::Defs;
use mdsim_core::fixture::{write_fixture, FixturePaths};
use mdsim_core::geom::V2;
use mdsim_core::scenario::Scenario;
use mdsim_runtime::behavior::DCtx;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::env::EnvState;
use mdsim_runtime::record::{chunk_header, encode_frame, AltRow, DecisionRow, EventRow, FrameRec, MemSink, Sink};
use mdsim_runtime::route::Path;
use mdsim_runtime::situation::{Choice, Ctx};
use mdsim_runtime::space::Space;
use mdsim_runtime::state::{Activity, Body, GazeTarget, RoutePlan, Target};
use mdsim_runtime::step::{Sim, SimState, World};
use mdsim_runtime::visit::{plan_arrivals, plan_background, prior_density, representation_ratio, sort_spawns, visit_decisions, Spawn, VisitDecision};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub struct Fx {
    pub paths: FixturePaths,
    pub defs: Defs,
    pub bundle: Bundle,
    pub space: Space,
}

pub fn fx() -> &'static Fx {
    static FX: OnceLock<Fx> = OnceLock::new();
    FX.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("mdsim-rt-fx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = write_fixture(&root).expect("fixture");
        let defs = Defs::load(&paths.defs).expect("defs");
        let bundle = Bundle::load(&paths.bundle, &paths.root, &defs).expect("bundle");
        let space = Space::build(&defs, &bundle).expect("space");
        Fx { paths, defs, bundle, space }
    })
}

pub fn scenario(name: &str) -> Scenario {
    let f = fx();
    let p = match name {
        "base" => &f.paths.scenario,
        "quick" => &f.paths.scenario_quick,
        _ => &f.paths.scenario_variant,
    };
    Scenario::load(p).expect("scenario")
}

// ---------- 실행 준비 ----------

/// 테스트 동안 살아 있으면 되는 값을 정적 수명으로 만든다.
pub fn leak<T>(v: T) -> &'static T {
    Box::leak(Box::new(v))
}

pub fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// 시나리오 해시(테스트용): 직렬화한 JSON의 SHA-256.
pub fn scenario_sha(s: &Scenario) -> String {
    sha_hex(&serde_json::to_vec(s).expect("scenario json"))
}

/// 시나리오의 실행 창을 바꾼다("HH:MM").
pub fn with_window(mut s: Scenario, start: &str, end: &str) -> Scenario {
    s.window.start = start.into();
    s.window.end = end.into();
    s
}

/// 한 실행의 준비물: World, 방문지 선택, 대표 비율, 정렬한 도착 계획(내국인 + 배경 보행자).
pub struct Prep {
    pub w: &'static World<'static>,
    pub dec: Vec<VisitDecision>,
    pub ratio: f64,
    pub spawns: &'static [Spawn],
}

impl Prep {
    pub fn sim(&self) -> Sim<'static> {
        Sim::new(self.w, self.spawns)
    }
}

pub fn world_on(bundle: &'static Bundle, space: &'static Space, scn: Scenario) -> &'static World<'static> {
    let f = fx();
    let scn = leak(scn);
    leak(World::new(&f.defs, bundle, space, scn, scenario_sha(scn)).expect("world"))
}

/// smoke 예제와 같은 순서: 방문지 선택 → 대표 비율 → 도착 계획 + 배경 보행자 → 몸 ID 순 정렬.
pub fn plan(w: &World) -> (Vec<VisitDecision>, f64, Vec<Spawn>) {
    let dec = visit_decisions(w.defs, w.space, w.bundle, w.scn, &w.p, &w.rng);
    let ratio = representation_ratio(&dec, w.scn, &w.p);
    let mut spawns = plan_arrivals(w.space, w.bundle, w.scn, &w.p, &w.rng, &dec, ratio);
    spawns.extend(plan_background(w.space, w.bundle, w.scn, &w.p, &w.rng));
    (dec, ratio, sort_spawns(spawns))
}

pub fn prep_on(bundle: &'static Bundle, space: &'static Space, scn: Scenario) -> Prep {
    let w = world_on(bundle, space, scn);
    let (dec, ratio, spawns) = plan(w);
    Prep { w, dec, ratio, spawns: Box::leak(spawns.into_boxed_slice()) }
}

pub fn prep(scn: Scenario) -> Prep {
    let f = fx();
    prep_on(&f.bundle, &f.space, scn)
}

/// 도착 계획 없이(몸을 테스트가 직접 넣는) 실행 준비물.
pub fn prep_empty(scn: Scenario) -> Prep {
    let f = fx();
    let w = world_on(&f.bundle, &f.space, scn);
    Prep { w, dec: Vec::new(), ratio: 0.0, spawns: &[] }
}

pub fn with_threads<R: Send>(n: usize, f: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new().num_threads(n).build().expect("rayon pool").install(f)
}

/// 스냅샷 전체의 해시(직렬화 JSON의 SHA-256).
pub fn state_hash(s: &SimState) -> String {
    sha_hex(&serde_json::to_vec(s).expect("state json"))
}

// ---------- 관찰 실행 ----------

/// 틱 t를 시작할 때(step 전) 몸 하나의 요약.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub id: u32,
    pub alive: bool,
    pub domestic: bool,
    pub pos: V2,
    pub heading: f64,
    pub gaze: f64,
    pub activity: Activity,
    pub gaze_target: GazeTarget,
    pub radius: f64,
    pub base_speed: f64,
}

impl Snap {
    pub fn of(b: &Body) -> Snap {
        Snap { id: b.id, alive: b.alive, domestic: !b.is_background(), pos: b.pos, heading: b.heading, gaze: b.gaze, activity: b.activity, gaze_target: b.gaze_target, radius: b.radius, base_speed: b.base_speed }
    }
}

/// 틱마다 step 전 상태를 남기며 돈 실행.
pub struct Observed {
    pub prep: Prep,
    /// snaps[t] = 틱 t를 시작할 때의 몸들(그 틱에 도착할 몸은 아직 없다). 길이는 틱 수 + 1.
    pub snaps: Vec<Vec<Snap>>,
    pub sink: MemSink,
    pub state: SimState,
}

pub fn observe(prep: Prep, ticks: u64) -> Observed {
    let end = ticks.min(prep.w.scn.n_ticks());
    let mut sim = prep.sim();
    let mut sink = MemSink::default();
    let mut snaps = Vec::with_capacity(end as usize + 1);
    while sim.state.tick < end {
        snaps.push(sim.state.bodies.iter().map(Snap::of).collect());
        let t = sim.state.tick;
        sim.run_until(t + 1, &mut sink).expect("step");
    }
    snaps.push(sim.state.bodies.iter().map(Snap::of).collect());
    let state = sim.state;
    Observed { prep, snaps, sink, state }
}

pub const QUICK_TICKS: u64 = 3600;

/// 빠른 시나리오(14:00~15:00)의 앞 30분을 틱마다 관찰한 실행(바이너리마다 한 번).
pub fn observed_quick() -> &'static Observed {
    static O: OnceLock<Observed> = OnceLock::new();
    O.get_or_init(|| observe(prep(scenario("quick")), QUICK_TICKS))
}

pub const MID_TICK: u64 = 1200;

/// 빠른 시나리오를 10분(1200틱) 돌린 상태. 단위 테스트가 실제 몸·인지 집합을 꺼내 쓴다.
pub struct Mid {
    pub prep: Prep,
    pub state: SimState,
}

pub fn mid_quick() -> &'static Mid {
    static M: OnceLock<Mid> = OnceLock::new();
    M.get_or_init(|| {
        let prep = prep(scenario("quick"));
        let mut sim = prep.sim();
        let mut sink = MemSink::default();
        sim.run_until(MID_TICK, &mut sink).expect("run");
        let state = sim.state;
        Mid { prep, state }
    })
}

impl Mid {
    pub fn w(&self) -> &'static World<'static> {
        self.prep.w
    }

    /// 거리 위의 살아 있는 내국인 몸들.
    pub fn street_domestic(&self) -> Vec<&Body> {
        self.state.bodies.iter().filter(|b| b.alive && !b.is_background() && b.activity.on_street()).collect()
    }
}

/// 계기가 없는 걷는 몸 하나(목표 장소와 경로가 있고, 처리할 계기·새 알아차림이 없고, 마감이 멀다).
pub fn quiet_walker(m: &Mid) -> Body {
    let w = m.w();
    let tick = m.state.tick;
    m.state
        .bodies
        .iter()
        .find(|b| {
            b.alive
                && !b.is_background()
                && b.activity == Activity::Walk
                && matches!(b.target, Target::Place(_))
                && b.route.is_some()
                && b.pending.is_none()
                && !b.arrived
                && b.new_notices.is_empty()
                && b.fatigue < 0.3
                && b.minutes_left(tick, w.dt()) > 30.0
        })
        .expect("계기 없는 걷는 몸")
        .clone()
}

// ---------- 지각 시험 도우미 ----------

/// 보행 공간 위의 표본 위치(간선마다 네 곳 × 옆 세 줄, 걷기 가능한 칸만).
pub fn sample_points(space: &Space) -> Vec<V2> {
    let mut v = Vec::new();
    for e in &space.edges {
        for t in [0.15, 0.4, 0.65, 0.9] {
            let (c, dir) = mdsim_core::geom::point_at(&e.poly, &e.cum, t * e.len);
            let half = if e.sidewalks { e.width / 2.0 - e.sw / 2.0 } else { e.width / 2.0 - 1.0 };
            for l in [-half, 0.0, half] {
                let q = c + dir.perp() * l;
                if space.walkable_static(q) {
                    v.push(q);
                }
            }
        }
    }
    v
}

/// 혼자 선 몸(id 0): 주어진 위치·진행 방향·시선, 빈 인지 집합, 피로 0, 서두르지 않음(마감이 멂), 경로 없음.
pub fn lone(m: &Mid, pos: V2, heading: f64, gaze: f64) -> Body {
    let mut b = quiet_walker(m);
    b.id = 0;
    b.pos = pos;
    b.heading = heading;
    b.gaze = gaze;
    b.gaze_target = GazeTarget::Path;
    b.aware.clear();
    b.obs_edges.clear();
    b.new_notices.clear();
    b.fatigue = 0.0;
    b.deadline_tick = m.state.tick + 1_000_000;
    b.route = None;
    b.cur_edge = None;
    b
}

/// 혼자 선 몸 하나의 집단 상태.
pub fn lone_coll(m: &Mid, b: &Body) -> Collective {
    Collective::build(std::slice::from_ref(b), m.w().space, &m.state.env, m.w().p.crowd.local_radius_m)
}

// ---------- 일자 통로 ----------

/// 건물·장소·출입구 없이 간선 하나뿐인 일자 통로(x축, 중심선 z = 0). 걷는 길이는 [−20, len + 20].
pub fn corridor_space(len: f64, width: f64) -> Space {
    let f = fx();
    let mut b = f.bundle.clone();
    let mut n0 = b.nodes[0].clone();
    let mut n1 = b.nodes[0].clone();
    (n0.id, n0.pos) = ("N:c0".into(), V2::new(-20.0, 0.0));
    (n1.id, n1.pos) = ("N:c1".into(), V2::new(len + 20.0, 0.0));
    let template = b.edges.iter().find(|e| e.road_type == "pedestrian").expect("보행자 도로 간선").clone();
    let mut e = template;
    e.id = "E:corridor".into();
    e.a = n0.id.clone();
    e.b = n1.id.clone();
    e.polyline = vec![n0.pos, n1.pos];
    e.length_m = len + 40.0;
    e.width_m = width;
    e.grade_pct = 0.0;
    e.grade_pct_smooth = 0.0;
    for (k, v) in [("length_m", len + 40.0), ("width_m", width), ("grade_pct", 0.0)] {
        e.attrs.insert(k.into(), v);
    }
    b.nodes = vec![n0, n1];
    b.edges = vec![e];
    b.buildings.clear();
    b.places.clear();
    b.portals.clear();
    Space::build(&f.defs, &b).expect("통로 공간")
}

/// 주기 경계 일자 통로 실험: 몸들을 [0, len)에 놓고 모두 +x로 걷게 한다. 경계 너머 이웃은 ±len 옮긴 그림자로 본다.
/// 돌려주는 값은 측정 구간(warm 뒤 measure 틱)의 평균 x 방향 속도(m/s)와 겹친 쌍의 최솟값(간격 − 반지름 합).
pub struct Corridor {
    pub space: Space,
    pub len: f64,
    pub width: f64,
}

impl Corridor {
    pub fn new(len: f64, width: f64) -> Corridor {
        Corridor { space: corridor_space(len, width), len, width }
    }

    /// 밀도 rho(인/㎡)로 몸을 육각 격자(줄 간격 a·√3/2, 줄 안 간격은 길이를 채우는 값)에 놓는다. 처음에 겹친 쌍이 없게
    /// 둔다(엔진은 이미 겹친 쌍을 서로 막지 않으므로 겹친 채 시작하면 실험이 무의미하다).
    pub fn place(&self, m: &Mid, rho: f64, v0: f64) -> Vec<Body> {
        let n = (rho * self.len * self.width).round() as usize;
        let margin = 0.45;
        let a = (2.0 / (3f64.sqrt() * rho)).sqrt();
        let lanes = ((((self.width - 2.0 * margin) / (a * 3f64.sqrt() / 2.0)).floor() as usize) + 1).max(1);
        let per = n.div_ceil(lanes);
        let rng = mdsim_core::rng::KeyRng::new(7);
        let tmpl = quiet_walker(m);
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            let (lane, j) = (k % lanes, k / lanes);
            let z = if lanes == 1 { 0.0 } else { -self.width / 2.0 + margin + lane as f64 * (self.width - 2.0 * margin) / (lanes - 1) as f64 };
            let x = (j as f64 + if lane % 2 == 1 { 0.5 } else { 0.0 }) * self.len / per as f64 + 0.02 * (rng.u("CORRIDOR", "x", k as u64, 0, 0) - 0.5);
            let mut b = tmpl.clone();
            b.id = k as u32;
            b.entity = format!("corridor#{k}");
            b.pos = V2::new(x.rem_euclid(self.len), z);
            b.heading = 0.0;
            b.gaze = 0.0;
            b.gaze_target = GazeTarget::Path;
            b.activity = Activity::Walk;
            b.base_speed = v0;
            b.radius = 0.25;
            b.fatigue = 0.0;
            b.route = None;
            b.cur_edge = Some(0);
            b.detour = None;
            out.push(b);
        }
        let start = self.min_gap(&out);
        assert!(start >= 0.0, "밀도 {rho}: 처음부터 겹친 쌍(간격 {start:.3})");
        out
    }

    /// 주기 경계를 고려한 쌍 간격(중심 거리 − 반지름 합)의 최솟값.
    pub fn min_gap(&self, bodies: &[Body]) -> f64 {
        let mut deepest = f64::INFINITY;
        for i in 0..bodies.len() {
            for j in (i + 1)..bodies.len() {
                let dx = (bodies[i].pos.x - bodies[j].pos.x).abs();
                let dx = dx.min(self.len - dx);
                let dz = bodies[i].pos.z - bodies[j].pos.z;
                deepest = deepest.min((dx * dx + dz * dz).sqrt() - bodies[i].radius - bodies[j].radius);
            }
        }
        deepest
    }

    /// 몸들을 ticks틱 걷게 하고, warm틱 뒤의 평균 x 속도와 가장 깊은 겹침을 돌려준다.
    pub fn run(&self, w: &World, bodies: &mut [Body], warm: u64, ticks: u64) -> (f64, f64) {
        let env = mdsim_runtime::env::env_at(w.scn, &self.space, w.p.perception.night_light, w.t_s(0));
        let (mut vsum, mut vn) = (0.0, 0usize);
        let mut deepest = f64::INFINITY;
        for t in 0..ticks {
            // 그림자: 경계에서 시야(주변 거리 + 여유) 안의 몸을 반대편에 하나씩 더 둔다
            let mut all: Vec<Body> = bodies.to_vec();
            for b in bodies.iter() {
                for shift in [-self.len, self.len] {
                    let x = b.pos.x + shift;
                    if (-12.0..self.len + 12.0).contains(&x) {
                        let mut g = b.clone();
                        g.id = all.len() as u32;
                        g.pos = V2::new(x, b.pos.z);
                        all.push(g);
                    }
                }
            }
            let coll = Collective::build(&all, &self.space, &env, w.p.crowd.local_radius_m);
            let m_in = mdsim_runtime::locomotion::MoveIn { space: &self.space, env: &env, coll: &coll, p: &w.p.loco, interactions: true, dt: w.dt() };
            // 엔진 한 틱과 같은 순서: 희망 이동 → 이웃 간격 예산(S-E43). 그림자 몸도 같은 희망 이동을 내야
            // 경계 너머 이웃과의 예산이 안쪽과 같다. 앞쪽 n개가 실제 몸이다.
            let mut outs: Vec<Option<_>> = all
                .iter()
                .map(|b| {
                    let drive = mdsim_runtime::locomotion::Drive { browse: false, speed_mult: 1.0, fatigue: 0.0, peripheral_deg: 150.0, peripheral_range: 10.0, gaze_off_path: false, goal: Some(V2::new(b.pos.x + 100.0, b.pos.z)) };
                    Some(mdsim_runtime::locomotion::move_body(&m_in, b, &drive))
                })
                .collect();
            mdsim_runtime::locomotion::limit_steps(&m_in, &all, &mut outs);
            for (b, o) in bodies.iter_mut().zip(outs) {
                let o = o.expect("희망 이동");
                if t >= warm {
                    vsum += o.vel.x;
                    vn += 1;
                }
                b.pos = V2::new(o.pos.x.rem_euclid(self.len), o.pos.z);
                b.vel = o.vel;
                b.heading = o.heading;
            }
            deepest = deepest.min(self.min_gap(bodies));
        }
        (vsum / vn.max(1) as f64, deepest)
    }
}

// ---------- 줄 ----------

/// 빈 실행에 장소 p의 줄 끝에 선 몸을 하나 넣는다(줄 서기 상태의 내국인, 마감이 멂).
pub fn inject_queuer(sim: &mut Sim, m: &Mid, p: u32, k: u32) -> u32 {
    let w = sim.w;
    let id = sim.state.bodies.len() as u32;
    let pl = &w.space.places[p as usize];
    let mut b = quiet_walker(m);
    b.id = id;
    b.entity = format!("queue#{k}");
    b.alive = true;
    b.arrival_tick = sim.state.tick;
    b.deadline_tick = w.scn.n_ticks() + 100_000;
    b.activity = Activity::Queue;
    b.act_place = Some(p);
    b.act_until = 0;
    b.target = Target::None;
    b.route = None;
    b.pending = None;
    b.arrived = false;
    b.pos = pl.anchor + w.queue_dir[p as usize] * (0.6 * (sim.state.queues[p as usize].waiting.len() + 1) as f64);
    if !w.space.walkable_static(b.pos) {
        b.pos = pl.anchor;
    }
    b.cur_edge = Some(pl.edge);
    b.stats = Default::default();
    sim.state.bodies.push(b);
    sim.state.queues[p as usize].waiting.push(id);
    id
}

// ---------- 단계 문맥 ----------

/// 결정 단계가 같은 틱에 함께 읽는 공용 사본(집단 상태·줄 길이·사전 혼잡).
pub struct Shared {
    pub coll: Collective,
    pub queue_len: Vec<u32>,
    pub inside_len: Vec<u32>,
    pub prior: f64,
}

pub fn shared_of(w: &World, bodies: &[Body], queues_waiting: &[u32], queues_inside: &[u32], env: &EnvState, tick: u64) -> Shared {
    Shared { coll: Collective::build(bodies, w.space, env, w.p.crowd.local_radius_m), queue_len: queues_waiting.to_vec(), inside_len: queues_inside.to_vec(), prior: prior_density(w.bundle, &w.p, w.t_s(tick) / 60.0) }
}

pub fn shared_state(w: &World, s: &SimState) -> Shared {
    let ql: Vec<u32> = s.queues.iter().map(|q| q.waiting.len() as u32).collect();
    let il: Vec<u32> = s.queues.iter().map(|q| q.inside.len() as u32).collect();
    shared_of(w, &s.bodies, &ql, &il, &s.env, s.tick)
}

pub fn dctx<'a>(w: &'a World<'_>, env: &'a EnvState, sh: &'a Shared, tick: u64) -> DCtx<'a> {
    DCtx { defs: w.defs, bundle: w.bundle, space: w.space, env, coll: &sh.coll, p: &w.p, scn: w.scn, rng: &w.rng, tick, dt: w.dt(), t_s: w.t_s(tick), prior_density: sh.prior, queue_len: &sh.queue_len, inside_len: &sh.inside_len }
}

pub fn ctx<'a>(w: &'a World<'_>, env: &'a EnvState, tick: u64) -> Ctx<'a> {
    Ctx { space: w.space, env, p: &w.p, dims: &w.defs.dims, tick, dt: w.dt(), prior_density: prior_density(w.bundle, &w.p, w.t_s(tick) / 60.0) }
}

pub fn prof_of<'a>(w: &World<'a>, b: &Body) -> &'a Profile {
    w.profile(b).expect("내국인 몸")
}

/// 옆 오프셋 없는 경로 계획.
pub fn route_plan(path: &Path, final_pos: V2) -> RoutePlan {
    let n = path.edges.len();
    RoutePlan { nodes: path.nodes.clone(), edges: path.edges.clone(), idx: 0, final_pos, speed_factor: 1.0, expected_density: 0.0, lateral: vec![0.0; n], side: vec![0; n] }
}

/// 두 상황(대안 목록)이 같은가: 행동·대상·대안 값·경로·머무는 시간.
pub fn same_choices(a: &[Choice], b: &[Choice]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.action == y.action && x.target == y.target && x.alt == y.alt && x.path == y.path && x.dwell_min == y.dwell_min)
}

pub fn node_at(space: &Space, p: V2) -> u32 {
    space.graph.node_pos.iter().position(|q| q.dist(p) < 1e-6).expect("노드") as u32
}

// ---------- 기록 ----------

/// MemSink에 그대로 넘기면서 10분 묶음 바이트(머리말 + 프레임)를 통째로 남긴다.
#[derive(Default)]
pub struct TeeSink {
    pub mem: MemSink,
    cur: Vec<u8>,
    n_frames: usize,
    /// (시작 틱, 끝 틱, 묶음 바이트)
    pub chunks: Vec<(u64, u64, Vec<u8>)>,
}

impl Sink for TeeSink {
    fn frame(&mut self, tick: u64, recs: &[FrameRec]) {
        self.mem.frame(tick, recs);
        encode_frame(tick, recs, &mut self.cur);
        self.n_frames += 1;
    }

    fn event(&mut self, e: EventRow) {
        self.mem.event(e);
    }

    fn decision(&mut self, d: DecisionRow, alts: Vec<AltRow>) {
        self.mem.decision(d, alts);
    }

    fn chunk_end(&mut self, s: u64, e: u64, state: &SimState, dt_s: f64, render_every: u32) -> mdsim_core::Result<()> {
        let mut bytes = chunk_header(s, e, dt_s, render_every, self.n_frames).into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(&self.cur);
        self.chunks.push((s, e, bytes));
        self.cur.clear();
        self.n_frames = 0;
        self.mem.chunk_end(s, e, state, dt_s, render_every)
    }
}
