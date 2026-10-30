//! E222: 행동 계층과 결정 계기(C-222)의 완료 기준 ①~⑦.
mod common;

use common::{dctx, mid_quick, observe, observed_quick, prep, prof_of, quiet_walker, scenario, shared_state, Mid, Observed, Shared};
use mdsim_core::bundle::Profile;
use mdsim_core::dims::NI;
use mdsim_runtime::behavior::{decide_domestic, Effects, QueueOp};
use mdsim_runtime::collective::Collective;
use mdsim_runtime::record::{AltRow, DecisionRow, EventRow};
use mdsim_runtime::state::{Activity, Aware, Body, Target, Trigger};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// 행사일 변형(15:30~16:30)의 앞 35분: 행사(15:45~16:15)·노점 개장(16:00)이 들어 있어 관람(WATCH) 전이가 생긴다.
fn observed_variant() -> &'static Observed {
    static O: OnceLock<Observed> = OnceLock::new();
    O.get_or_init(|| observe(prep(scenario("variant")), 4200))
}

/// 엔진의 결정 계기 이름(01b §3.3 표 + S-E36의 연기 끝).
fn trigger_names() -> BTreeSet<&'static str> {
    use Trigger::*;
    [Spawned, ActivityEnd, Arrived, Deadline, Fatigue, ObservedChange, Notice, StartRoute, Admitted, NoTarget, EnvChange, DwellEnd, Collision, DeferEnd].iter().map(|t| t.name()).collect()
}

/// 맥락마다 생길 수 있는 계기.
fn allowed_triggers(ctx: &str) -> &'static [&'static str] {
    match ctx {
        "NEXT" => &["도착", "활동 종료", "목표 도착", "마감", "관찰 값 변화", "알아차림", "목표 없음", "환경 사건"],
        "ROUTE" => &["이동 시작", "관찰 값 변화", "연기 끝", "환경 사건"],
        "REST" => &["피로"],
        "PURCHASE" => &["입장"],
        "LOOK" => &["시선 유지 종료", "충돌 위험"],
        _ => &[],
    }
}

/// 모든 결정에 계기(종류)와 틱이 남고, 그 틱에 몸이 살아 있었으며, 계기는 그 맥락의 계기다.
#[test]
fn e222_1_trigger_logged() {
    let names = trigger_names();
    for o in [observed_quick(), observed_variant()] {
        let n_ticks = o.snaps.len() as u64 - 1;
        let mut by_ctx: BTreeMap<String, usize> = BTreeMap::new();
        for d in &o.sink.decisions {
            assert!(names.contains(d.trigger.as_str()), "결정 {}: 모르는 계기 '{}'", d.id, d.trigger);
            assert!(allowed_triggers(&d.context).contains(&d.trigger.as_str()), "결정 {}: {}에 계기 '{}'", d.id, d.context, d.trigger);
            assert!(d.tick < n_ticks);
            let b = &o.state.bodies[d.body as usize];
            assert!(b.arrival_tick <= d.tick && b.departed_tick.map(|t| d.tick <= t).unwrap_or(true), "결정 {}: 몸이 없는 틱", d.id);
            *by_ctx.entry(d.context.clone()).or_default() += 1;
        }
        for c in ["NEXT", "ROUTE", "PURCHASE", "LOOK"] {
            assert!(by_ctx.get(c).copied().unwrap_or(0) > 10, "{c} 결정 수 {by_ctx:?}");
        }
    }
}

fn walker_with_ctx(m: &Mid) -> (Body, &'static Profile, Shared) {
    let w = m.w();
    let mut b = quiet_walker(m);
    b.obs_edges.clear(); // 관찰 값 변화 계기가 없게(본 혼잡 없음)
    let prof = prof_of(w, &b);
    (b, prof, shared_state(w, &m.state))
}

fn decide_at(m: &Mid, sh: &Shared, prof: &Profile, b: &mut Body, tick: u64) -> Effects {
    let c = dctx(m.w(), &m.state.env, sh, tick);
    decide_domestic(&c, prof, b)
}

