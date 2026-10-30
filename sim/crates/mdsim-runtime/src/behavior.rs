//! 행동 계층과 결정 계기 (C-222, 02d I-222-r, S-F2·S-F3·S-E8).
//!
//! 한 틱에 몸마다 결정 맥락 하나를 계기 우선순위로 고른다. 결정은 그 몸의 상태와 틱 t의 공용 사본
//! (집단 상태·줄 길이·환경)만 읽으므로 몸별로 병렬 계산하고, 줄 같은 공용 상태 변경은 효과로
//! 모아 몸 순서대로 적용한다(동시 갱신).

use crate::collective::Collective;
use crate::env::EnvState;
use crate::params::RtParams;
use crate::react::{react, Alt, Caps, Person, Reaction};
use crate::record::{AltRow, DecisionRow, EventRow};
use crate::route::Path;
use crate::situation::{next_situation, purchase_situation, rest_situation, route_situation, Action, AltTarget, Choice, Context, Ctx};
use crate::space::{PlaceKind, Space};
use crate::state::{recover, satisfy, spend, Activity, Body, GazeTarget, RoutePlan, Target, Trigger};
use mdsim_core::bundle::{Bundle, Profile};
use mdsim_core::defs::{Defs, LimitKind};
use mdsim_core::dims::{Dim, ND, NI};
use mdsim_core::geom::V2;
use mdsim_core::rng::KeyRng;
use mdsim_core::scenario::{parse_hhmm, Scenario};

pub struct DCtx<'a> {
    pub defs: &'a Defs,
    pub bundle: &'a Bundle,
    pub space: &'a Space,
    pub env: &'a EnvState,
    pub coll: &'a Collective,
    pub p: &'a RtParams,
    pub scn: &'a Scenario,
    pub rng: &'a KeyRng,
    pub tick: u64,
    pub dt: f64,
    pub t_s: f64,
    pub prior_density: f64,
    pub queue_len: &'a [u32],
    pub inside_len: &'a [u32],
}

impl DCtx<'_> {
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx { space: self.space, env: self.env, p: self.p, dims: &self.defs.dims, tick: self.tick, dt: self.dt, prior_density: self.prior_density }
    }

    fn ticks(&self, minutes: f64) -> u64 {
        (minutes * 60.0 / self.dt).round().max(1.0) as u64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum QueueOp {
    /// 줄 끝에 선다(내국인).
    Join { place: u32, body: u32 },
    /// 바로 들어간다(배경 보행자). 자리가 없으면 앞에서 둘러본다.
    EnterDirect { place: u32, body: u32, until: u64 },
    /// 장소(안·줄)에서 나간다.
    Leave { place: u32, body: u32 },
    /// 벤치에 앉는다(자리가 없으면 서서 쉰다).
    Sit { place: u32, body: u32 },
}

#[derive(Clone, Debug, Default)]
pub struct Effects {
    pub ops: Vec<QueueOp>,
    pub events: Vec<EventRow>,
    pub decisions: Vec<(DecisionRow, Vec<AltRow>)>,
}

fn ev(c: &DCtx, b: &Body, kind: &str, place: Option<u32>, value: f64, info: String) -> EventRow {
    EventRow { tick: c.tick, t_s: c.t_s, body: b.id as i64, kind: kind.to_string(), place, value, info }
}

/// 유효 능력(I-206-r): 피로·동반·지출·남은 시간이 편안 상한과 한계를 바꾼다.
pub fn effective_caps(defs: &Defs, prof: &Profile, b: &Body, minutes_left: f64) -> Caps {
    caps_from(defs, prof, b.fatigue, b.stroller || b.luggage, b.spent, minutes_left)
}

/// `effective_caps`의 몸 없는 형태(probe와 공유). `wheels`는 유모차·캐리어 동반.
pub fn caps_from(defs: &Defs, prof: &Profile, fatigue: f64, wheels: bool, spent: f64, minutes_left: f64) -> Caps {
    let mut comfort = prof.comfort;
    let mut limit = prof.limit;
    let mut hard = [false; ND];
    for d in Dim::ALL {
        hard[d.ix()] = defs.dims[d.ix()].limit == LimitKind::Hard;
    }
    let f = 1.0 - 0.5 * fatigue.clamp(0.0, 1.0);
    for d in [Dim::Climb, Dim::Walk] {
        comfort[d.ix()] *= f;
        limit[d.ix()] *= f;
    }
    if wheels {
        comfort[Dim::Step.ix()] = 0.0;
        limit[Dim::Step.ix()] = 1e-6;
        hard[Dim::Step.ix()] = true;
    } else if defs.dims[Dim::Step.ix()].limit == LimitKind::HardForWheels {
        hard[Dim::Step.ix()] = false;
    }
    comfort[Dim::Money.ix()] = (comfort[Dim::Money.ix()] - spent).max(0.0);
    limit[Dim::Money.ix()] = (limit[Dim::Money.ix()] - spent).max(1e-6);
    limit[Dim::Time.ix()] = minutes_left.max(1e-6);
    comfort[Dim::Time.ix()] = comfort[Dim::Time.ix()].min(limit[Dim::Time.ix()]);
    hard[Dim::Time.ix()] = true;
    Caps { comfort, limit, hard }
}

pub fn person(prof: &Profile) -> Person<'_> {
    Person { sens: &prof.sens, interest: &prof.interest, risk: prof.risk, punctuality: prof.punctuality, tau: prof.tau }
}

