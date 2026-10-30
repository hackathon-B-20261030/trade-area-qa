//! E223: 이동과 상호 회피(C-223)의 완료 기준 ①~⑦.
mod common;

use common::{inject_queuer, lone, mid_quick, observe, observed_quick, prep, prep_empty, prof_of, route_plan, scenario, with_window, Corridor, Mid, Observed};
use mdsim_core::geom::point_at;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::env::env_at;
use mdsim_runtime::locomotion::{move_body, Drive, MoveIn, MoveOut};
use mdsim_runtime::params::{tobler, weidmann_speed};
use mdsim_runtime::perception::effective_fov;
use mdsim_runtime::record::MemSink;
use mdsim_runtime::route::Path;
use mdsim_runtime::space::PlaceKind;
use mdsim_runtime::state::{Activity, Body, GazeTarget};
use mdsim_runtime::step::Sim;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// 행사일 변형(15:30~16:30)의 앞 35분: 16:00에 노점이 열려 그 칸이 걷기 불가가 되는 순간을 지난다.
fn observed_variant() -> &'static Observed {
    static O: OnceLock<Observed> = OnceLock::new();
    O.get_or_init(|| observe(prep(scenario("variant")), 4200))
}

/// 장소 안이 아닌 몸의 위치는 늘 보행 공간 안이다: 정적 보행 다각형(건물 제외 걷기 칸) 안이고, 그 틱의 동적 층
/// (운영 중인 노점 칸)까지 본 걷기 가능 칸 안이다.
#[test]
fn e223_1_inside_walkable() {
    for o in [observed_quick(), observed_variant()] {
        let w = o.prep.w;
        let mut n = 0;
        for t in 0..o.snaps.len() - 1 {
            let env = env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(t as u64));
            for s in o.snaps[t + 1].iter().filter(|s| s.alive && s.activity.on_street()) {
                assert!(w.space.walkable_static(s.pos), "몸 {} 틱 {t}: 보행 다각형 밖 ({:.2},{:.2}) {:?}", s.id, s.pos.x, s.pos.z, s.activity);
                assert!(w.space.walkable(s.pos, &env), "몸 {} 틱 {t}: 그 틱에 걷기 불가 칸 ({:.2},{:.2}) {:?}", s.id, s.pos.x, s.pos.z, s.activity);
                n += 1;
            }
        }
        // 표본 크기: 모퉁이 제자리걸음이 없어진 뒤(S-E52) 빠른 실행의 거리 위 몸-틱이 약 7만이다
        assert!(n > 50_000, "본 몸-틱 {n}");
    }
}

/// 한 틱의 이동 거리는 최대 속도 × Δt 이하다. 최대 속도 = 기준 속도 × Tobler 상한(loco.tobler_fmax)
/// (PACE·둘러보기·피로·계단은 희망 속도를 줄이기만 한다). 장소에 들어가는(장소 앵커로 옮겨지는) 틱은 걷기가 아니라 뺀다.
#[test]
fn e223_2_max_step() {
    for o in [observed_quick(), observed_variant()] {
        let w = o.prep.w;
        let mut worst = 0.0f64;
        for t in 0..o.snaps.len() - 1 {
            for s in o.snaps[t + 1].iter().filter(|s| s.alive && s.activity.on_street()) {
                let Some(prev) = o.snaps[t].get(s.id as usize) else { continue };
                if !(prev.alive && prev.activity.on_street()) {
                    continue;
                }
                let vmax = s.base_speed * w.p.loco.tobler_fmax;
                let moved = prev.pos.dist(s.pos);
                assert!(moved <= vmax * w.dt() + 1e-9, "몸 {} 틱 {t}: {moved:.4}m > {:.4}m", s.id, vmax * w.dt());
                worst = worst.max(moved / (vmax * w.dt()));
            }
        }
        assert!(worst > 0.5, "실제로 걸은 틱이 있어야 한다(최대 비율 {worst:.3})");
    }
}