/// 이 몸이 끌릴 만한(제공 × 관심 ≥ 계기 문턱) 열린 가게 하나.
fn salient_place(m: &Mid, prof: &Profile, b: &Body) -> u32 {
    let w = m.w();
    let thr = w.p.notice.trigger_min_offer;
    (0..w.space.places.len() as u32)
        .filter(|&p| {
            let pl = &w.space.places[p as usize];
            pl.go_to && m.state.env.open[p as usize] && Some(p) != match b.target { Target::Place(t) => Some(t), _ => None }
        })
        .max_by(|&x, &y| {
            let s = |p: u32| (0..NI).map(|k| w.space.places[p as usize].offer[k] * prof.interest[k] * (1.0 - b.sat[k])).fold(0.0, f64::max);
            s(x).total_cmp(&s(y)).then(y.cmp(&x))
        })
        .filter(|&p| (0..NI).map(|k| w.space.places[p as usize].offer[k] * prof.interest[k] * (1.0 - b.sat[k])).fold(0.0, f64::max) >= thr)
        .expect("끌릴 만한 가게")
}

fn notice(b: &mut Body, p: u32, tick: u64) {
    if b.aware_of(p).is_none() {
        let i = b.aware.partition_point(|a| a.place < p);
        b.aware.insert(i, Aware { place: p, first_tick: tick, last_seen_tick: Some(tick), seen_open: Some(true), seen_queue: Some(0), known_before: false });
    }
    b.new_notices = vec![p];
}

/// 계기가 없는 틱에는 결정하지 않는다: 걷는 중이고 처리할 계기가 없는 몸은 200초 동안 결정하지 않고,
/// 합성 사건(새 알아차림, 목표 도착) 하나를 주면 그 계기의 결정을 정확히 하나 한다.
/// 실행 기록에서도 몸마다 한 틱에 LOOK 밖의 결정은 많아야 하나다(S-E8).
#[test]
fn e222_2_no_trigger_no_decision() {
    let m = mid_quick();
    let (b0, prof, sh) = walker_with_ctx(m);
    let t0 = m.state.tick;
    for k in 0..400 {
        let mut b = b0.clone();
        let fx = decide_at(m, &sh, prof, &mut b, t0 + k);
        assert!(fx.decisions.is_empty() && fx.ops.is_empty() && fx.events.is_empty(), "계기 없는 틱 {}에 결정·효과", t0 + k);
        assert_eq!(b, b0, "계기 없는 틱에 몸 상태가 바뀜");
    }
    let p = salient_place(m, prof, &b0);
    let mut b = b0.clone();
    notice(&mut b, p, t0);
    let fx = decide_at(m, &sh, prof, &mut b, t0);
    assert_eq!(fx.decisions.len(), 1, "새 알아차림 → 결정 하나");
    assert_eq!((fx.decisions[0].0.context.as_str(), fx.decisions[0].0.trigger.as_str()), ("NEXT", Trigger::Notice.name()));
    let mut b = b0.clone();
    b.arrived = true;
    let fx = decide_at(m, &sh, prof, &mut b, t0);
    assert!(fx.decisions.len() <= 1, "목표 도착 → 결정은 많아야 하나(줄 서기·앉기는 결정이 아님)");
    assert!(!fx.ops.is_empty() || !fx.decisions.is_empty() || b.activity != b0.activity, "목표 도착을 처리함");
    for o in [observed_quick(), observed_variant()] {
        let mut per: BTreeMap<(i64, u64), usize> = BTreeMap::new();
        for d in o.sink.decisions.iter().filter(|d| d.context != "LOOK") {
            *per.entry((d.body, d.tick)).or_default() += 1;
        }
        assert!(per.values().all(|&n| n == 1), "한 틱에 몸 하나가 둘 이상 결정");
    }
}

/// 01b §3.3 상태도의 전이(둘러보기는 걷기 안의 감속 상태). 도착 → 걷기는 생성으로 본다.
const DIAGRAM: [(Activity, Activity); 14] = {
    use Activity::*;
    [(Walk, Browse), (Browse, Walk), (Walk, Queue), (Queue, Inside), (Queue, Walk), (Walk, Inside), (Inside, Walk), (Walk, Rest), (Rest, Walk), (Walk, Watch), (Watch, Walk), (Walk, Exit), (Rest, Exit), (Exit, Gone)]
};

