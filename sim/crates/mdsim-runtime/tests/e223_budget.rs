//! E223-3·4 (02d S-E43~S-E45): 이웃 간격 예산, 이미 겹친 쌍, 도착·퇴장 자리.
mod common;

use common::{mid_quick, prep, prep_empty, quiet_walker, scenario, with_threads, with_window, Corridor, Mid};
use mdsim_core::bundle::Mode;
use mdsim_core::geom::V2;
use mdsim_core::rng::KeyRng;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::env::env_at;
use mdsim_runtime::locomotion::{limit_steps, MoveIn, MoveOut};
use mdsim_runtime::record::MemSink;
use mdsim_runtime::space::PlaceKind;
use mdsim_runtime::state::{Activity, Body, Target};
use mdsim_runtime::step::{Sim, JAM_S, JAM_SPREAD_S, REJAM_S, RELEASE_S};
use mdsim_runtime::visit::{Spawn, SpawnKind};

fn walker(m: &Mid, id: u32, pos: V2, radius: f64) -> Body {
    let mut b = quiet_walker(m);
    b.id = id;
    b.entity = format!("budget#{id}");
    b.pos = pos;
    b.radius = radius;
    b.activity = Activity::Walk;
    b.route = None;
    b.cur_edge = None;
    b
}

/// 몸 `b`가 `d`만큼 가려는 희망 이동(이동 단계 ①이 내는 값을 흉내 낸다).
fn intent(b: &Body, d: V2, dt: f64) -> MoveOut {
    MoveOut { pos: b.pos + d, vel: d * (1.0 / dt), heading: d.angle(), moved: d.len(), route_idx: 0, side_assign: None, arrived: false, cur_edge: None, grade_pct: 0.0, on_steps: false, blocked: false, v0: 1.3, final_pos: b.pos + d * 100.0 }
}

/// 몸 목록과 희망 이동(None = 움직이지 않음)에 이웃 간격 예산을 적용한 결과.
fn budgeted(m: &Mid, c: &Corridor, bodies: &[Body], want: &[Option<V2>]) -> Vec<Option<MoveOut>> {
    let w = m.w();
    let env = env_at(w.scn, &c.space, w.p.perception.night_light, w.t_s(0));
    let coll = Collective::positions(bodies);
    let mi = MoveIn { space: &c.space, env: &env, coll: &coll, p: &w.p.loco, interactions: true, dt: w.dt() };
    let mut outs: Vec<Option<MoveOut>> = bodies.iter().zip(want).map(|(b, d)| d.map(|d| intent(b, d, w.dt()))).collect();
    limit_steps(&mi, bodies, &mut outs);
    outs
}

/// 거절 표집으로 겹치지 않게 놓은 몸 `n`명(8m × 8m, 반지름 0.25와 0.4를 섞음)과 임의의 희망 이동(길이 0.9m 이하, 열에 하나 꼴로 안 움직임).
fn random_config(m: &Mid, trial: u64, n: usize) -> (Vec<Body>, Vec<Option<V2>>) {
    let rng = KeyRng::new(23);
    let mut bodies: Vec<Body> = Vec::new();
    let mut k = 0u64;
    while bodies.len() < n && k < 50_000 {
        let pos = V2::new(6.0 + 8.0 * rng.u("E223", "pos", trial, k, 0), -4.0 + 8.0 * rng.u("E223", "pos", trial, k, 1));
        let radius = if rng.u("E223", "pos", trial, k, 2) < 0.2 { 0.4 } else { 0.25 };
        k += 1;
        if bodies.iter().all(|o| o.pos.dist(pos) - o.radius - radius >= 0.0) {
            let id = bodies.len() as u32;
            bodies.push(walker(m, id, pos, radius));
        }
    }
    assert_eq!(bodies.len(), n, "겹치지 않게 {n}명을 놓지 못함");
    let want = (0..n)
        .map(|i| {
            if rng.u("E223", "move", trial, i as u64, 2) < 0.1 {
                return None;
            }
            let a = std::f64::consts::TAU * rng.u("E223", "move", trial, i as u64, 0);
            Some(V2::from_angle(a) * (0.9 * rng.u("E223", "move", trial, i as u64, 1)))
        })
        .collect();
    (bodies, want)
}