/// 두 몸 사이 거리는 몸 반지름 합에서 허용 오차(0.1m: 몸 지름의 5분의 1, 0.5초 틱에 서로 다가가며 생기는 접촉 오차)를
/// 뺀 값 이상이다. 장소 안·사라진 몸은 빼고, 거리 위의 모든 쌍(걷기·줄·쉬기·관람 포함)을 본다.
#[test]
fn e223_3_min_separation() {
    let tol = 0.1;
    for o in [observed_quick(), observed_variant()] {
        let mut bad = 0usize;
        let mut worst = (f64::INFINITY, String::new());
        for (t, snap) in o.snaps.iter().enumerate() {
            let on: Vec<_> = snap.iter().filter(|s| s.alive && s.activity.on_street()).collect();
            for (i, a) in on.iter().enumerate() {
                for b in &on[i + 1..] {
                    let gap = a.pos.dist(b.pos) - a.radius - b.radius;
                    if gap < -tol {
                        bad += 1;
                        if gap < worst.0 {
                            worst = (gap, format!("틱 {t} 몸 {}({:?})·{}({:?})", a.id, a.activity, b.id, b.activity));
                        }
                    }
                }
            }
        }
        assert_eq!(bad, 0, "겹친 몸-쌍-틱 {bad}, 가장 깊은 겹침 {:.3}m: {}", worst.0, worst.1);
    }
}

/// 이동 결과의 비교 키(위치·속도·방향 비트, 경로 진행, 쪽 정하기, 도착, 간선, 막힘).
type MoveKey = ([u64; 5], usize, Option<(usize, i8)>, bool, Option<u32>, bool);

fn key(o: &MoveOut) -> MoveKey {
    ([o.pos.x.to_bits(), o.pos.z.to_bits(), o.vel.x.to_bits(), o.vel.z.to_bits(), o.heading.to_bits()], o.route_idx, o.side_assign, o.arrived, o.cur_edge, o.blocked)
}

fn drive_of(m: &Mid, coll: &Collective, b: &Body) -> Drive {
    let w = m.w();
    let fov = effective_fov(prof_of(w, b), b, &m.state.env, coll.local_density[b.id as usize], &w.p, m.state.tick, w.dt());
    Drive { browse: b.activity == Activity::Browse, speed_mult: b.route.as_ref().map(|r| r.speed_factor).unwrap_or(1.0), fatigue: b.fatigue, peripheral_deg: fov.peri_angle_deg, peripheral_range: fov.peri_range, gaze_off_path: matches!(b.gaze_target, GazeTarget::Place(_)), goal: None }
}

/// 처리 순서와 무관하게 같은 결과를 낸다: 같은 t 위치 사본에서 모든 몸의 이동을 정순·역순·병렬로, 그리고 몸 번호를
/// 거꾸로 매긴 세계에서 다시 계산해 비교한다(실행 단위의 병렬 분할 무관은 e225_2가 본다).
#[test]
fn e223_4_order_invariance() {
    let m = mid_quick();
    let w = m.w();
    let st = &m.state;
    let coll = Collective::build(&st.bodies, w.space, &st.env, w.p.crowd.local_radius_m);
    let mi = MoveIn { space: w.space, env: &st.env, coll: &coll, p: &w.p.loco, interactions: true, dt: w.dt() };
    let ids: Vec<usize> = st.bodies.iter().filter(|b| b.alive && !b.is_background() && b.activity.moving()).map(|b| b.id as usize).collect();
    assert!(ids.len() >= 5);
    let one = |i: usize| key(&move_body(&mi, &st.bodies[i], &drive_of(m, &coll, &st.bodies[i])));
    let fwd: Vec<_> = ids.iter().map(|&i| one(i)).collect();
    let rev: Vec<_> = ids.iter().rev().map(|&i| one(i)).collect::<Vec<_>>().into_iter().rev().collect();
    let par: Vec<_> = ids.par_iter().map(|&i| one(i)).collect();
    assert_eq!(fwd, rev);
    assert_eq!(fwd, par);
    let n = st.bodies.len();
    let mut relabeled: Vec<Body> = st.bodies.iter().rev().cloned().collect();
    for (k, b) in relabeled.iter_mut().enumerate() {
        b.id = k as u32;
    }
    let coll2 = Collective::build(&relabeled, w.space, &st.env, w.p.crowd.local_radius_m);
    let mi2 = MoveIn { coll: &coll2, ..mi };
    for (k, &i) in ids.iter().enumerate() {
        let b = &relabeled[n - 1 - i];
        assert_eq!(key(&move_body(&mi2, b, &drive_of(m, &coll2, b))), fwd[k], "몸 {i}: 몸 번호를 바꾸니 이동이 다름");
    }
}