/// 틱 사이에 보인 전이 a → b가 상태도로 설명되는가: 직접 전이이거나, 한 틱 안의 결정이 활동을 끝내고(→ 걷기)
/// 곧바로 다음 활동을 시작한(걷기 →) 경우처럼 걷기·둘러보기만 거치는 연쇄.
fn explained(a: Activity, b: Activity) -> bool {
    let edge = |x: Activity, y: Activity| DIAGRAM.contains(&(x, y));
    if edge(a, b) {
        return true;
    }
    let mids = [Activity::Walk, Activity::Browse];
    mids.iter().any(|&m1| edge(a, m1) && (edge(m1, b) || mids.iter().any(|&m2| edge(m1, m2) && edge(m2, b))))
}

/// 활동 전이는 허용 표(01b §3.3)에 있는 것만 일어난다. 엔진의 허용 표는 상태도의 전이를 모두 담고, 실행에서 틱마다
/// 본 전이는 모두 상태도로 설명된다(빠른 시나리오 30분 + 행사일 변형 35분).
#[test]
fn e222_3_allowed_transitions() {
    for (a, b) in DIAGRAM {
        assert!(a.allowed(b), "엔진 허용 표에 상태도 전이 {a:?} → {b:?}가 없음");
    }
    let mut seen: BTreeMap<(Activity, Activity), usize> = BTreeMap::new();
    for o in [observed_quick(), observed_variant()] {
        for t in 0..o.snaps.len() - 1 {
            for s in &o.snaps[t + 1] {
                // 그 틱에 도착한 몸은 도착 → 걷기에서 시작한다(생성)
                let prev = match o.snaps[t].get(s.id as usize) {
                    Some(p) if !p.alive => {
                        assert!(!s.alive, "사라진 몸이 돌아옴");
                        continue;
                    }
                    Some(p) => p.activity,
                    None => Activity::Walk,
                };
                if prev != s.activity {
                    assert!(explained(prev, s.activity), "몸 {} 틱 {t}: 허용되지 않은 전이 {:?} → {:?}", s.id, prev, s.activity);
                    *seen.entry((prev, s.activity)).or_default() += 1;
                }
                assert_eq!(s.alive, s.activity != Activity::Gone, "사라짐과 살아 있음이 맞음");
            }
        }
    }
    // 행사 보기는 드물어(02d §6) 실행 표본에 없을 수 있다. 진행 중인 행사에 닿은 몸 하나로 Walk → Watch를 직접 본다.
    let (a, b) = watch_transition();
    assert!(explained(a, b), "행사 도착 전이 {a:?} → {b:?}");
    *seen.entry((a, b)).or_default() += 1;
    for need in [(Activity::Walk, Activity::Browse), (Activity::Exit, Activity::Gone), (Activity::Walk, Activity::Watch)] {
        assert!(seen.contains_key(&need), "{need:?} 전이를 한 번은 봐야 시험이 의미가 있다: {seen:?}");
    }
}

/// 진행 중인 행사(행사일 15:45~16:15)의 앵커에 닿은 걷는 몸이 다음 틱에 보는 전이(빈 실행, 15:50).
fn watch_transition() -> (Activity, Activity) {
    let m = mid_quick();
    let pr = common::prep_empty(common::with_window(scenario("variant"), "15:50", "15:55"));
    let w = pr.w;
    let ev = w.space.places.iter().position(|p| p.kind == mdsim_runtime::space::PlaceKind::Event).expect("행사 장소") as u32;
    let mut sim = pr.sim();
    let mut b = quiet_walker(m);
    b.id = 0;
    b.entity = "watch#0".into();
    b.arrival_tick = 0;
    b.deadline_tick = w.scn.n_ticks() + 100_000;
    b.pos = w.space.places[ev as usize].anchor;
    b.cur_edge = Some(w.space.places[ev as usize].edge);
    b.target = Target::Place(ev);
    b.route = None;
    b.arrived = true;
    b.pending = None;
    b.activity = Activity::Walk;
    sim.state.bodies.push(b);
    let mut sink = mdsim_runtime::record::MemSink::default();
    sim.run_until(1, &mut sink).expect("step");
    (Activity::Walk, sim.state.bodies[0].activity)
}