fn near(a: V2, b: V2) -> bool {
    (a - b).len() < 1e-9
}

/// 틱 시작에 겹침이 없으면 방향과 무관하게 틱 끝에도 겹침이 없다(02d I-223-r 5의 보장).
/// 깎인 이동은 희망 이동보다 길지 않고 거꾸로 가지 않으며, 이웃을 따라 미끄러지는 이동이 남는다.
/// 예산이 없으면 같은 배치에서 쌍이 겹친다.
#[test]
fn e223_3_budget_keeps_gap() {
    let m = mid_quick();
    let c = Corridor::new(40.0, 12.0);
    let dt = m.w().dt();
    let (mut pairs, mut cut, mut slid, mut would) = (0usize, 0usize, 0usize, 0usize);
    for trial in 0..40u64 {
        let (bodies, want) = random_config(m, trial, 60);
        let outs = budgeted(m, &c, &bodies, &want);
        let end = |k: usize| outs[k].as_ref().map_or(bodies[k].pos, |o| o.pos);
        let raw = |k: usize| bodies[k].pos + want[k].unwrap_or(V2::ZERO);
        for i in 0..bodies.len() {
            for j in i + 1..bodies.len() {
                let r = bodies[i].radius + bodies[j].radius;
                if raw(i).dist(raw(j)) - r < -1e-9 {
                    would += 1;
                }
                let gap = end(i).dist(end(j)) - r;
                assert!(gap >= -1e-9, "시도 {trial}: 몸 {i}·{j}의 틈이 {gap:.4}m");
                pairs += 1;
            }
        }
        for i in 0..bodies.len() {
            let (Some(o), Some(d)) = (outs[i].as_ref(), want[i]) else { continue };
            let got = o.pos - bodies[i].pos;
            assert!(got.len() <= d.len() + 1e-9, "시도 {trial}: 몸 {i}의 이동 {:.4}이 희망 {:.4}보다 김", got.len(), d.len());
            assert!(got.dot(d) >= -1e-9, "시도 {trial}: 몸 {i}가 희망과 반대로 감");
            assert!((o.moved - got.len()).abs() < 1e-9 && (o.vel * dt - got).len() < 1e-9, "이동 거리·속도가 깎은 이동과 같다");
            if (got - d).len() > 1e-9 {
                cut += 1;
                if got.len() > 1e-6 && got.cross(d).abs() > 1e-6 {
                    slid += 1;
                }
            }
        }
    }
    assert!(pairs >= 70_000, "본 쌍 {pairs}");
    assert!(would > 100, "예산이 없으면 겹치는 쌍 {would}: 시험이 예산을 시험해야 한다");
    assert!(cut > 100, "깎인 이동 {cut}");
    assert!(slid > 50, "이웃을 따라 미끄러진 이동 {slid}: 접근 성분만 덜어 내야 한다");
}