/// 경로 계획을 만든다(옆 오프셋은 키 LATERAL, S-E14).
pub fn make_route(c: &DCtx, b: &Body, path: &Path, final_pos: V2, speed_factor: f64) -> RoutePlan {
    let lateral = path.edges.iter().map(|(e, _)| c.rng.u("LATERAL", &b.entity, *e as u64, 0, 0) - 0.5).collect();
    let expected_density = path.edges.iter().map(|(e, _)| c.ctx().believed_density(b, *e)).fold(0.0, f64::max);
    RoutePlan { nodes: path.nodes.clone(), edges: path.edges.clone(), idx: 0, final_pos, speed_factor, expected_density, lateral, side: vec![0; path.edges.len()] }
}

fn record(c: &DCtx, b: &Body, ctx: Context, trig: Trigger, choices: &[Choice], r: &Reaction, slot: u64) -> (DecisionRow, Vec<AltRow>) {
    let rng_key = Some(mdsim_core::rng::canonical_key(c.rng.seed, ctx.name(), &b.entity, c.tick, slot, 0));
    let row = DecisionRow { id: 0, tick: c.tick, body: b.id as i64, context: ctx.name().into(), trigger: trig.name().into(), n_alts: choices.len() as u32, chosen: r.chosen as u32, forced: r.forced, top: r.top3.clone(), base_min: 0.0, wait_min: 0.0, exit_walk_min: 0.0, rng_key };
    let alts = choices
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let t = &r.terms[i];
            AltRow { decision: 0, alt: i as u32, action: ch.action.name().into(), target: ch.target.label(c.space), prob: r.probs[i], utility: r.utils[i], purpose: t.purpose, interest: t.interest_total(), cost: t.cost_total(), time: t.time, money: t.money, cost_dims: t.cost }
        })
        .collect();
    (row, alts)
}

fn decide(c: &DCtx, prof: &Profile, b: &Body, ctx: Context, trig: Trigger, choices: &[Choice], slot: u64) -> Reaction {
    let alts: Vec<Alt> = choices.iter().map(|ch| ch.alt.clone()).collect();
    let caps = effective_caps(c.defs, prof, b, b.minutes_left(c.tick, c.dt));
    let u = c.rng.u(ctx.name(), &b.entity, c.tick, slot, 0);
    let _ = trig;
    react(&person(prof), &caps, &b.sat, b.minutes_left(c.tick, c.dt), &alts, &c.defs.dims, &c.p.react, u)
}