/// 행선지 대안은 고려 집합 안에서만 생기고, 결정은 그 틱의 관찰 값(믿는 세계)만 읽는다: 몸이 보지 못한 참값
/// (장소의 실제 영업 여부, 실제 줄 길이, 실제 간선 밀도)을 바꿔도 같은 결정이 나온다.
#[test]
fn e222_4_believed_world_only() {
    // (1) 실행 기록: NEXT·REST의 장소 대안은 결정 틱에 이미 인지 집합에 있었다
    for o in [observed_quick(), observed_variant()] {
        let space = o.prep.w.space;
        let alts: BTreeMap<u64, Vec<&AltRow>> = o.sink.alts.iter().fold(BTreeMap::new(), |mut m, a| {
            m.entry(a.decision).or_insert_with(Vec::new).push(a);
            m
        });
        for d in o.sink.decisions.iter().filter(|d| d.context == "NEXT" || d.context == "REST") {
            let b = &o.state.bodies[d.body as usize];
            for a in alts.get(&d.id).into_iter().flatten() {
                if let Some(id) = a.target.strip_prefix("place:") {
                    let p = space.place_ix(id).unwrap() as u32;
                    assert!(b.aware_of(p).map(|x| x.first_tick <= d.tick).unwrap_or(false), "결정 {}: 고려 집합 밖 {id}", d.id);
                }
            }
        }
    }
    // (2) 같은 믿음, 다른 참값
    let m = mid_quick();
    let w = m.w();
    let (b0, prof, sh) = walker_with_ctx(m);
    let t0 = m.state.tick;
    let p = salient_place(m, prof, &b0);
    let mut b = b0.clone();
    notice(&mut b, p, t0);
    let run = |env: &mdsim_runtime::env::EnvState, sh: &Shared, b: &Body| -> (Vec<(DecisionRow, Vec<AltRow>)>, Body) {
        let mut bb = b.clone();
        let c = dctx(w, env, sh, t0);
        let fx = decide_domestic(&c, prof, &mut bb);
        (fx.decisions, bb)
    };
    let base = run(&m.state.env, &sh, &b);
    assert_eq!(base.0.len(), 1);
    // 몸이 본 적 없는(또는 신선한 관찰이 없는) 장소들이 실제로는 닫혀 있다
    let mut closed = m.state.env.clone();
    let mut n_flip = 0;
    for (i, o) in closed.open.iter_mut().enumerate() {
        let seen = b.aware_of(i as u32).and_then(|a| a.seen_open).is_some();
        if !seen && w.space.places[i].kind != mdsim_runtime::space::PlaceKind::Event {
            *o = false;
            n_flip += 1;
        }
    }
    assert!(n_flip > 10);
    assert_eq!(run(&closed, &sh, &b), base, "보지 못한 영업 종료가 결정을 바꿈");
    // 실제 줄이 길고 실제 간선이 붐빈다(몸은 그 줄·밀도를 보지 못했다)
    let crowded = Shared {
        coll: {
            let mut c: Collective = sh.coll.clone();
            for d in c.edge_density.iter_mut() {
                *d += 3.0;
            }
            c
        },
        queue_len: sh.queue_len.iter().map(|q| q + 40).collect(),
        inside_len: sh.inside_len.clone(),
        prior: sh.prior,
    };
    assert_eq!(run(&m.state.env, &crowded, &b), base, "보지 못한 혼잡·줄이 결정을 바꿈");
}

/// 결정(효과·몸 상태)의 키: 몸 번호 대신 개체 ID를 쓴다(몸 번호를 다시 매긴 세계와 비교하려고).
type DecisionKey = (Vec<(DecisionRow, Vec<AltRow>)>, Vec<EventRow>, Vec<QueueOp>, Body);

