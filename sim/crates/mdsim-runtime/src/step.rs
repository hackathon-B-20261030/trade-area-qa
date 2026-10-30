//! 스냅샷과 틱 실행기 (C-225, 02d I-225-r, S-F1).
//!
//! step(S(t)) → S(t+Δt). 단계 순서는 01b §3.1을 따른다. 몸별 계산은 rayon으로 병렬화하되
//! 틱 t의 공용 사본만 읽고, 공용 상태 변경은 몸 순서대로 적용한다(처리 순서·분할 무관, E225-2).

use crate::behavior::{apply_gaze, decide_background, decide_domestic, effective_caps, passive_recovery, DCtx, Effects, QueueOp};
use crate::collective::{Collective, PlaceQueue};
use crate::env::{env_at, env_changes, EnvState};
use crate::locomotion::{limit_steps, MoveOut};
use crate::params::RtParams;
use crate::perception::{collision_risk, effective_fov, look_decide, look_situation, perceive, PerceptOut};
use crate::record::{frame_records, EventRow, FrameRec, Sink};
use crate::situation::Context;
use crate::space::{PlaceKind, Space};
use crate::state::{add_walk_fatigue, Activity, Body, BodyStats, GazeTarget, Target, Trigger};
use crate::summary::SummaryAcc;
use crate::visit::{prior_density, Spawn, SpawnKind};
use mdsim_core::bundle::{Bundle, Profile};
use mdsim_core::defs::Defs;
use mdsim_core::dims::{Dim, NI};
use mdsim_core::geom::{angle_between, point_at, project_on_polyline, V2};
use mdsim_core::rng::KeyRng;
use mdsim_core::scenario::Scenario;
use mdsim_core::{Result, SimError};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// 한 실행의 정적 입력.
pub struct World<'a> {
    pub defs: &'a Defs,
    pub bundle: &'a Bundle,
    pub space: &'a Space,
    pub scn: &'a Scenario,
    pub p: RtParams,
    pub rng: KeyRng,
    pub scenario_sha256: String,
    /// 장소별 줄이 서는 방향(앵커에서 간선을 따라).
    pub queue_dir: Vec<V2>,
}

impl<'a> World<'a> {
    pub fn new(defs: &'a Defs, bundle: &'a Bundle, space: &'a Space, scn: &'a Scenario, scenario_sha256: String) -> Result<World<'a>> {
        let mut p = RtParams::load(&defs.params)?;
        // 정의의 NEXT 맥락이 허용하는 행동만 대안으로 만든다(contexts.yaml, 02c I-207-a).
        if let Some(ctx) = defs.contexts.get("NEXT") {
            p.next_explore = ctx.actions.iter().any(|a| a == "EXPLORE");
            p.next_watch = ctx.actions.iter().any(|a| a == "WATCH");
            p.next_rest_seat = ctx.actions.iter().any(|a| a == "REST_SEAT");
        }
        if let Some(ctx) = defs.contexts.get("REST") {
            p.rest_paid = ctx.actions.iter().any(|a| a == "REST_PAID");
        }
        let queue_dir = space
            .places
            .iter()
            .map(|pl| {
                let es = &space.edges[pl.edge as usize];
                let (s, _) = project_on_polyline(&es.poly, &es.cum, pl.anchor);
                let (_, dir) = point_at(&es.poly, &es.cum, s);
                dir
            })
            .collect();
        Ok(World { defs, bundle, space, scn, p, rng: KeyRng::new(scn.seed), scenario_sha256, queue_dir })
    }

    pub fn dt(&self) -> f64 {
        self.scn.dt_s
    }

    pub fn t_s(&self, tick: u64) -> f64 {
        self.scn.window_s().0 + tick as f64 * self.scn.dt_s
    }

    pub fn profile(&self, b: &Body) -> Option<&'a Profile> {
        b.profile.map(|i| &self.bundle.profiles[i as usize])
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimState {
    pub tick: u64,
    pub bodies: Vec<Body>,
    pub next_spawn: usize,
    pub queues: Vec<PlaceQueue>,
    pub env: EnvState,
    pub decision_seq: u64,
    pub scenario_sha256: String,
    pub defs_sha256: String,
    pub traced: Vec<bool>,
    pub acc: SummaryAcc,
}

pub struct Sim<'a> {
    pub w: &'a World<'a>,
    pub spawns: &'a [Spawn],
    pub state: SimState,
}