/// 출구까지의 대략 시간(분): 들어온 출입구까지 직선거리 × 1.3 ÷ 걸음.
fn est_exit_min(c: &DCtx, b: &Body) -> f64 {
    let po = &c.space.portals[b.portal_in as usize];
    po.pos.dist(b.pos) * 1.3 / b.base_speed.max(0.3) / 60.0
}

/// 계기 판정(우선순위, S-E8). 결정할 맥락과 계기.
fn pick_trigger(c: &DCtx, prof: &Profile, b: &Body) -> Option<(Context, Trigger)> {
    use Activity::*;
    if b.pending == Some(Trigger::Admitted) && b.activity == Inside {
        return Some((Context::Purchase, Trigger::Admitted));
    }
    if matches!(b.activity, Inside | Rest | Watch | Browse) && b.act_until <= c.tick && b.act_until > 0 {
        return Some((Context::Next, Trigger::ActivityEnd));
    }
    if b.arrived && b.target != Target::None {
        return Some((Context::Next, Trigger::Arrived));
    }
    if b.activity == Queue {
        // 줄을 오래 기다리면(마감이 가까우면) 다시 고른다
        let waited = b.stats.queue_wait_s;
        if waited > 600.0 || b.minutes_left(c.tick, c.dt) < est_exit_min(c, b) + 5.0 {
            return Some((Context::Next, Trigger::ObservedChange));
        }
        return None;
    }
    if !matches!(b.activity, Walk | Browse) {
        return None;
    }
    // 연기 중에는 기다림이 끝날 때 경로를 다시 본다(S-E36).
    if let Some(until) = b.defer_until {
        return if c.tick >= until { Some((Context::Route, Trigger::DeferEnd)) } else { None };
    }
    let since = |slot: usize| b.last_decision[slot].map(|t| c.tick.saturating_sub(t) as f64 * c.dt).unwrap_or(f64::INFINITY);
    let leaving = matches!(b.target, Target::Portal(_));
    if !leaving && b.minutes_left(c.tick, c.dt) <= est_exit_min(c, b) + 2.0 && since(0) > 30.0 {
        return Some((Context::Next, Trigger::Deadline));
    }
    if !leaving && b.fatigue >= c.p.state.rest_trigger && since(2) > 60.0 {
        let to_bench = matches!(b.target, Target::Place(p) if c.space.places[p as usize].can_rest);
        if !to_bench {
            return Some((Context::Rest, Trigger::Fatigue));
        }
    }
    if let Target::Place(p) = b.target {
        if b.aware_of(p).and_then(|a| a.seen_open) == Some(false) && since(0) > 5.0 {
            return Some((Context::Next, Trigger::ObservedChange));
        }
        if crate::situation::believed_wait_min(b, &c.ctx(), p) > 10.0 && since(0) > 60.0 {
            return Some((Context::Next, Trigger::ObservedChange));
        }
    }
    if b.pending == Some(Trigger::StartRoute) {
        return Some((Context::Route, Trigger::StartRoute));
    }
    if let Some(r) = &b.route {
        if let Some((ne, _)) = r.next() {
            if let Some(d) = b.seen_density(ne, c.tick, c.ctx().fresh_ticks()) {
                if d > r.expected_density + c.p.route.replan_margin && since(1) > 30.0 {
                    return Some((Context::Route, Trigger::ObservedChange));
                }
            }
        }
    }
    // 이미 알던 행사가 닫힘에서 열림으로 바뀐 것을 보면(행사 시작) 환경 사건 계기다. 새 알아차림과 달리
    // 돋보임 문턱 없이 NEXT를 다시 본다(01b §3.3·§3.6, S-E47). 알던 장소가 알아차림 목록에 드는 길은 S-E37(다시 본
    // 행사 열림)뿐이고, 새로 알아차린 장소는 그 틱에 처음 생긴 항목이다.
    let env_seen = b.new_notices.iter().any(|&p| c.space.places[p as usize].kind == PlaceKind::Event && b.aware_of(p).is_some_and(|a| a.first_tick < c.tick));
    if !leaving && env_seen && since(0) >= c.p.notice.trigger_min_interval_s {
        return Some((Context::Next, Trigger::EnvChange));
    }
    if !b.new_notices.is_empty() && !leaving {
        let interval = b.last_notice_trigger.map(|t| c.tick.saturating_sub(t) as f64 * c.dt).unwrap_or(f64::INFINITY);
        let salient = b.new_notices.iter().any(|&p| {
            let pl = &c.space.places[p as usize];
            (0..NI).map(|k| pl.offer[k] * prof.interest[k] * (1.0 - b.sat[k])).fold(0.0, f64::max) >= c.p.notice.trigger_min_offer
        });
        if salient && interval >= c.p.notice.trigger_min_interval_s && since(0) >= c.p.notice.trigger_min_interval_s {
            return Some((Context::Next, Trigger::Notice));
        }
    }
    if b.target == Target::None {
        return Some((Context::Next, if b.pending == Some(Trigger::Spawned) { Trigger::Spawned } else { Trigger::NoTarget }));
    }
    None
}