fn decision_key(fx: Effects, mut b: Body, id_to: &dyn Fn(u32) -> u32) -> DecisionKey {
    let decisions = fx
        .decisions
        .into_iter()
        .map(|(mut d, a)| {
            d.body = id_to(d.body as u32) as i64;
            (d, a)
        })
        .collect();
    let events = fx
        .events
        .into_iter()
        .map(|mut e| {
            if e.body >= 0 {
                e.body = id_to(e.body as u32) as i64;
            }
            e
        })
        .collect();
    let ops = fx
        .ops
        .into_iter()
        .map(|op| match op {
            QueueOp::Join { place, body } => QueueOp::Join { place, body: id_to(body) },
            QueueOp::EnterDirect { place, body, until } => QueueOp::EnterDirect { place, body: id_to(body), until },
            QueueOp::Leave { place, body } => QueueOp::Leave { place, body: id_to(body) },
            QueueOp::Sit { place, body } => QueueOp::Sit { place, body: id_to(body) },
        })
        .collect();
    b.id = id_to(b.id);
    (decisions, events, ops, b)
}

/// 같은 S(t)와 키는 몸 처리 순서와 무관하게 같은 결정을 낸다: 실행 중 여러 틱의 상태에서 모든 내국인 몸의 결정을
/// 정순·역순·병렬로, 그리고 몸 번호를 거꾸로 매긴 세계에서 다시 계산해 비교한다.
#[test]
fn e222_5_order_invariance() {
    let pr = prep(scenario("quick"));
    let w = pr.w;
    let mut sim = pr.sim();
    let mut sink = mdsim_runtime::record::MemSink::default();
    let mut n_dec = 0;
    for stop in [600u64, 1050, 1500, 1950, 2400, 2850, 3300] {
        sim.run_until(stop, &mut sink).expect("run");
        let st = &sim.state;
        let sh = shared_state(w, st);
        let c = dctx(w, &st.env, &sh, st.tick);
        let ids: Vec<usize> = st.bodies.iter().filter(|b| b.alive && !b.is_background()).map(|b| b.id as usize).collect();
        // 계기가 드물어 그대로 두면 결정이 거의 없다: 걷는 몸에는 '목표 없음' 계기를 주어 NEXT를 부른다
        let with_trigger = |mut b: Body| -> Body {
            if matches!(b.activity, Activity::Walk | Activity::Browse) && !matches!(b.target, Target::Portal(_)) {
                b.target = Target::None;
                b.route = None;
                b.pending = Some(Trigger::NoTarget);
            }
            b
        };
        let one = |i: usize| -> DecisionKey {
            let mut b = with_trigger(st.bodies[i].clone());
            let fx = decide_domestic(&c, prof_of(w, &b), &mut b);
            decision_key(fx, b, &|x| x)
        };
        let fwd: Vec<DecisionKey> = ids.iter().map(|&i| one(i)).collect();
        let mut rev: Vec<DecisionKey> = ids.iter().rev().map(|&i| one(i)).collect();
        rev.reverse();
        let par: Vec<DecisionKey> = ids.par_iter().map(|&i| one(i)).collect();
        assert!(fwd == rev && fwd == par, "틱 {}: 처리 순서에 따라 결정이 다름", st.tick);
        // 몸 번호를 거꾸로 매긴 같은 세계
        let n = st.bodies.len() as u32;
        let flip = move |x: u32| n - 1 - x;
        let mut bodies: Vec<Body> = st.bodies.iter().rev().cloned().collect();
        for b in bodies.iter_mut() {
            b.id = flip(b.id);
        }
        let sh2 = Shared { coll: Collective::build(&bodies, w.space, &st.env, w.p.crowd.local_radius_m), queue_len: sh.queue_len.clone(), inside_len: sh.inside_len.clone(), prior: sh.prior };
        let c2 = dctx(w, &st.env, &sh2, st.tick);
        for (k, &i) in ids.iter().enumerate() {
            let mut b = with_trigger(bodies[flip(i as u32) as usize].clone());
            let fx = decide_domestic(&c2, prof_of(w, &b), &mut b);
            assert!(decision_key(fx, b, &flip) == fwd[k], "틱 {} 몸 {i}: 몸 번호를 바꾸니 결정이 다름", st.tick);
            n_dec += fwd[k].0.len();
        }
    }
    assert!(n_dec > 40, "비교한 결정 {n_dec}");
}