/// LOOK 상세를 남길 몸(키 해시 순 상위 n, S-E17). 몸 id = 도착 계획 순번.
pub fn traced_bodies(spawns: &[Spawn], rng: &KeyRng, n: usize) -> Vec<bool> {
    let mut v: Vec<(f64, usize)> = spawns.iter().enumerate().filter(|(_, s)| matches!(s.kind, SpawnKind::Domestic { .. })).map(|(i, s)| (rng.u("TRACE", &s.entity, 0, 0, 0), i)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out = vec![false; spawns.len()];
    for &(_, i) in v.iter().take(n) {
        out[i] = true;
    }
    out
}

impl<'a> Sim<'a> {
    pub fn new(w: &'a World<'a>, spawns: &'a [Spawn]) -> Sim<'a> {
        let env = env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(0));
        let n_ticks = w.scn.n_ticks();
        let state = SimState {
            tick: 0,
            bodies: Vec::new(),
            next_spawn: 0,
            queues: vec![PlaceQueue::default(); w.space.places.len()],
            env,
            decision_seq: 0,
            scenario_sha256: w.scenario_sha256.clone(),
            defs_sha256: w.defs.sha256.clone(),
            traced: traced_bodies(spawns, &w.rng, w.scn.trace_bodies as usize),
            acc: SummaryAcc::new(w.scn.window_s().0, n_ticks, w.scn.dt_s, w.space.edges.len(), w.space.places.len(), w.scn.render_every_ticks as u64),
        };
        Sim { w, spawns, state }
    }

    pub fn from_state(w: &'a World<'a>, spawns: &'a [Spawn], state: SimState) -> Result<Sim<'a>> {
        if state.scenario_sha256 != w.scenario_sha256 || state.defs_sha256 != w.defs.sha256 {
            return Err(SimError::Invariant("체크포인트의 시나리오·정의 해시가 이 실행과 다르다".into()));
        }
        Ok(Sim { w, spawns, state })
    }

    fn spawn(&mut self, s: &Spawn, idx: &Collective, sink: &mut dyn Sink) {
        let w = self.w;
        let id = self.state.bodies.len() as u32;
        let po = &w.space.portals[s.portal as usize];
        let heading = (w.space.graph.node_pos[po.node as usize] - po.pos).angle();
        let tick = self.state.tick;
        let (profile, replicate, base_speed, bg) = match &s.kind {
            SpawnKind::Domestic { profile, replicate } => (Some(*profile), *replicate, w.bundle.profiles[*profile as usize].base_speed_mps, None),
            SpawnKind::Background { itinerary } => (None, 0, w.p.bg.speed_mps, Some(itinerary.clone())),
        };
        let draw = |k: u32| w.rng.u("SPAWN", &s.entity, 0, 1, k);
        let (stroller, luggage, child) = match profile {
            Some(pi) => {
                let pr = &w.bundle.profiles[pi as usize];
                (draw(1) < pr.stroller_prob, draw(2) < pr.luggage_prob, draw(3) < pr.child_prob)
            }
            None => (false, false, false),
        };
        let radius = w.p.loco.body_radius_m + if stroller || child { w.p.loco.companion_radius_add_m } else { 0.0 };
        // 같은 출입구에 함께 도착한 몸이 이웃과 겹치지 않게 1.5m 안에서 자리를 고른다(키 SPAWN, S-E45).
        // 같은 틱에 먼저 놓인 몸은 몸 목록 끝에 있다(몸 번호 순서 = 도착 순서).
        let placed: Vec<(V2, f64)> = self.state.bodies.iter().rev().take_while(|o| o.arrival_tick == tick).map(|o| (o.pos, o.radius)).collect();
        let start = free_spot(idx, &placed, po.pos, radius, SPOT_TRIES, |i| {
            let p = po.pos + V2::from_angle(draw(4 + 2 * i as u32) * std::f64::consts::TAU) * (ARRIVAL_SCATTER_M * draw(5 + 2 * i as u32).sqrt());
            w.space.walkable_static(p).then_some(p)
        });
        let mut b = Body {
            id,
            profile,
            replicate,
            entity: s.entity.clone(),
            arrival_tick: tick,
            deadline_tick: s.deadline_tick,
            portal_in: s.portal,
            mode: s.mode,
            alive: true,
            departed_tick: None,
            portal_out: None,
            pos: start,
            vel: V2::ZERO,
            heading,
            radius,
            base_speed: base_speed * if stroller { 0.85 } else { 1.0 },
            gaze: heading,
            gaze_target: GazeTarget::Path,
            gaze_since: tick,
            activity: Activity::Walk,
            act_until: 0,
            act_place: None,
            target: Target::None,
            purpose_value: 0.0,
            route: None,
            aware: Vec::new(),
            obs_edges: Vec::new(),
            fatigue: 0.0,
            sat: [0.0; NI],
            entered: Vec::new(),
            spent: 0.0,
            stroller,
            luggage,
            child,
            last_decision: [None; 5],
            last_notice_trigger: None,
            new_notices: Vec::new(),
            cur_edge: None,
            arrived: false,
            blocked_ticks: 0,
            jam_until: 0,
            detour: None,
            explored: Vec::new(),
            edge_run_m: 0.0,
            passed: None,
            fallback_u: 0.0,
            defer_until: None,
            choice_margin: 0.0,
            pending: if profile.is_some() { Some(Trigger::Spawned) } else { None },
            avoid: None,
            rest_seated: false,
            last_place: None,
            target_purpose: 0.0,
            stats: BodyStats::default(),
            bg,
        };
        if let Some(pi) = profile {
            known_places(w, &w.bundle.profiles[pi as usize], &mut b, tick);
        }
        sink.event(EventRow { tick, t_s: w.t_s(tick), body: id as i64, kind: "spawn".into(), place: None, value: 0.0, info: po.id.clone() });
        let fi = self.state.acc.frame_ix(tick, w.dt());
        self.state.acc.frames[fi].arrivals += 1;
        self.state.bodies.push(b);
    }

    /// 한 틱을 진행한다.
    pub fn step(&mut self, sink: &mut dyn Sink) -> Result<()> {
        let w = self.w;
        let p = &w.p;
        let dt = w.dt();
        let tick = self.state.tick;
        let t_s = w.t_s(tick);
        // ① 환경
        let env = env_at(w.scn, w.space, p.perception.night_light, t_s);
        for ch in env_changes(&self.state.env, &env) {
            sink.event(EventRow { tick, t_s, body: -1, kind: "env".into(), place: ch.place.map(|i| i as u32), value: ch.value, info: ch.kind.clone() });
        }
        self.state.env = env;
        // ② 도착
        let alive_before = self.state.bodies.iter().filter(|b| b.alive).count();
        let mut spawned = 0usize;
        let mut spawn_idx: Option<Collective> = None;
        while self.state.next_spawn < self.spawns.len() && self.spawns[self.state.next_spawn].tick <= tick {
            let s = self.spawns[self.state.next_spawn].clone();
            let idx = spawn_idx.get_or_insert_with(|| Collective::positions(&self.state.bodies));
            self.spawn(&s, idx, sink);
            self.state.next_spawn += 1;
            spawned += 1;
        }
        // ③ 집단 상태(틱 t의 위치)
        let coll = Collective::build(&self.state.bodies, w.space, &self.state.env, p.crowd.local_radius_m);
        let queue_len: Vec<u32> = self.state.queues.iter().map(|q| q.waiting.len() as u32).collect();
        let inside_len: Vec<u32> = self.state.queues.iter().map(|q| q.inside.len() as u32).collect();
        let minute = t_s / 60.0;
        let prior = prior_density(w.bundle, p, minute);
        // ④ 지각·알아차림·관찰
        let env = &self.state.env;
        let percepts: Vec<Option<PerceptOut>> = self
            .state
            .bodies
            .par_iter()
            .map(|b| match w.profile(b) {
                Some(prof) if b.alive && b.activity.on_street() => Some(perceive(w.space, env, &coll, &queue_len, p, prof, b, &w.rng, tick, dt)),
                _ => None,
            })
            .collect();
        for (b, po) in self.state.bodies.iter_mut().zip(percepts.iter()) {
            b.new_notices.clear();
            let Some(po) = po else { continue };
            for &pl in &po.notices {
                if b.add_aware(pl, tick, false) {
                    b.new_notices.push(pl);
                    b.stats.notices += 1;
                    let zone = if po.visible.iter().any(|&(i, focal)| i == pl && focal) { "focal" } else { "peripheral" };
                    sink.event(EventRow { tick, t_s, body: b.id as i64, kind: "notice".into(), place: Some(pl), value: 0.0, info: zone.into() });
                    let fi = self.state.acc.frame_ix(tick, dt);
                    self.state.acc.frames[fi].notices += 1;
                }
            }
            for &(pl, open, q) in &po.seen {
                let mut reopened = false;
                if let Some(a) = b.aware_mut(pl) {
                    reopened = a.seen_open == Some(false) && open;
                    a.last_seen_tick = Some(tick);
                    a.seen_open = Some(open);
                    a.seen_queue = Some(q);
                }
                // 닫혀 있던 행사가 시작된 것을 보면 새로 알아차린 것처럼 행선지를 다시 볼 계기가 된다(S-E37).
                if reopened && w.space.places[pl as usize].kind == PlaceKind::Event && !b.new_notices.contains(&pl) {
                    b.new_notices.push(pl);
                }
            }
            for &(e, d) in &po.seen_edges {
                b.observe_edge(e, d, tick);
            }
        }
        // ⑤ 시선(LOOK)
        let traced = self.state.traced.clone();
        let look_out: Vec<Option<(crate::record::DecisionRow, Vec<crate::record::AltRow>)>> = self
            .state
            .bodies
            .par_iter_mut()
            .zip(percepts.par_iter())
            .map(|(b, po)| {
                let (Some(prof), Some(po)) = (w.profile(b), po.as_ref()) else {
                    if b.alive && b.activity.on_street() {
                        b.gaze = b.heading;
                    }
                    return None;
                };
                let dwell = (prof.perc.min_dwell_s / dt).round().max(1.0) as u64;
                // 대상 추적: 고개 회전 한계를 넘으면 길로 돌아온다
                if let GazeTarget::Place(pl) = b.gaze_target {
                    let a = (w.space.places[pl as usize].anchor - b.pos).angle();
                    if angle_between(a, b.heading) > prof.perc.head_turn_deg.to_radians() {
                        apply_gaze(w.space, b, GazeTarget::Path, tick, prof.perc.head_turn_deg);
                        return None;
                    }
                    if collision_risk(&coll, b, p.look.interrupt_m) {
                        apply_gaze(w.space, b, GazeTarget::Path, tick, prof.perc.head_turn_deg);
                        return None;
                    }
                }
                if tick < b.gaze_since + dwell {
                    let t = b.gaze_target;
                    b.gaze = crate::perception::gaze_dir(w.space, b, t);
                    return None;
                }
                let rho = coll.local_density.get(b.id as usize).copied().unwrap_or(0.0);
                let choices = look_situation(w.space, &self.state.env, p, &w.defs.dims, b, &po.visible, rho, tick, dt, prof.perc.head_turn_deg);
                let caps = effective_caps(w.defs, prof, b, 1e6);
                let r = look_decide(&choices, prof, b, &w.defs.dims, p, &w.rng, tick, &caps);
                let ch = &choices[r.chosen];
                let target = match ch.target {
                    crate::situation::AltTarget::Place(pl) => GazeTarget::Place(pl),
                    _ if ch.action == crate::situation::Action::Footing => GazeTarget::Footing,
                    _ => GazeTarget::Path,
                };
                let dc = DCtx { defs: w.defs, bundle: w.bundle, space: w.space, env: &self.state.env, coll: &coll, p, scn: w.scn, rng: &w.rng, tick, dt, t_s, prior_density: prior, queue_len: &queue_len, inside_len: &inside_len };
                let rec = if traced.get(b.id as usize).copied().unwrap_or(false) {
                    let row = crate::record::DecisionRow { id: 0, tick, body: b.id as i64, context: Context::Look.name().into(), trigger: Trigger::DwellEnd.name().into(), n_alts: choices.len() as u32, chosen: r.chosen as u32, forced: r.forced, top: r.top3.clone(), base_min: 0.0, wait_min: 0.0, exit_walk_min: 0.0, rng_key: Some(mdsim_core::rng::canonical_key(w.rng.seed, "LOOK", &b.entity, tick, 0, 0)) };
                    let alts = choices
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let t = &r.terms[i];
                            crate::record::AltRow { decision: 0, alt: i as u32, action: c.action.name().into(), target: c.target.label(dc.space), prob: r.probs[i], utility: r.utils[i], purpose: t.purpose, interest: t.interest_total(), cost: t.cost_total(), time: t.time, money: t.money, cost_dims: t.cost }
                        })
                        .collect();
                    Some((row, alts))
                } else {
                    None
                };
                b.last_decision[Context::Look.slot()] = Some(tick);
                apply_gaze(w.space, b, target, tick, prof.perc.head_turn_deg);
                b.gaze_since = tick;
                rec
            })
            .collect();
        for rec in look_out.into_iter().flatten() {
            self.record_decision(rec, sink);
        }
        // ⑥ 계기 판정과 결정
        let effects: Vec<Effects> = {
            let dc = DCtx { defs: w.defs, bundle: w.bundle, space: w.space, env: &self.state.env, coll: &coll, p, scn: w.scn, rng: &w.rng, tick, dt, t_s, prior_density: prior, queue_len: &queue_len, inside_len: &inside_len };
            self.state
                .bodies
                .par_iter_mut()
                .map(|b| match w.profile(b) {
                    Some(prof) => decide_domestic(&dc, prof, b),
                    None => decide_background(&dc, b),
                })
                .collect()
        };
        let mut despawned = 0usize;
        // 이번 틱에 장소에서 나와 새 자리에 놓인 몸(⑥ 앞의 색인에는 장소 안이라 없다).
        let mut placed: Vec<(V2, f64)> = Vec::new();
        for fx in effects {
            for e in fx.events {
                match e.kind.as_str() {
                    "despawn" => {
                        despawned += 1;
                        let fi = self.state.acc.frame_ix(tick, dt);
                        self.state.acc.frames[fi].departures += 1;
                    }
                    "purchase" => {
                        let fi = self.state.acc.frame_ix(tick, dt);
                        let f = &mut self.state.acc.frames[fi];
                        f.purchases += 1;
                        f.revenue += e.value;
                        *f.revenue_by_cat.entry(e.info.clone()).or_default() += e.value;
                    }
                    "enter" => {
                        let fi = self.state.acc.frame_ix(tick, dt);
                        self.state.acc.frames[fi].enters += 1;
                    }
                    _ => {}
                }
                sink.event(e);
            }
            for d in fx.decisions {
                self.record_decision(d, sink);
            }
            for op in fx.ops {
                self.apply_op(op, &coll, &mut placed, sink)?;
            }
        }
        // ⑦ 이동(틱 t의 위치 사본만 읽는다)
        let env = &self.state.env;
        let queues = &self.state.queues;
        let qdir = &w.queue_dir;
        let interactions = w.scn.interactions;
        // 교착 풀기(S-E56): 20~30초 넘게 막힌 몸은 잠시 이웃을 무시하고 움직인다. 언제 풀렸는지 사건으로 남긴다.
        let jam_ticks = (JAM_S / dt).round() as u32;
        let spread = (JAM_SPREAD_S / dt).round().max(1.0) as u32;
        let release_ticks = (RELEASE_S / dt).round() as u64;
        let rejam_ticks = (REJAM_S / dt).round().max(1.0) as u32;
        let mut released: Vec<(u32, u32)> = Vec::new();
        let mut near = Vec::new();
        for b in self.state.bodies.iter_mut() {
            let first = jam_ticks + b.id % spread;
            // 처음은 `first`틱 막힌 때이고, 풀어도 계속 막혀 있으면 `rejam_ticks`마다 다시 푼다(같은 몸의 사건이 쏟아지지 않게).
            if interactions && b.alive && b.activity.moving() && b.jam_until <= tick && b.blocked_ticks >= first && (b.blocked_ticks - first).is_multiple_of(rejam_ticks) {
                // 벽이나 길 문제로 혼자 막힌 몸이 아니라 이웃과 맞닿아 막힌 몸만 푼다.
                coll.neighbors(b.pos, b.radius + JAM_CONTACT_M + 0.5, &mut near);
                if near.iter().any(|&j| j != b.id && b.pos.dist(coll.pos[j as usize]) - b.radius - coll.radius[j as usize] < JAM_CONTACT_M) {
                    b.jam_until = tick + release_ticks;
                    released.push((b.id, b.blocked_ticks));
                }
            }
        }
        for (id, blocked) in released {
            sink.event(EventRow { tick, t_s, body: id as i64, kind: "jam_release".into(), place: None, value: blocked as f64 * dt, info: String::new() });
        }
        // 이동 단계의 이웃 색인: 그 틱 결정 뒤의 활동으로 위치만 다시 만들고, 이웃을 무시하는 몸은 뺀다(S-E43, S-E56).
        let coll_mv = Collective::positions_where(&self.state.bodies, |b| b.jam_until <= tick);
        // ⑦-1 희망 이동: 몸마다 구한다.
        let intents: Vec<Option<(bool, MoveOut)>> = self.state.bodies.par_iter_mut().map(|b| {
            if !b.alive {
                return None;
            }
            // 2초 넘게 막히면 걷기 칸 위 너비 우선 탐색으로 짧은 경유점을 잡아 잠시 따라간다(S-E22).
            if b.activity.moving() && b.blocked_ticks >= 4 && b.detour.is_none() {
                if let Some(r) = b.route.as_ref() {
                    let hint = r.nodes.get(r.idx + 1).map(|&n| w.space.graph.node_pos[n as usize]).unwrap_or(r.final_pos);
                    if let Some(wp) = w.space.local_waypoint(b.pos, hint, 8.0, 3.0, env) {
                        b.detour = Some((wp, tick + (5.0 / dt).ceil() as u64));
                    }
                }
            }
            if let Some((wp, until)) = b.detour {
                if tick >= until || b.pos.dist(wp) <= p.loco.arrive_m {
                    b.detour = None;
                }
            }
            let goal = match b.activity {
                Activity::Queue => b.act_place.map(|pl| {
                    let k = queues[pl as usize].waiting.iter().position(|&x| x == b.id).unwrap_or(0);
                    let anchor = w.space.places[pl as usize].anchor;
                    let slot = anchor + qdir[pl as usize] * ((k + 1) as f64 * p.loco.queue_spacing_m);
                    if w.space.walkable(slot, env) { slot } else { anchor }
                }),
                _ => b.detour.map(|(wp, _)| wp),
            };
            if !(b.activity.moving() || goal.is_some()) {
                b.vel = V2::ZERO;
                return None;
            }
            let prof = w.profile(b);
            let (peri_deg, peri_range) = match prof {
                Some(pr) => {
                    let rho = coll.local_density.get(b.id as usize).copied().unwrap_or(0.0);
                    let f = effective_fov(pr, b, env, rho, p, tick, dt);
                    (f.peri_angle_deg, f.peri_range)
                }
                None => (150.0, 10.0),
            };
            let drive = crate::locomotion::Drive {
                browse: b.activity == Activity::Browse,
                speed_mult: b.route.as_ref().map(|r| r.speed_factor).unwrap_or(1.0),
                fatigue: b.fatigue,
                peripheral_deg: peri_deg,
                peripheral_range: peri_range,
                gaze_off_path: matches!(b.gaze_target, GazeTarget::Place(_)),
                goal,
            };
            let m = crate::locomotion::MoveIn { space: w.space, env, coll: &coll_mv, p: &p.loco, interactions: interactions && b.jam_until <= tick, dt };
            Some((goal.is_some(), crate::locomotion::move_body(&m, b, &drive)))
        }).collect();
        // ⑦-2 이웃 간격 예산: 같은 틱에 서로를 향해 움직여도 겹치지 않게 희망 이동을 깎는다(S-E43).
        let mut outs: Vec<Option<MoveOut>> = intents.iter().zip(self.state.bodies.iter()).map(|(x, b)| if b.jam_until > tick { None } else { x.as_ref().map(|(_, o)| o.clone()) }).collect();
        limit_steps(&crate::locomotion::MoveIn { space: w.space, env, coll: &coll_mv, p: &p.loco, interactions, dt }, &self.state.bodies, &mut outs);
        // 이웃을 무시하는 몸은 예산 없이 희망 이동대로 간다.
        for (i, b) in self.state.bodies.iter().enumerate() {
            if b.jam_until > tick {
                outs[i] = intents[i].as_ref().map(|(_, o)| o.clone());
            }
        }
        // ⑦-3 적용
        self.state.bodies.par_iter_mut().zip(intents.par_iter()).zip(outs.into_par_iter()).for_each(|((b, intent), out)| {
            let (Some((has_goal, _)), Some(out)) = (intent, out) else { return };
            let prof = w.profile(b);
            b.pos = out.pos;
            b.vel = out.vel;
            b.heading = out.heading;
            if let Some(r) = b.route.as_mut() {
                r.idx = out.route_idx.min(r.edges.len());
                if let Some((i, sd)) = out.side_assign {
                    if i < r.side.len() {
                        r.side[i] = sd;
                    }
                }
            }
            if !*has_goal {
                let new_edge = out.cur_edge.or(b.cur_edge);
                if new_edge != b.cur_edge {
                    if let Some(e) = b.cur_edge {
                        if b.edge_run_m >= 0.5 * w.space.edges[e as usize].len {
                            b.passed = Some(e);
                        }
                    }
                    b.edge_run_m = 0.0;
                }
                b.edge_run_m += out.moved;
                b.cur_edge = new_edge;
                if out.arrived && b.target != Target::None {
                    b.arrived = true;
                }
            }
            b.blocked_ticks = if out.blocked { b.blocked_ticks + 1 } else { 0 };
            b.stats.distance_m += out.moved;
            let walk_comfort = prof.map(|pr| pr.comfort[Dim::Walk.ix()]).unwrap_or(3000.0);
            add_walk_fatigue(b, out.moved, out.grade_pct, out.on_steps, walk_comfort, p.state.climb_coef, p.state.steps_coef);
        });
        // 간선 통과 수(edge_pass_count)
        {
            let fi = self.state.acc.frame_ix(tick, dt);
            for b in self.state.bodies.iter_mut() {
                if let Some(e) = b.passed.take() {
                    self.state.acc.frames[fi].edge_pass[e as usize] += 1;
                }
            }
        }
        // ⑧ 줄 처리와 상태 갱신
        for (pi, pl) in w.space.places.iter().enumerate() {
            if matches!(pl.kind, PlaceKind::Bench) {
                continue;
            }
            let q = &mut self.state.queues[pi];
            if q.waiting.is_empty() {
                q.accrue(pl.service_rate, dt);
                continue;
            }
            q.accrue(pl.service_rate, dt);
            while !q.waiting.is_empty() && (q.inside.len() as u32) < pl.capacity && q.credit >= 1.0 {
                let id = q.waiting.remove(0);
                q.inside.push(id);
                q.credit -= 1.0;
                q.served += 1;
                let b = &mut self.state.bodies[id as usize];
                b.set_activity(Activity::Inside).map_err(SimError::Invariant)?;
                b.pending = Some(Trigger::Admitted);
                b.pos = pl.anchor;
                b.vel = V2::ZERO;
            }
            q.max_len = q.max_len.max(q.waiting.len() as u32);
        }
        let env = &self.state.env;
        self.state.bodies.par_iter_mut().for_each(|b| {
            if !b.alive {
                return;
            }
            passive_recovery(b, dt, p);
            if b.activity == Activity::Queue {
                b.stats.queue_wait_s += dt;
            }
            if b.activity == Activity::Inside {
                b.stats.inside_s += dt;
            }
            if b.activity.on_street() {
                match b.gaze_target {
                    GazeTarget::Path => b.stats.gaze_path_s += dt,
                    GazeTarget::Footing => b.stats.gaze_footing_s += dt,
                    GazeTarget::Place(_) => b.stats.gaze_place_s += dt,
                }
                if let Some(prof) = w.profile(b) {
                    let mut sense = b.cur_edge.map(|e| w.space.edges[e as usize].ad).unwrap_or(0.0);
                    for (i, pl) in w.space.places.iter().enumerate() {
                        if pl.kind == PlaceKind::Event && env.event_active[i] && pl.anchor.dist(b.pos) < 15.0 {
                            sense = sense.max(pl.demand[Dim::Sense.ix()]);
                        }
                    }
                    if sense > prof.comfort[Dim::Sense.ix()] {
                        b.stats.sense_over_s += dt;
                    }
                }
            }
        });
        // ⑨ 보존 검사(E225-5)
        let alive_after = self.state.bodies.iter().filter(|b| b.alive).count();
        if alive_after + despawned != alive_before + spawned {
            return Err(SimError::Invariant(format!("틱 {tick}: 활동 인원 보존 실패({alive_before} + {spawned} − {despawned} ≠ {alive_after})")));
        }
        self.state.tick += 1;
        Ok(())
    }

    fn record_decision(&mut self, (mut row, mut alts): (crate::record::DecisionRow, Vec<crate::record::AltRow>), sink: &mut dyn Sink) {
        row.id = self.state.decision_seq;
        for a in alts.iter_mut() {
            a.decision = row.id;
        }
        self.state.decision_seq += 1;
        sink.decision(row, alts);
    }

    fn apply_op(&mut self, op: QueueOp, coll: &Collective, placed: &mut Vec<(V2, f64)>, sink: &mut dyn Sink) -> Result<()> {
        let w = self.w;
        let tick = self.state.tick;
        let t_s = w.t_s(tick);
        match op {
            QueueOp::Join { place, body } => {
                let q = &mut self.state.queues[place as usize];
                q.waiting.push(body);
                q.max_len = q.max_len.max(q.waiting.len() as u32);
                sink.event(EventRow { tick, t_s, body: body as i64, kind: "queue_join".into(), place: Some(place), value: q.waiting.len() as f64, info: String::new() });
            }
            QueueOp::EnterDirect { place, body, until } => {
                let pl = &w.space.places[place as usize];
                let q = &mut self.state.queues[place as usize];
                let b = &mut self.state.bodies[body as usize];
                if (q.inside.len() as u32) < pl.capacity {
                    q.inside.push(body);
                    b.set_activity(Activity::Inside).map_err(SimError::Invariant)?;
                    b.act_until = until;
                    b.pos = pl.anchor;
                    b.vel = V2::ZERO;
                } else {
                    b.set_activity(Activity::Browse).map_err(SimError::Invariant)?;
                    b.act_until = tick + (until - tick) / 2 + 1;
                }
            }
            QueueOp::Leave { place, body } => {
                let anchor = w.space.places[place as usize].anchor;
                let q = &mut self.state.queues[place as usize];
                let was_inside = q.inside.contains(&body);
                q.waiting.retain(|&x| x != body);
                q.inside.retain(|&x| x != body);
                // 장소 안(앵커)에서 거리로 나오는 몸은 이웃과 겹치지 않는 자리에서 걷기를 시작한다(S-E45).
                let b = &self.state.bodies[body as usize];
                if was_inside && b.alive && b.activity.on_street() && b.pos == anchor {
                    let (r, entity) = (b.radius, b.entity.clone());
                    let env = &self.state.env;
                    let spot = free_spot(coll, placed, anchor, r, SPOT_TRIES, |i| {
                        if i == 0 {
                            return Some(anchor);
                        }
                        let u = |k: u32| w.rng.u("SPAWN", &entity, tick, 2, k);
                        let p = anchor + V2::from_angle(u(2 * i as u32) * std::f64::consts::TAU) * (EXIT_SCATTER_M * u(2 * i as u32 + 1).sqrt());
                        w.space.walkable(p, env).then_some(p)
                    });
                    self.state.bodies[body as usize].pos = spot;
                    placed.push((spot, r));
                }
            }
            QueueOp::Sit { place, body } => {
                let pl = &w.space.places[place as usize];
                let q = &mut self.state.queues[place as usize];
                let seated = (q.inside.len() as u32) < pl.capacity;
                if seated {
                    q.inside.push(body);
                }
                self.state.bodies[body as usize].rest_seated = seated;
            }
        }
        Ok(())
    }

    fn focal_of(&self) -> impl Fn(&Body) -> (f64, f64) + '_ {
        move |b: &Body| match self.w.profile(b) {
            Some(pr) => (pr.perc.focal_range_m, pr.perc.focal_angle_deg),
            None => (10.0, 60.0),
        }
    }

    fn render(&mut self, sink: &mut dyn Sink) {
        let tick = self.state.tick;
        let focal = |b: &Body| match self.w.profile(b) {
            Some(pr) => (pr.perc.focal_range_m, pr.perc.focal_angle_deg),
            None => (10.0, 60.0),
        };
        let recs: Vec<FrameRec> = frame_records(&self.state.bodies, &focal);
        sink.frame(tick, &recs);
        let t_s = self.w.t_s(tick);
        let (bodies, acc, env, queues) = (&self.state.bodies, &mut self.state.acc, &self.state.env, &self.state.queues);
        acc.on_render(tick, t_s, self.w.dt(), bodies, self.w.space, env, queues, &focal);
    }

    /// 창 끝까지 돈다. 렌더 프레임은 render_every 틱마다, 10분 묶음 끝에 체크포인트.
    pub fn run(&mut self, sink: &mut dyn Sink) -> Result<()> {
        self.run_until(self.w.scn.n_ticks(), sink)
    }

    pub fn run_until(&mut self, end_tick: u64, sink: &mut dyn Sink) -> Result<()> {
        let re = self.w.scn.render_every_ticks as u64;
        let tpc = self.w.scn.ticks_per_chunk();
        let n = self.w.scn.n_ticks();
        while self.state.tick < end_tick.min(n) {
            if self.state.tick.is_multiple_of(re) {
                self.render(sink);
            }
            self.step(sink)?;
            let t = self.state.tick;
            if t.is_multiple_of(tpc) || t == n {
                let start = (t - 1) / tpc * tpc;
                sink.chunk_end(start, t, &self.state, self.w.dt(), self.w.scn.render_every_ticks)?;
            }
        }
        let _ = self.focal_of();
        Ok(())
    }
}