/// 같은 몸은 오르막에서 평지보다 느리다(경사 5.3%·10% 간선을 오르는 쪽 vs 평지 간선, 이웃 없음).
#[test]
fn e223_5_uphill_slower() {
    let m = mid_quick();
    let w = m.w();
    let speed_on = |e: usize, forward: bool| -> f64 {
        let es = &w.space.edges[e];
        let s = if forward { 10.0 } else { es.len - 10.0 };
        let (pos, dir) = point_at(&es.poly, &es.cum, s);
        let heading = if forward { dir.angle() } else { (-dir).angle() };
        let mut b = lone(m, pos, heading, heading);
        let (na, nb) = if forward { (es.a, es.b) } else { (es.b, es.a) };
        let path = Path { nodes: vec![na, nb], edges: vec![(e as u32, forward)], length: es.len };
        b.route = Some(route_plan(&path, w.space.graph.node_pos[nb as usize]));
        b.cur_edge = Some(e as u32);
        let coll = Collective::default();
        let mi = MoveIn { space: w.space, env: &m.state.env, coll: &coll, p: &w.p.loco, interactions: false, dt: w.dt() };
        let d = Drive { browse: false, speed_mult: 1.0, fatigue: 0.0, peripheral_deg: 150.0, peripheral_range: 10.0, gaze_off_path: false, goal: None };
        let o = move_body(&mi, &b, &d);
        assert!((o.grade_pct - es.grade(forward)).abs() < 1e-12, "이동이 그 간선 경사를 쓴다");
        o.moved / w.dt()
    };
    let flat = w.space.edges.iter().position(|e| !e.sidewalks && !e.is_steps && e.grade_ab == 0.0 && e.len > 40.0).unwrap();
    let mut hills: Vec<(usize, f64)> = w.space.edges.iter().enumerate().filter(|(_, e)| !e.sidewalks && !e.is_steps && e.grade_ab.abs() > 3.0 && e.len > 40.0).map(|(i, e)| (i, e.grade_ab)).collect();
    hills.sort_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
    assert!(hills.len() >= 2, "언덕 간선 {hills:?}");
    let v_flat = speed_on(flat, true);
    let mut prev = v_flat;
    for &(e, g) in &hills {
        let v_up = speed_on(e, g > 0.0);
        assert!(v_up < v_flat, "경사 {:.1}% 오르막 {v_up:.3} ≥ 평지 {v_flat:.3}", g.abs());
        assert!(v_up <= prev + 1e-12, "더 가파르면 더 느리다");
        // 자유 보행이면 비율이 Tobler 보정 그대로다
        let ratio = tobler(&w.p.loco, g.abs() / 100.0) / tobler(&w.p.loco, 0.0);
        assert!((v_up / v_flat - ratio).abs() < 1e-9, "속도 비 {:.4} vs Tobler {ratio:.4}", v_up / v_flat);
        prev = v_up;
    }
}

/// 일자 통로(길이 40m, 폭 4m, 주기 경계, 모두 같은 방향)의 밀도-속도 관계가 Weidmann(1993) 기준선의 허용 범위 안이다.
/// 허용 범위: 기준선 ±0.4m/s. Weidmann 곡선은 여러 관측을 맞춘 평균선이고 같은 밀도에서 관측 속도는 대략 ±0.3~0.4m/s
/// 퍼진다(02d·01b에 수치 규정이 없어 이 폭을 가정한다). 몸은 겹치지 않는 육각 격자에서 출발하고 기준 속도는 1.34m/s다.
#[test]
fn e223_6_fundamental_diagram() {
    let rows = fundamental_diagram();
    let bad: Vec<_> = rows.iter().filter(|(rho, v, _)| (v - weidmann_speed(*rho)).abs() > 0.4).collect();
    let show: Vec<_> = rows.iter().map(|(r, v, d)| (*r, (v * 1000.0).round() / 1000.0, (weidmann_speed(*r) * 1000.0).round() / 1000.0, (d * 1000.0).round() / 1000.0)).collect();
    assert!(bad.is_empty(), "밀도별 (ρ, 속도, Weidmann, 가장 깊은 겹침): {show:?}");
    for pair in rows.windows(2) {
        assert!(pair[1].1 <= pair[0].1 + 0.05, "밀도가 오르면 속도는 내려간다(흔들림 0.05m/s 허용): {show:?}");
    }
}