/// 떠나기를 고른 몸(활동 EXIT)은 출구에 닿아 사라지거나, 막힘이 감지되어 보고된다. 떠나기 시작한 뒤 넉넉한 시간
/// (출구까지 그래프 거리를 세 번 걸을 시간 + 1분)이 지났는데 남아 있는 몸은 막힘 표시(blocked_ticks)라도 있어야 한다.
#[test]
fn e222_6_leavers_exit() {
    let stuck = leavers(observed_quick()).1;
    assert!(stuck.is_empty(), "출구에 닿지 못하고 보고도 없는 떠나는 몸 {}명: {:?}", stuck.len(), &stuck[..stuck.len().min(6)]);
}

/// 앞보기 점이 다음 간선으로 넘어갔는데 그 점이 진행 방향으로 몸보다 뒤면 그 틱에 다음 간선으로 넘기고 되돌아가지
/// 않는다(S-E52). 합성 자료의 전체 폭 간선 E:w7001:4 끝(왕복이 있던 (207,66))에서 넓은 보도 간선 E:w7007:1로 꺾는 몸을
/// 이웃 없이 40틱 걷게 한다: 경로 인덱스는 줄지 않고, x 방향 속도의 부호가 틱마다 바뀌는 왕복이 없으며, 출구 쪽으로 나아간다.
#[test]
fn e222_6_spill_target_behind_advances() {
    let m = mid_quick();
    let w = m.w();
    let ix = |id: &str| w.space.edges.iter().position(|e| e.id == id).unwrap_or_else(|| panic!("간선 {id}")) as u32;
    let (e7, e26) = (ix("E:w7001:4"), ix("E:w7007:1"));
    let (a7, a26) = (&w.space.edges[e7 as usize], &w.space.edges[e26 as usize]);
    assert_eq!(a7.b, a26.b, "두 간선은 (210,68)에서 만난다");
    let path = mdsim_runtime::route::Path { nodes: vec![a7.a, a7.b, a26.a], edges: vec![(e7, true), (e26, false)], length: a7.len + a26.len };
    let final_pos = mdsim_core::geom::V2::new(198.5, 34.0);
    let (c0, _) = mdsim_core::geom::point_at(&a7.poly, &a7.cum, a7.len - 2.8);
    let pos = c0 + mdsim_core::geom::V2::new(0.0, -1.98);
    let mut b = common::lone(m, pos, 0.0, 0.0);
    b.route = Some(common::route_plan(&path, final_pos));
    b.cur_edge = Some(e7);
    b.activity = Activity::Exit;
    let coll = Collective::default();
    let mi = mdsim_runtime::locomotion::MoveIn { space: w.space, env: &m.state.env, coll: &coll, p: &w.p.loco, interactions: false, dt: w.dt() };
    let d = mdsim_runtime::locomotion::Drive { browse: false, speed_mult: 1.0, fatigue: 0.0, peripheral_deg: 150.0, peripheral_range: 10.0, gaze_off_path: false, goal: None };
    let (mut last_idx, mut flips, mut prev_vx) = (0usize, 0usize, 0.0f64);
    let start_dist = b.pos.dist(final_pos);
    for t in 0..40 {
        let o = mdsim_runtime::locomotion::move_body(&mi, &b, &d);
        assert!(o.route_idx >= last_idx, "틱 {t}: 경로 인덱스가 {last_idx} → {}로 돌아감", o.route_idx);
        last_idx = o.route_idx;
        if prev_vx * o.vel.x < -1e-6 {
            flips += 1;
        }
        prev_vx = o.vel.x;
        let r = b.route.as_mut().unwrap();
        r.idx = o.route_idx;
        if let Some((i, sd)) = o.side_assign {
            r.side[i] = sd;
        }
        b.pos = o.pos;
        b.heading = o.heading;
    }
    assert_eq!(last_idx, 1, "다음 간선으로 넘어감");
    assert!(flips <= 1, "x 방향 왕복 {flips}번");
    assert!(b.pos.dist(final_pos) < start_dist - 5.0, "출구 쪽으로 나아감: {start_dist:.1} → {:.1}", b.pos.dist(final_pos));
}