/// 내국인 몸 하나의 결정(몸 상태를 직접 바꾸고 공용 효과를 돌려준다).
pub fn decide_domestic(c: &DCtx, prof: &Profile, b: &mut Body) -> Effects {
    let mut fx = Effects::default();
    if !b.alive {
        return fx;
    }
    let Some((ctx, trig)) = pick_trigger(c, prof, b) else { return fx };
    match (ctx, trig) {
        (Context::Purchase, _) => purchase(c, prof, b, &mut fx),
        (Context::Next, Trigger::ActivityEnd) => {
            // 장소 방문(가게 안·행사 보기)의 끝은 exit, 벤치 쉬기의 끝은 rest_end이고 value는 머문 초다(02c §2.6, S-E55).
            let stay_s = c.tick.saturating_sub(b.stats.act_since) as f64 * c.dt;
            if let Some(p) = b.act_place {
                if matches!(b.activity, Activity::Inside | Activity::Rest | Activity::Watch) {
                    fx.ops.push(QueueOp::Leave { place: p, body: b.id });
                }
                match b.activity {
                    Activity::Inside | Activity::Watch => fx.events.push(ev(c, b, "exit", Some(p), stay_s, String::new())),
                    Activity::Rest => fx.events.push(ev(c, b, "rest_end", Some(p), stay_s, String::new())),
                    _ => {}
                }
                b.last_place = Some(p);
            }
            let _ = b.set_activity(Activity::Walk);
            b.rest_seated = false;
            b.act_place = None;
            b.act_until = 0;
            b.target = Target::None;
            b.route = None;
            next(c, prof, b, trig, &mut fx);
        }
        (Context::Next, Trigger::Arrived) => arrived(c, prof, b, &mut fx),
        (Context::Next, Trigger::ObservedChange) if b.activity == Activity::Queue => {
            if let Some(p) = b.act_place {
                fx.ops.push(QueueOp::Leave { place: p, body: b.id });
                fx.events.push(ev(c, b, "queue_leave", Some(p), b.stats.queue_wait_s, "abandoned".into()));
                b.avoid = Some((p, c.tick + c.ticks(10.0)));
            }
            let _ = b.set_activity(Activity::Walk);
            b.rest_seated = false;
            b.act_place = None;
            b.target = Target::None;
            b.route = None;
            next(c, prof, b, trig, &mut fx);
        }
        (Context::Next, _) => next(c, prof, b, trig, &mut fx),
        (Context::Rest, _) => rest(c, prof, b, trig, &mut fx),
        (Context::Route, _) => route(c, prof, b, trig, &mut fx),
        _ => {}
    }
    b.new_notices.clear();
    fx
}