/// (밀도, 측정 구간 평균 속도, 실행 중 가장 깊은 겹침)
fn fundamental_diagram() -> Vec<(f64, f64, f64)> {
    let m = mid_quick();
    let c = Corridor::new(40.0, 4.0);
    let v0 = 1.34; // Weidmann 자유 보행 속도로 기준 속도를 맞춘다
    [0.25, 0.5, 1.0, 1.5, 2.0, 2.5]
        .par_iter()
        .map(|&rho| {
            let mut bodies = c.place(m, rho, v0);
            let (v, deepest) = c.run(m.w(), &mut bodies, 60, 120);
            (rho, v, deepest)
        })
        .collect()
}

/// 줄은 음수가 되지 않고 선착순을 지킨다: (1) 한 장소 줄에 한꺼번에 선 6명이 선 순서대로 들어가고,
/// (2) 실행 기록에서 장소마다 줄에 선 순서와 들어간 순서가 같다(줄을 떠난 몸은 뺀다).
#[test]
fn e223_7_queue_fifo() {
    let m = mid_quick();
    let pr = prep_empty(with_window(scenario("quick"), "14:00", "14:40"));
    let w = pr.w;
    let mut sim = Sim::new(w, &[]);
    let mut sink = MemSink::default();
    let p = w.space.places.iter().position(|pl| pl.kind == PlaceKind::Shop && pl.service_rate > 0.0 && pl.service_rate <= 0.6 && pl.capacity >= 10).unwrap() as u32;
    sim.run_until(300, &mut sink).expect("run"); // 처리 기회가 쌓이게
    let ids: Vec<u32> = (0..6).map(|k| inject_queuer(&mut sim, m, p, k)).collect();
    let mut admitted = Vec::new();
    let end = sim.state.tick + 20 * 120; // 0.5명/분이면 6명은 12분 남짓
    while sim.state.tick < end && admitted.len() < ids.len() {
        let t = sim.state.tick;
        sim.run_until(t + 1, &mut sink).expect("step");
        let q = &sim.state.queues[p as usize];
        let waiting_order: Vec<u32> = ids.iter().copied().filter(|i| q.waiting.contains(i)).collect();
        assert_eq!(q.waiting, waiting_order, "줄 순서는 선 순서 그대로");
        for &i in &ids {
            if !admitted.contains(&i) && sim.state.bodies[i as usize].activity != Activity::Queue {
                admitted.push(i);
            }
        }
    }
    assert_eq!(admitted, ids, "선착순 입장");
    // 실행 기록: 장소마다 줄 선 순서 = 들어간 순서
    for o in [observed_quick(), observed_variant()] {
        let mut joins: BTreeMap<u32, Vec<i64>> = BTreeMap::new();
        let mut enters: BTreeMap<u32, Vec<i64>> = BTreeMap::new();
        for e in &o.sink.events {
            let Some(pl) = e.place else { continue };
            match e.kind.as_str() {
                "queue_join" => joins.entry(pl).or_default().push(e.body),
                // 줄을 떠난 몸은 포기(abandoned)다. served는 들어간 몸이라 enter로 센다(S-E55).
                "queue_leave" if e.info == "abandoned" => {
                    let v = joins.entry(pl).or_default();
                    if let Some(k) = v.iter().position(|&b| b == e.body) {
                        v.remove(k);
                    }
                }
                "enter" => enters.entry(pl).or_default().push(e.body),
                _ => {}
            }
        }
        let mut n = 0;
        for (pl, ent) in &enters {
            let j = &joins[pl];
            assert_eq!(&j[..ent.len()], &ent[..], "장소 {pl}: 줄 선 순서와 들어간 순서");
            n += ent.len();
        }
        assert!(n > 20, "입장 {n}");
    }
}