/// 벤치로 가던 몸이 NEXT를 다시 볼 때 '현재 계획 유지' 대안은 그 벤치의 쉬기 대안과 같은 값이고(요구·목적·시간이
/// REST 맥락의 REST_SEAT와 같음), 정의의 NEXT 행동에 REST_SEAT가 없으면 GO_TO로 적힌다(S-E54). 다른 행선지는 전환 비용을 진다.
#[test]
fn e222_7_bench_plan_kept_value() {
    let m = mid_quick();
    let w = m.w();
    let c = common::ctx(w, &m.state.env, m.state.tick);
    let mut n = 0;
    for (i, pl) in w.space.places.iter().enumerate().filter(|(_, p)| p.kind == mdsim_runtime::space::PlaceKind::Bench) {
        let bench = i as u32;
        let mut b = quiet_walker(m);
        b.pos = pl.anchor;
        b.cur_edge = Some(pl.edge);
        b.fatigue = 0.9;
        if b.aware_of(bench).is_none() {
            let k = b.aware.partition_point(|a| a.place < bench);
            b.aware.insert(k, Aware { place: bench, first_tick: 0, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: true });
        }
        let prof = prof_of(w, &b);
        let rest = mdsim_runtime::situation::rest_situation(&c, prof, &b);
        let Some(seat) = rest.iter().find(|ch| ch.action == mdsim_runtime::situation::Action::RestSeat && ch.target == mdsim_runtime::situation::AltTarget::Place(bench)) else { continue };
        b.target = Target::Place(bench);
        let u = w.rng.u("EXPLORE", &b.entity, m.state.tick, 0, 0);
        let next = mdsim_runtime::situation::next_situation(&c, prof, &b, Some(bench), None, u);
        let keep: Vec<_> = next.iter().filter(|ch| ch.target == mdsim_runtime::situation::AltTarget::Place(bench)).collect();
        assert_eq!(keep.len(), 1, "벤치 {}: 유지 대안 하나", pl.id);
        let k = keep[0];
        assert_eq!(k.action, if w.p.next_rest_seat { mdsim_runtime::situation::Action::RestSeat } else { mdsim_runtime::situation::Action::GoTo });
        assert_eq!((k.alt.demand, k.alt.purpose, k.alt.dt_min, k.alt.cost_krw), (seat.alt.demand, seat.alt.purpose, seat.alt.dt_min, seat.alt.cost_krw), "벤치 {}: REST의 쉬기와 같은 값", pl.id);
        n += 1;
    }
    assert!(n >= 2, "본 벤치 {n}");
}

/// 떠나서 사라진 몸은 모두 목표 출구에서 사라진다(E222-6의 앞 절반).
#[test]
fn e222_6_despawn_at_exit_portal() {
    let (n_gone, _) = leavers(observed_quick());
    assert!(n_gone > 20, "떠나서 사라진 몸 {n_gone}");
}

/// (출구에서 사라진 떠나는 몸 수, 넉넉한 시간이 지나도 남아 있고 막힘 표시도 없는 몸).
fn leavers(o: &Observed) -> (usize, Vec<String>) {
    let w = o.prep.w;
    let dt = w.dt();
    let end = (o.snaps.len() - 1) as u64;
    let despawn: BTreeMap<i64, &EventRow> = o.sink.events.iter().filter(|e| e.kind == "despawn").map(|e| (e.body, e)).collect();
    let mut exit_start: BTreeMap<u32, (u64, mdsim_core::geom::V2)> = BTreeMap::new();
    for (t, snap) in o.snaps.iter().enumerate() {
        for s in snap.iter().filter(|s| s.alive && s.activity == Activity::Exit) {
            exit_start.entry(s.id).or_insert((t as u64, s.pos));
        }
    }
    let mut n_gone = 0;
    let mut stuck = Vec::new();
    for (&id, &(te, pos)) in &exit_start {
        let b = &o.state.bodies[id as usize];
        let Target::Portal(pi) = b.portal_out.map(Target::Portal).unwrap_or(b.target) else { continue };
        let po = &w.space.portals[pi as usize];
        if !b.alive {
            let e = despawn[&(id as i64)];
            assert_eq!(e.info, po.id, "몸 {id}: 사라진 출입구");
            assert!(b.pos.dist(po.pos) <= w.p.loco.arrive_m + 1e-9, "몸 {id}: 출구에서 {:.2}m 떨어져 사라짐", b.pos.dist(po.pos));
            n_gone += 1;
            continue;
        }
        let from = w.space.graph.nearest_node(pos);
        let path = w.space.graph.shortest(from, po.node, &|e, _| w.space.edges[e as usize].len).map(|p| p.length).unwrap_or(0.0);
        let allowed_s = 3.0 * (path + 30.0) / b.base_speed.max(0.3) + 60.0;
        let waited_s = (end - te) as f64 * dt;
        if waited_s > allowed_s && b.blocked_ticks == 0 {
            stuck.push(format!("몸 {id}: {waited_s:.0}초째(허용 {allowed_s:.0}초) ({:.1},{:.1})에서 출구 {}까지 {:.1}m", b.pos.x, b.pos.z, po.id, b.pos.dist(po.pos)));
        }
    }
    (n_gone, stuck)
}