fn apply_goto(c: &DCtx, b: &mut Body, ch: &Choice) {
    match (&ch.target, &ch.path) {
        (AltTarget::Place(p), Some(path)) => {
            let anchor = c.space.places[*p as usize].anchor;
            b.route = Some(make_route(c, b, path, anchor, 1.0));
            b.target = Target::Place(*p);
            b.target_purpose = ch.alt.purpose;
            b.pending = Some(Trigger::StartRoute);
            b.arrived = false;
            if b.activity == Activity::Browse {
                let _ = b.set_activity(Activity::Walk);
            }
        }
        (AltTarget::Node(n), Some(path)) => {
            let pos = c.space.graph.node_pos[*n as usize];
            b.route = Some(make_route(c, b, path, pos, 1.0));
            b.target = Target::Node(*n);
            b.target_purpose = ch.alt.purpose;
            if !b.explored.contains(n) {
                b.explored.push(*n);
            }
            b.pending = None;
            b.arrived = false;
            let _ = b.set_activity(Activity::Walk);
        }
        (AltTarget::Portal(pi), Some(path)) => {
            let pos = c.space.portals[*pi as usize].pos;
            b.route = Some(make_route(c, b, path, pos, 1.0));
            b.target = Target::Portal(*pi);
            b.pending = None;
            b.arrived = false;
            let _ = b.set_activity(Activity::Exit);
        }
        _ => {}
    }
}

fn next(c: &DCtx, prof: &Profile, b: &mut Body, trig: Trigger, fx: &mut Effects) {
    let current = match (trig, b.target) {
        (Trigger::Notice | Trigger::ObservedChange | Trigger::EnvChange, Target::Place(p)) => Some(p),
        _ => None,
    };
    let avoid = b.avoid.filter(|(_, until)| *until > c.tick).map(|(p, _)| p).or(b.last_place.filter(|_| trig == Trigger::ActivityEnd));
    let avoid = if trig == Trigger::ObservedChange { current.or(avoid) } else { avoid };
    let current = if trig == Trigger::ObservedChange { None } else { current };
    let u_explore = c.rng.u("EXPLORE", &b.entity, c.tick, 0, 0);
    let choices = next_situation(&c.ctx(), prof, b, current, avoid, u_explore);
    if choices.is_empty() {
        return;
    }
    let r = decide(c, prof, b, Context::Next, trig, &choices, 0);
    b.fallback_u = (0..choices.len()).filter(|&i| i != r.chosen && r.feasible[i]).map(|i| r.utils[i]).fold(f64::NEG_INFINITY, f64::max);
    if !b.fallback_u.is_finite() {
        b.fallback_u = 0.0;
    }
    b.choice_margin = (b.fallback_u - r.utils[r.chosen]).max(0.0);
    fx.decisions.push(record(c, b, Context::Next, trig, &choices, &r, 0));
    b.last_decision[Context::Next.slot()] = Some(c.tick);
    b.stats.decisions += 1;
    if trig == Trigger::Notice {
        b.last_notice_trigger = Some(c.tick);
    }
    let ch = &choices[r.chosen];
    let same = matches!((ch.target.clone(), b.target), (AltTarget::Place(p), Target::Place(q)) if p == q);
    if !same {
        apply_goto(c, b, ch);
        if ch.action == Action::RestSeat {
            b.pending = None;
        }
    } else {
        b.pending = None;
    }
}

fn rest(c: &DCtx, prof: &Profile, b: &mut Body, trig: Trigger, fx: &mut Effects) {
    let choices = rest_situation(&c.ctx(), prof, b);
    let r = decide(c, prof, b, Context::Rest, trig, &choices, 0);
    fx.decisions.push(record(c, b, Context::Rest, trig, &choices, &r, 0));
    b.last_decision[Context::Rest.slot()] = Some(c.tick);
    b.stats.decisions += 1;
    let ch = &choices[r.chosen];
    match ch.action {
        Action::RestSeat | Action::RestPaid | Action::Leave => {
            apply_goto(c, b, ch);
            b.pending = None;
        }
        _ => {}
    }
}