/// 이만큼(초) 넘게 막혀 있으면 교착으로 보고 이웃을 잠시 무시한다(몸 번호에 따라 `JAM_SPREAD_S`까지 더 기다린다, S-E56).
pub const JAM_S: f64 = 20.0;
pub const JAM_SPREAD_S: f64 = 10.0;
/// 이웃을 무시하며 움직이는 시간(초).
pub const RELEASE_S: f64 = 8.0;
/// 풀어도 계속 막혀 있는 몸을 다시 푸는 간격(초).
pub const REJAM_S: f64 = 300.0;
/// 이 틈(m)보다 가까운 이웃이 있어야 이웃 때문에 막힌 것으로 본다.
pub const JAM_CONTACT_M: f64 = 0.3;
/// 도착·퇴장 자리를 고를 때 보는 후보 수(S-E45).
const SPOT_TRIES: usize = 8;
/// 출입구 도착 몸을 흩는 반경(m).
const ARRIVAL_SCATTER_M: f64 = 1.5;
/// 장소 앵커에서 나오는 몸을 흩는 반경(m).
const EXIT_SCATTER_M: f64 = 1.0;
/// 후보 둘레에서 겹침을 따질 이웃을 찾는 여유 거리(m, 이웃 반지름 최대 0.4 + 여유).
const SPOT_REACH_M: f64 = 1.0;