/// 같은 계기는 최소 간격 안에 다시 결정을 일으키지 않는다: 단위 시험(알아차림 계기는 notice.trigger_min_interval_s 안에
/// 다시 생기지 않음)과 실행 기록(알아차림 20초, 마감 30초, 피로 60초, 경로의 관찰 값 변화 30초, 시선은 최소 유지 시간).
#[test]
fn e222_7_min_interval() {
    let m = mid_quick();
    let w = m.w();
    let (b0, prof, sh) = walker_with_ctx(m);
    let t0 = m.state.tick;
    let p = salient_place(m, prof, &b0);
    let gap = (w.p.notice.trigger_min_interval_s / w.dt()).round() as u64;
    let mut b = b0.clone();
    notice(&mut b, p, t0);
    assert_eq!(decide_at(m, &sh, prof, &mut b, t0).decisions.len(), 1);
    // 결정 뒤 몸은 새 목표로 가고 있다. 최소 간격 안의 새 알아차림은 다시 결정을 일으키지 않는다.
    let q = (0..w.space.places.len() as u32).find(|&x| x != p && b.aware_of(x).is_none() && w.space.places[x as usize].go_to && m.state.env.open[x as usize]).unwrap();
    for k in 1..gap {
        let mut bb = b.clone();
        bb.new_notices = vec![q];
        bb.aware.push(Aware { place: q, first_tick: t0 + k, last_seen_tick: Some(t0 + k), seen_open: Some(true), seen_queue: Some(0), known_before: false });
        bb.aware.sort_by_key(|a| a.place);
        let fx = decide_at(m, &sh, prof, &mut bb, t0 + k);
        assert!(fx.decisions.iter().all(|(d, _)| d.trigger != Trigger::Notice.name()), "알아차림 계기가 {}틱 만에 다시 결정을 일으킴", k);
    }
    // 실행 기록
    let min_s: [(&str, &str, f64); 4] = [("NEXT", "알아차림", w.p.notice.trigger_min_interval_s), ("NEXT", "마감", 30.0), ("REST", "피로", 60.0), ("ROUTE", "관찰 값 변화", 30.0)];
    for o in [observed_quick(), observed_variant()] {
        let mut last: BTreeMap<(i64, &str, &str), u64> = BTreeMap::new();
        let mut n = 0;
        for d in &o.sink.decisions {
            let lim = if d.context == "LOOK" {
                let b = &o.state.bodies[d.body as usize];
                (w.bundle.profiles[b.profile.unwrap() as usize].perc.min_dwell_s / w.dt()).round().max(1.0) * w.dt()
            } else if let Some(x) = min_s.iter().find(|x| x.0 == d.context && x.1 == d.trigger) {
                x.2
            } else {
                continue;
            };
            let key = (d.body, d.context.as_str(), d.trigger.as_str());
            if let Some(&prev) = last.get(&key) {
                let gap_s = (d.tick - prev) as f64 * w.dt();
                assert!(gap_s >= lim - 1e-9, "몸 {} {} '{}': {gap_s}초 만에 다시 결정(최소 {lim}초)", d.body, d.context, d.trigger);
                n += 1;
            }
            last.insert(key, d.tick);
        }
        assert!(n > 100, "간격을 본 결정 {n}");
    }
}