fn route(c: &DCtx, prof: &Profile, b: &mut Body, trig: Trigger, fx: &mut Effects) {
    b.pending = None;
    b.defer_until = None;
    let (node, final_pos, offer) = match b.target {
        Target::Place(p) => (c.space.places[p as usize].node_near, c.space.places[p as usize].anchor, c.space.places[p as usize].offer),
        _ => return,
    };
    // 경로 대안은 목적지의 매력(목적 가치·제공)을 함께 지니고, 포기는 그것을 잃는다.
    // 고를 때 이긴 몫(choice_margin)을 경로 대안에 그대로 지닌다: 새 정보 없이 다시 뽑아 뒤집지 않는다(S-E23).
    let choices = route_situation(&c.ctx(), b, node, final_pos, b.target_purpose + b.choice_margin, offer, b.fallback_u);
    let r = decide(c, prof, b, Context::Route, trig, &choices, 0);
    fx.decisions.push(record(c, b, Context::Route, trig, &choices, &r, 0));
    b.last_decision[Context::Route.slot()] = Some(c.tick);
    b.stats.decisions += 1;
    let ch = &choices[r.chosen];
    match (ch.action, &ch.path) {
        (Action::Proceed | Action::Detour, Some(path)) => b.route = Some(make_route(c, b, path, final_pos, 1.0)),
        (Action::Pace, Some(path)) => b.route = Some(make_route(c, b, path, final_pos, 1.0 / c.p.route.pace_time)),
        (Action::Defer, _) => {
            // 그 자리에서 기다린다: 경로를 내려놓고 기다림이 끝나면 ROUTE를 다시 부른다.
            b.route = None;
            b.defer_until = Some(c.tick + c.ticks(c.p.route.defer_min).max(1));
        }
        (Action::Abandon, _) => {
            if let Target::Place(p) = b.target {
                b.avoid = Some((p, c.tick + c.ticks(5.0)));
                fx.events.push(ev(c, b, "abandon", Some(p), 0.0, String::new()));
            }
            b.target = Target::None;
            b.route = None;
            b.pending = Some(Trigger::NoTarget);
        }
        _ => {}
    }
}

fn arrived(c: &DCtx, prof: &Profile, b: &mut Body, fx: &mut Effects) {
    b.arrived = false;
    match b.target {
        Target::Portal(pi) => {
            b.alive = false;
            let _ = b.set_activity(Activity::Gone);
            b.departed_tick = Some(c.tick);
            b.portal_out = Some(pi);
            b.route = None;
            fx.events.push(EventRow { tick: c.tick, t_s: c.t_s, body: b.id as i64, kind: "despawn".into(), place: None, value: 0.0, info: c.space.portals[pi as usize].id.clone() });
        }
        Target::Place(p) => {
            let pl = &c.space.places[p as usize];
            b.route = None;
            if let Some(a) = b.aware_mut(p) {
                a.seen_open = Some(c.env.open[p as usize]);
                a.seen_queue = Some(c.queue_len[p as usize]);
                a.last_seen_tick = Some(c.tick);
            }
            match pl.kind {
                PlaceKind::Bench => {
                    fx.ops.push(QueueOp::Sit { place: p, body: b.id });
                    let _ = b.set_activity(Activity::Rest);
                    b.act_place = Some(p);
                    b.act_until = c.tick + c.ticks(c.p.rest.seat_min);
                    b.target = Target::None;
                    b.stats.act_since = c.tick;
                    fx.events.push(ev(c, b, "rest_start", Some(p), 0.0, String::new()));
                }
                PlaceKind::Event => {
                    if c.env.event_active[p as usize] {
                        let end = c.scn.events.iter().filter(|e| e.place_id == pl.id).filter_map(|e| parse_hhmm(&e.end)).fold(0.0, f64::max) * 60.0;
                        let until = (((end - c.t_s) / c.dt).max(1.0) as u64).min(c.ticks(15.0));
                        let _ = b.set_activity(Activity::Watch);
                        b.act_place = Some(p);
                        b.act_until = c.tick + until;
                        b.target = Target::None;
                        satisfy(b, &pl.offer, 1.0);
                        b.stats.visits += 1;
                        // 행사 보기는 장소 방문이다: enter를 내고 끝에 exit를 낸다(Y-17). watch는 Rust 덧붙임(S-E55).
                        b.stats.act_since = c.tick;
                        fx.events.push(ev(c, b, "enter", Some(p), 0.0, String::new()));
                        fx.events.push(ev(c, b, "watch", Some(p), 0.0, String::new()));
                    } else {
                        b.target = Target::None;
                        next(c, prof, b, Trigger::ObservedChange, fx);
                    }
                }
                _ => {
                    if c.env.open[p as usize] {
                        fx.ops.push(QueueOp::Join { place: p, body: b.id });
                        let _ = b.set_activity(Activity::Queue);
                        // 유료 쉼터(카페) 안에서는 앉아 회복한다(S-E53).
                        b.rest_seated = pl.rest_paid;
                        b.act_place = Some(p);
                        b.stats.queue_wait_s = 0.0;
                        b.target = Target::None;
                    } else {
                        b.target = Target::None;
                        next(c, prof, b, Trigger::ObservedChange, fx);
                    }
                }
            }
        }
        Target::Node(_) => {
            // 둘러보기 끝: 새로 알아차린 장소가 있으면 NEXT가 고른다.
            b.route = None;
            b.target = Target::None;
            b.pending = Some(Trigger::NoTarget);
        }
        Target::None => {}
    }
}