/// 반지름 `r`인 몸이 이웃과 겹치지 않고 놓일 자리(02d S-E45). 후보 `tries`개를 차례로 보고(`cand(i)`가 None이면 건너뛴다)
/// 이웃과의 틈이 0 이상인 첫 후보를 쓴다. 이웃은 `idx`의 거리 위 몸과 이번 틱에 먼저 놓인 `placed`다.
/// 그런 후보가 없으면 틈이 가장 큰 후보, 후보가 모두 없으면 `fallback`이다.
fn free_spot(idx: &Collective, placed: &[(V2, f64)], fallback: V2, r: f64, tries: usize, cand: impl Fn(usize) -> Option<V2>) -> V2 {
    let mut buf = Vec::new();
    let mut best: Option<(f64, V2)> = None;
    for i in 0..tries {
        let Some(p) = cand(i) else { continue };
        idx.neighbors(p, r + SPOT_REACH_M, &mut buf);
        let mut gap = f64::INFINITY;
        for &j in &buf {
            gap = gap.min(p.dist(idx.pos[j as usize]) - r - idx.radius[j as usize]);
        }
        for &(q, rq) in placed {
            gap = gap.min(p.dist(q) - r - rq);
        }
        if gap >= 0.0 {
            return p;
        }
        if best.is_none_or(|(g, _)| gap > g) {
            best = Some((gap, p));
        }
    }
    best.map_or(fallback, |(_, p)| p)
}

/// 도착할 때 아는 장소(C-214 ⑤): 목적 장소와 친숙도에 비례한 주요 장소(시계성 상위).
fn known_places(w: &World, prof: &Profile, b: &mut Body, tick: u64) {
    for pu in &prof.purposes {
        if let Some(i) = pu.target_place_id.as_deref().and_then(|id| w.space.place_ix(id)) {
            b.add_aware(i as u32, tick, true);
        }
    }
    let way = &prof.comfort[Dim::Way.ix()];
    let fam = ((way - 0.2) / 0.6).clamp(0.0, 1.0);
    let n = (10.0 * fam).round() as usize;
    let mut ranked: Vec<(f64, usize)> = w.space.places.iter().enumerate().filter(|(_, pl)| matches!(pl.kind, PlaceKind::Shop | PlaceKind::Visit)).map(|(i, pl)| (pl.visibility, i)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in ranked.iter().take(n) {
        b.add_aware(i as u32, tick, true);
    }
}