/// 이미 겹친 쌍은 서로를 향한 접근만 막고, 멀어지거나 옆으로 비키는 이동은 그대로 둔다. 중심이 같은 쌍은 방향이 없어 뺀다.
/// 틈이 있는 쌍은 접근 성분이 틈에 이르도록 덜어 내고, 맞닿은 이웃을 비스듬히 향하면 접근만 빼고 미끄러진다.
/// 한동안 막힌 몸은 막은 이웃을 반시계로 돌아 지나가는 옆걸음을 더해 마주 선 두 몸이 서로 반대쪽으로 비킨다.
#[test]
fn e223_3_overlapped_pair_no_approach() {
    let m = mid_quick();
    let c = Corridor::new(40.0, 12.0);
    let a0 = V2::new(10.0, 0.0);
    // 몸 0은 a0, 몸 1은 a0 + (dx, 0). 두 몸이 각각 가려는 이동과 막힌 틱 수 → 깎인 이동.
    let run = |dx: f64, da: Option<V2>, db: Option<V2>, blocked: u32| {
        let mut bodies = vec![walker(m, 0, a0, 0.25), walker(m, 1, a0 + V2::new(dx, 0.0), 0.25)];
        for b in bodies.iter_mut() {
            b.blocked_ticks = blocked;
        }
        let outs = budgeted(m, &c, &bodies, &[da, db]);
        let got = |k: usize| outs[k].as_ref().map_or(V2::ZERO, |o| o.pos - bodies[k].pos);
        (got(0), got(1))
    };
    let right = V2::new(0.4, 0.0);
    let left = V2::new(-0.4, 0.0);
    let up = V2::new(0.0, 0.4);
    // 겹친 쌍(틈 −0.2)
    let (da, _) = run(0.3, Some(right), None, 0);
    assert!(da.len() < 1e-12, "겹친 이웃을 향한 접근은 막힌다: {da:?}");
    let (da, _) = run(0.3, Some(left), None, 0);
    assert!(near(da, left), "멀어지는 이동은 그대로: {da:?}");
    let (da, _) = run(0.3, Some(up), None, 0);
    assert!(near(da, up), "옆으로 비키는 이동은 그대로: {da:?}");
    let (da, db) = run(0.3, Some(right), Some(left), 0);
    assert!(da.len() < 1e-12 && db.len() < 1e-12, "마주 보고 움직여도 처음에는 둘 다 멈춘다: {da:?} {db:?}");
    // 중심이 같은 쌍은 방향이 없어 제외된다
    let (da, db) = run(0.0, Some(right), Some(left), 0);
    assert!(near(da, right) && near(db, left), "중심이 같은 쌍은 제외: {da:?} {db:?}");
    // 닿은 쌍(틈 0): 정면 접근은 막고, 비스듬한 접근은 접근 성분만 빼고 미끄러진다
    let (da, _) = run(0.5, Some(right), None, 0);
    assert!(da.len() < 1e-12, "닿은 이웃을 향한 접근은 막힌다: {da:?}");
    let (da, _) = run(0.5, Some(V2::new(0.4, 0.4)), None, 0);
    assert!(near(da, V2::new(0.0, 0.4)), "닿은 이웃을 비스듬히 향하면 접근 성분만 빠진다: {da:?}");
    // 틈 0.1: 접근 성분이 0.1에 이르도록 덜어 낸다. 서로 다가오면 틈을 반씩 쓴다.
    let (da, _) = run(0.6, Some(right), None, 0);
    assert!(near(da, V2::new(0.1, 0.0)), "틈 0.1이면 0.1만 간다: {da:?}");
    let (da, db) = run(0.6, Some(right), Some(left), 0);
    assert!(near(da, V2::new(0.05, 0.0)) && near(db, V2::new(-0.05, 0.0)), "마주 오면 틈을 반씩 쓴다: {da:?} {db:?}");
    // 한동안(4틱) 막혀 있으면 옆걸음이 더해져 마주 선 두 몸이 서로 반대쪽으로 비킨다
    let (da, db) = run(0.5, Some(right), Some(left), 8);
    assert!(near(da, V2::new(0.0, 0.2)) && near(db, V2::new(0.0, -0.2)), "마주 선 두 몸이 반대쪽으로 비킨다: {da:?} {db:?}");
}

/// 처리 순서와 몸 번호, 스레드 수와 무관하게 같은 결과를 낸다(E223-4).
#[test]
fn e223_4_budget_order_invariance() {
    let m = mid_quick();
    let c = Corridor::new(40.0, 12.0);
    let bits = |o: &Option<MoveOut>| o.as_ref().map(|o| (o.pos.x.to_bits(), o.pos.z.to_bits(), o.vel.x.to_bits(), o.vel.z.to_bits(), o.moved.to_bits(), o.arrived, o.blocked));
    for trial in 0..10u64 {
        let (bodies, want) = random_config(m, trial, 60);
        let n = bodies.len();
        let fwd = budgeted(m, &c, &bodies, &want);
        // 몸 번호를 거꾸로 매긴 세계
        let mut rb: Vec<Body> = bodies.iter().rev().cloned().collect();
        for (k, b) in rb.iter_mut().enumerate() {
            b.id = k as u32;
        }
        let rw: Vec<Option<V2>> = want.iter().rev().copied().collect();
        let rev = budgeted(m, &c, &rb, &rw);
        for i in 0..n {
            assert_eq!(bits(&fwd[i]), bits(&rev[n - 1 - i]), "시도 {trial}: 몸 {i}: 몸 번호를 바꾸니 결과가 다름");
        }
        // 스레드 하나로 돌려도 같다
        let single = with_threads(1, || budgeted(m, &c, &bodies, &want));
        for i in 0..n {
            assert_eq!(bits(&fwd[i]), bits(&single[i]), "시도 {trial}: 몸 {i}: 스레드 수에 따라 결과가 다름");
        }
    }
}