fn purchase(c: &DCtx, prof: &Profile, b: &mut Body, fx: &mut Effects) {
    b.pending = None;
    let Some(p) = b.act_place else { return };
    // 줄에서 들어감: queue_leave(served, value = 기다린 초)와 enter를 같은 틱에 내고 머문 시간을 여기서부터 잰다(S-E55).
    b.stats.act_since = c.tick;
    fx.events.push(ev(c, b, "queue_leave", Some(p), b.stats.queue_wait_s, "served".into()));
    let pl = &c.space.places[p as usize];
    b.stats.visits += 1;
    if !b.entered.contains(&p) {
        b.entered.push(p);
    }
    fx.events.push(ev(c, b, "enter", Some(p), b.stats.queue_wait_s, String::new()));
    if pl.kind == PlaceKind::Visit && !pl.can_buy {
        // 방문만 하는 장소(성당·공원): 결정 없이 머문다.
        let dwell = crate::situation::expected_dwell_min(c.p, pl.kind);
        satisfy(b, &pl.offer, c.p.purchase.band_gain.get(1).copied().unwrap_or(1.0));
        b.act_until = c.tick + c.ticks(dwell);
        return;
    }
    let choices = purchase_situation(&c.ctx(), b, p);
    let r = decide(c, prof, b, Context::Purchase, Trigger::Admitted, &choices, p as u64);
    fx.decisions.push(record(c, b, Context::Purchase, Trigger::Admitted, &choices, &r, p as u64));
    b.last_decision[Context::Purchase.slot()] = Some(c.tick);
    b.stats.decisions += 1;
    let ch = &choices[r.chosen];
    satisfy(b, &ch.alt.offer, 1.0);
    if ch.action == Action::Buy {
        spend(b, ch.alt.cost_krw);
        b.stats.purchases += 1;
        fx.events.push(ev(c, b, "purchase", Some(p), ch.alt.cost_krw, pl.category.clone()));
    }
    b.act_until = c.tick + c.ticks(ch.dwell_min);
}

/// 배경 보행자(S-F18): 결정 없이 정해진 장소를 차례로 들른다.
pub fn decide_background(c: &DCtx, b: &mut Body) -> Effects {
    let mut fx = Effects::default();
    if !b.alive {
        return fx;
    }
    let Some(mut it) = b.bg.clone() else { return fx };
    if matches!(b.activity, Activity::Inside | Activity::Browse) && b.act_until <= c.tick && b.act_until > 0 {
        if b.activity == Activity::Inside {
            if let Some(p) = b.act_place {
                fx.ops.push(QueueOp::Leave { place: p, body: b.id });
            }
        }
        let _ = b.set_activity(Activity::Walk);
        b.act_place = None;
        b.act_until = 0;
        b.target = Target::None;
        it.next += 1;
    }
    if b.arrived {
        b.arrived = false;
        match b.target {
            Target::Portal(pi) => {
                b.alive = false;
                let _ = b.set_activity(Activity::Gone);
                b.departed_tick = Some(c.tick);
                b.portal_out = Some(pi);
                b.route = None;
                fx.events.push(EventRow { tick: c.tick, t_s: c.t_s, body: b.id as i64, kind: "despawn".into(), place: None, value: 0.0, info: c.space.portals[pi as usize].id.clone() });
                b.bg = Some(it);
                return fx;
            }
            Target::Place(p) => {
                let dwell = it.dwell_s.get(it.next).copied().unwrap_or(300.0);
                let until = c.tick + (dwell / c.dt).round().max(1.0) as u64;
                b.route = None;
                b.target = Target::None;
                b.act_place = Some(p);
                if c.env.open[p as usize] && c.space.places[p as usize].kind != PlaceKind::Stall {
                    fx.ops.push(QueueOp::EnterDirect { place: p, body: b.id, until });
                } else {
                    let _ = b.set_activity(Activity::Browse);
                    b.act_until = c.tick + (dwell * 0.5 / c.dt).round().max(1.0) as u64;
                }
            }
            Target::Node(_) => {
                b.route = None;
                b.target = Target::None;
            }
            Target::None => {}
        }
    }
    if b.activity == Activity::Walk && b.target == Target::None {
        let from = crate::situation::start_node(c.space, b);
        let speed = b.base_speed;
        let w = |e: u32, _f: bool| c.space.walk_cost(e, speed, c.p.route.crossing_wait_s);
        if it.next < it.stops.len() {
            let p = it.stops[it.next];
            let pl = &c.space.places[p as usize];
            if let Some(path) = c.space.graph.shortest(from, pl.node_near, &w) {
                b.route = Some(make_route(c, b, &path, pl.anchor, 1.0));
                b.target = Target::Place(p);
            } else {
                it.next += 1;
            }
        } else {
            let po = &c.space.portals[it.exit_portal as usize];
            if let Some(path) = c.space.graph.shortest(from, po.node, &w) {
                b.route = Some(make_route(c, b, &path, po.pos, 1.0));
                b.target = Target::Portal(it.exit_portal);
                let _ = b.set_activity(Activity::Exit);
            }
        }
    }
    b.bg = Some(it);
    fx
}

/// 시선 적용(LOOK 결과와 대상 추적). 몸 상태만 바꾼다.
pub fn apply_gaze(space: &Space, b: &mut Body, target: GazeTarget, tick: u64, head_turn_deg: f64) {
    if b.gaze_target != target {
        b.gaze_since = tick;
    }
    b.gaze_target = target;
    b.gaze = crate::perception::gaze_dir(space, b, target);
    // 걷는 중 장소를 가까이 보면 둘러보기(감속), 길로 돌아오면 걷기
    match target {
        GazeTarget::Place(p) if b.activity == Activity::Walk && space.places[p as usize].anchor.dist(b.pos) < 8.0 => {
            let _ = b.set_activity(Activity::Browse);
            b.act_until = 0;
        }
        GazeTarget::Path | GazeTarget::Footing if b.activity == Activity::Browse && b.act_until == 0 => {
            let _ = b.set_activity(Activity::Walk);
        }
        _ => {}
    }
    let _ = head_turn_deg;
}

/// 쉬기·줄·안에서의 회복(틱마다).
pub fn passive_recovery(b: &mut Body, dt: f64, p: &RtParams) {
    let minutes = dt / 60.0;
    match b.activity {
        Activity::Rest => recover(b, minutes, b.rest_seated, p.state.recover_seated_per_min, p.state.recover_standing_per_min),
        // 유료 쉼터 안은 앉아 회복한다(S-E53: 줄에 설 때 정한 rest_seated).
        Activity::Inside => recover(b, minutes, b.rest_seated, p.state.recover_seated_per_min, p.state.recover_standing_per_min),
        Activity::Queue | Activity::Watch => recover(b, minutes, false, p.state.recover_seated_per_min, p.state.recover_standing_per_min),
        _ => {}
    }
}