/// 같은 출입구에 같은 틱에 연달아 도착하는 몸은 서로 겹치지 않는 자리에서 걷기 시작한다(S-E45).
/// 한 틱을 지난 뒤에도 틈이 0 이상이어야 하는데, 겹친 채 낳았다면 예산이 틈을 못 만든다.
#[test]
fn e223_3_spawn_spot_clear() {
    let pr = prep_empty(scenario("quick"));
    let w = pr.w;
    let spawns: Vec<Spawn> = (0..6u16).map(|k| Spawn { tick: 0, kind: SpawnKind::Domestic { profile: 0, replicate: k }, portal: 0, mode: Mode::Walk, deadline_tick: w.scn.n_ticks(), entity: format!("spot#{k}") }).collect();
    let spawns: &'static [Spawn] = Box::leak(spawns.into_boxed_slice());
    let portal = w.space.portals[0].pos;
    // 시험 조건: 종래의 흩기(첫 후보 점)대로 놓으면 이 여섯 명 가운데 겹치는 쌍이 있다.
    let old: Vec<V2> = spawns
        .iter()
        .map(|s| {
            let draw = |k: u32| w.rng.u("SPAWN", &s.entity, 0, 1, k);
            portal + V2::from_angle(draw(4) * std::f64::consts::TAU) * (1.5 * draw(5).sqrt())
        })
        .collect();
    let old_overlaps = (0..old.len()).flat_map(|i| (i + 1..old.len()).map(move |j| (i, j))).filter(|&(i, j)| old[i].dist(old[j]) < 2.0 * w.p.loco.body_radius_m).count();
    assert!(old_overlaps >= 1, "종래의 흩기로도 겹치지 않는 배치라 시험이 약하다");
    let mut sim = Sim::new(w, spawns);
    let mut sink = MemSink::default();
    sim.run_until(1, &mut sink).expect("run");
    let bodies = &sim.state.bodies;
    assert_eq!(bodies.len(), 6, "도착한 몸");
    for (i, a) in bodies.iter().enumerate() {
        assert!(a.pos.dist(portal) < 1.5 + 1.0, "몸 {i}가 출입구에서 {:.2}m", a.pos.dist(portal));
        for (j, b) in bodies.iter().enumerate().skip(i + 1) {
            let gap = a.pos.dist(b.pos) - a.radius - b.radius;
            assert!(gap >= -1e-9, "몸 {i}·{j}의 틈 {gap:.3}m");
        }
    }
}

/// 같은 가게에서 같은 틱에 나오는 몸은 앵커 한 점에 겹치지 않고 서로 겹치지 않는 자리에서 걷기 시작한다(S-E45).
/// 앵커 둘레 반경 1m에는 몸 3~4명이 겹치지 않고 들어가므로 세 명이 한꺼번에 나오는 경우를 본다.
#[test]
fn e223_3_exit_spot_clear() {
    const N_EXIT: usize = 3;
    let m = mid_quick();
    let pr = prep_empty(with_window(scenario("quick"), "14:00", "14:40"));
    let w = pr.w;
    let mut sim = Sim::new(w, &[]);
    let mut sink = MemSink::default();
    sim.run_until(1, &mut sink).expect("run");
    let p = w.space.places.iter().position(|pl| pl.kind == PlaceKind::Shop && pl.capacity >= 6 && w.space.walkable_static(pl.anchor)).expect("가게") as u32;
    let pl = &w.space.places[p as usize];
    let tick = sim.state.tick;
    for k in 0..N_EXIT {
        let id = sim.state.bodies.len() as u32;
        let mut b = quiet_walker(m);
        b.id = id;
        b.entity = format!("exit#{k}");
        b.alive = true;
        b.arrival_tick = 0;
        b.deadline_tick = w.scn.n_ticks() + 100_000;
        b.activity = Activity::Inside;
        b.act_place = Some(p);
        b.act_until = tick; // 이번 틱에 활동이 끝난다
        b.target = Target::None;
        b.route = None;
        b.pending = None;
        b.arrived = false;
        b.pos = pl.anchor;
        b.cur_edge = Some(pl.edge);
        b.stats = Default::default();
        sim.state.bodies.push(b);
        sim.state.queues[p as usize].inside.push(id);
    }
    sim.run_until(tick + 1, &mut sink).expect("step");
    let exits = sink.events.iter().filter(|e| e.kind == "exit" && e.place == Some(p)).count();
    assert_eq!(exits, N_EXIT, "나온 몸");
    let bodies = &sim.state.bodies;
    let mut moved_off = 0;
    for (i, a) in bodies.iter().enumerate() {
        assert!(a.activity.on_street() && a.alive, "몸 {i}는 거리에 나와 있다");
        assert!(a.pos.dist(pl.anchor) < 1.0 + 1.0, "몸 {i}가 앵커에서 {:.2}m", a.pos.dist(pl.anchor));
        if a.pos.dist(pl.anchor) > 0.05 {
            moved_off += 1;
        }
        for (j, b) in bodies.iter().enumerate().skip(i + 1) {
            let gap = a.pos.dist(b.pos) - a.radius - b.radius;
            assert!(gap >= -1e-9, "몸 {i}·{j}의 틈 {gap:.3}m");
        }
    }
    assert!(moved_off >= N_EXIT - 1, "앵커를 벗어난 몸 {moved_off}");
}

/// 시험용으로 빈 실행에 걷는 몸을 하나 넣는다(목표·경로 없음).
fn add_walker(sim: &mut Sim<'static>, m: &Mid, pos: V2, name: &str) -> u32 {
    let w = sim.w;
    let id = sim.state.bodies.len() as u32;
    let mut b = quiet_walker(m);
    b.id = id;
    b.entity = format!("jam#{name}");
    b.alive = true;
    b.arrival_tick = sim.state.tick;
    b.deadline_tick = w.scn.n_ticks() + 100_000;
    b.activity = Activity::Walk;
    b.target = Target::None;
    b.route = None;
    b.pending = None;
    b.arrived = false;
    b.pos = pos;
    b.cur_edge = None;
    b.blocked_ticks = 0;
    b.jam_until = 0;
    b.stats = Default::default();
    sim.state.bodies.push(b);
    id
}

/// 교착 풀기(S-E56): 맞닿은 이웃이 있는 채로 오래 막힌 몸만 풀어 사건을 남기고, 혼자 막힌 몸은 풀지 않는다.
/// 풀어도 계속 막혀 있으면 `REJAM_S`(5분)마다만 다시 푼다.
#[test]
fn e223_3_jam_release_touching_only() {
    let m = mid_quick();
    let pr = prep_empty(with_window(scenario("quick"), "14:00", "14:40"));
    let w = pr.w;
    let dt = w.dt();
    let first = |id: u32| (JAM_S / dt).round() as u32 + id % (JAM_SPREAD_S / dt).round() as u32;
    let release_ticks = (RELEASE_S / dt).round() as u64;
    let rejam = (REJAM_S / dt).round() as u32;
    let pts = common::sample_points(w.space);
    let p1 = pts[0];
    let p2 = *pts.iter().find(|p| p.dist(p1) > 30.0).expect("먼 자리");
    let mut sim = Sim::new(w, &[]);
    let mut sink = MemSink::default();
    sim.run_until(1, &mut sink).expect("run");
    let a = add_walker(&mut sim, m, p1, "a");
    let b = add_walker(&mut sim, m, p1 + V2::new(0.6, 0.0), "b"); // 틈 0.1m
    let c = add_walker(&mut sim, m, p2, "c"); // 혼자
    sim.state.bodies[a as usize].blocked_ticks = first(a);
    sim.state.bodies[c as usize].blocked_ticks = first(c);
    let tick = sim.state.tick;
    sim.run_until(tick + 1, &mut sink).expect("step");
    let releases = |sink: &MemSink| sink.events.iter().filter(|e| e.kind == "jam_release").map(|e| e.body).collect::<Vec<_>>();
    assert_eq!(releases(&sink), vec![a as i64], "맞닿은 이웃이 있는 몸만 푼다(혼자 막힌 몸 {c}는 아님)");
    assert_eq!(sim.state.bodies[a as usize].jam_until, tick + release_ticks, "풀기는 8초");
    assert_eq!(sim.state.bodies[c as usize].jam_until, 0);
    let ev = sink.events.iter().find(|e| e.kind == "jam_release").unwrap();
    assert!((ev.value - first(a) as f64 * dt).abs() < 1e-9, "value는 막힌 초: {}", ev.value);
    // 풀기가 끝난 뒤 계속 막혀 있어도 곧바로 다시 풀지 않고 5분 뒤에 푼다
    sim.run_until(tick + 1 + release_ticks, &mut sink).expect("window");
    let t = sim.state.tick;
    for (blocked, expect_new) in [(first(a) + 7, false), (first(a) + rejam, true)] {
        let pos = sim.state.bodies[a as usize].pos;
        sim.state.bodies[b as usize].pos = pos + V2::new(0.6, 0.0);
        sim.state.bodies[a as usize].blocked_ticks = blocked;
        let before = releases(&sink).len();
        sim.run_until(sim.state.tick + 1, &mut sink).expect("step");
        assert_eq!(releases(&sink).len() - before, usize::from(expect_new), "막힌 틱 {blocked}에서 새 사건 {expect_new}");
        let _ = t;
    }
}

/// 이웃을 무시하는 몸은 이웃 색인에서 빠져 남에게 보이지 않는다(S-E56).
#[test]
fn e223_3_jam_release_hides_ghost() {
    let m = mid_quick();
    let mut bodies: Vec<Body> = (0..3).map(|i| walker(m, i, V2::new(10.0 + 0.6 * i as f64, 0.0), 0.25)).collect();
    bodies[1].jam_until = 10;
    let all = Collective::positions(&bodies);
    assert!(all.on_street.iter().all(|&x| x));
    let seen = Collective::positions_where(&bodies, |b| b.jam_until <= 5);
    assert_eq!(seen.on_street, vec![true, false, true]);
    let mut near = Vec::new();
    seen.neighbors(V2::new(10.6, 0.0), 1.0, &mut near);
    assert_eq!(near, vec![0, 2], "이웃을 무시하는 몸 1은 찾아지지 않는다");
    assert_eq!(seen.pos[1], bodies[1].pos, "위치 배열은 그대로 둔다(몸 번호로 찾는다)");
}

/// 기본 3시간 실행이 교착 없이 흐른다: 오래 거리에 머무는 인원이 쌓이지 않고 걷는 평균 속도가 유지된다.
/// (맞닿은 쌍이 서로를 막는 교착은 한 시간 실행에서는 안 보이고 2시간 뒤 교차로에서 퍼졌다.)
#[test]
#[ignore = "예산 시험: 기본 실행(3시간, 목표 동시 2,000명)을 끝까지 돈다"]
fn e223_3_no_gridlock_default() {
    let pr = prep(scenario("base"));
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    for t in (3600..=21600u64).step_by(3600) {
        sim.run_until(t, &mut sink).expect("run");
        let on: Vec<_> = sim.state.bodies.iter().filter(|b| b.alive && b.activity.on_street()).collect();
        let walkers: Vec<_> = on.iter().filter(|b| b.activity == Activity::Walk).collect();
        let stuck = walkers.iter().filter(|b| b.vel.len() < 0.05).count();
        let mean_v = walkers.iter().map(|b| b.vel.len()).sum::<f64>() / walkers.len().max(1) as f64;
        println!("{}분: 거리 위 {}명, 걷는 {}명 중 멈춘 {stuck}, 평균 속력 {mean_v:.2}", t / 120, on.len(), walkers.len());
        assert!(on.len() < 900, "{}분: 거리 위 {}명(정체가 쌓임)", t / 120, on.len());
        assert!(mean_v > 0.5, "{}분: 걷는 평균 속력 {mean_v:.2}m/s", t / 120);
        assert!(stuck as f64 <= 0.25 * walkers.len() as f64, "{}분: 걷는 {}명 중 {stuck}명이 멈춤", t / 120, walkers.len());
    }
    let releases = sink.events.iter().filter(|e| e.kind == "jam_release").count();
    assert!(releases < 1500, "교착 풀기 사건 {releases}");
}
