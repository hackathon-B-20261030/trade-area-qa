//! E210: 집단 상호작용(C-210)의 완료 기준 ①·②·③·④·⑤·⑦ (⑥ 같은 단계 순환 검사는 C-207 등록부, 02c 쪽 시험).
mod common;

use common::{ctx, fx, inject_queuer, mid_quick, observed_quick, prep, prep_empty, prof_of, quiet_walker, scenario, with_window, Corridor};
use mdsim_core::dims::Dim;
use mdsim_core::geom::V2;
use mdsim_core::rng::KeyRng;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::env::{env_at, env_at_hours, EnvState};
use mdsim_runtime::record::MemSink;
use mdsim_runtime::situation::{believed_wait_min, next_situation, path_exposures};
use mdsim_runtime::space::{PlaceKind, Space, STALL_RADIUS_M};
use mdsim_runtime::state::{Activity, Body, ObservedEdge};
use mdsim_runtime::step::Sim;
use std::collections::BTreeMap;

/// 밀도·대기는 틱 t의 위치만 읽는다: 몸 배열의 순서(몸 번호)를 섞어도 같은 몸의 국소 밀도와 간선 밀도가 같고,
/// 실행 중 몸이 틱 t에 본 간선 밀도는 그 틱을 시작할 때의 위치로 만든 집단 상태의 값과 같다.
#[test]
fn e210_1_order_invariance() {
    let m = mid_quick();
    let w = m.w();
    let st = &m.state;
    let r = w.p.crowd.local_radius_m;
    let base = Collective::build(&st.bodies, w.space, &st.env, r);
    assert_eq!(Collective::build(&st.bodies, w.space, &st.env, r), base, "같은 위치 → 같은 집단 상태");
    let n = st.bodies.len();
    let rng = KeyRng::new(210);
    for round in 0..3u64 {
        // 키드 난수로 섞은 순열 π: 새 배열의 k번째 = 원래 몸 π[k]
        let mut perm: Vec<usize> = (0..n).collect();
        perm.sort_by(|&a, &b| rng.u("E210", "perm", round, a as u64, 0).total_cmp(&rng.u("E210", "perm", round, b as u64, 0)));
        let mut bodies: Vec<Body> = perm.iter().map(|&i| st.bodies[i].clone()).collect();
        for (k, b) in bodies.iter_mut().enumerate() {
            b.id = k as u32;
        }
        let c = Collective::build(&bodies, w.space, &st.env, r);
        for (k, &i) in perm.iter().enumerate() {
            assert_eq!(c.local_density[k], base.local_density[i], "몸 {i}의 국소 밀도");
        }
        assert_eq!(c.edge_count, base.edge_count);
        assert_eq!(c.edge_density, base.edge_density);
    }
    // 실행: 틱 t에 본 간선 밀도 = 틱 t 위치의 집단 상태(새로 도착한 몸은 아직 간선에 없다)
    let pr = prep(scenario("quick"));
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    let mut n_obs = 0;
    while sim.state.tick < 1800 {
        let t = sim.state.tick;
        let env = env_at(pr.w.scn, pr.w.space, pr.w.p.perception.night_light, pr.w.t_s(t));
        let dens = Collective::build(&sim.state.bodies, pr.w.space, &env, r).edge_density;
        sim.run_until(t + 1, &mut sink).expect("step");
        for b in &sim.state.bodies {
            for o in b.obs_edges.iter().filter(|o| o.tick == t) {
                assert_eq!(o.density, dens[o.edge as usize], "몸 {} 간선 {} 틱 {t}", b.id, o.edge);
                n_obs += 1;
            }
        }
    }
    assert!(n_obs > 1000, "본 간선 밀도 {n_obs}");
}

/// 유효 폭이 넓어지면 같은 인원에서 그 구간의 밀도는 낮아지고(앞 절반) 속도는 낮아지지 않는다(뒤 절반).
#[test]
fn e210_2_width_density() {
    wider_lower_density();
    let m = mid_quick();
    let speeds: Vec<(f64, f64)> = [3.0, 4.0, 6.0]
        .iter()
        .map(|&width| {
            let c = Corridor::new(40.0, width);
            let mut bodies = c.place(m, 160.0 / (40.0 * width), 1.34);
            assert_eq!(bodies.len(), 160);
            (width, c.run(m.w(), &mut bodies, 60, 120).0)
        })
        .collect();
    for pair in speeds.windows(2) {
        assert!(pair[1].1 >= pair[0].1, "폭 {}m → {}m에서 속도 {:.4} → {:.4}", pair[0].0, pair[1].0, pair[0].1, pair[1].1);
    }
}

/// E210-2의 앞 절반: 폭을 1.5배로 넓히면 같은 몸들(같은 간선 인원)에서 간선 유효 면적은 늘고 밀도는 낮아진다.
#[test]
fn e210_2_wider_lower_density() {
    wider_lower_density();
}

fn wider_lower_density() {
    let m = mid_quick();
    let w = m.w();
    let st = &m.state;
    let r = w.p.crowd.local_radius_m;
    let narrow = Collective::build(&st.bodies, w.space, &st.env, r);
    let mut wide_space: Space = w.space.clone();
    for e in wide_space.edges.iter_mut() {
        e.width *= 1.5;
        e.sw *= 1.5;
    }
    let wide = Collective::build(&st.bodies, &wide_space, &st.env, r);
    let mut n = 0;
    for (e, (a, b)) in w.space.edges.iter().zip(&wide_space.edges).enumerate() {
        assert!(b.walk_area(&st.env, 0.0) > a.walk_area(&st.env, 0.0), "간선 {} 유효 면적", a.id);
        assert_eq!(narrow.edge_count[e], wide.edge_count[e], "같은 인원");
        if narrow.edge_count[e] > 0 {
            assert!(wide.edge_density[e] < narrow.edge_density[e], "간선 {} 밀도 {} → {}", a.id, narrow.edge_density[e], wide.edge_density[e]);
            n += 1;
        }
    }
    assert!(n > 3, "몸이 있는 간선 {n}");
}

/// 노점이 늘면(운영하는 노점이 많아지면) 그 구간의 유효 면적이 줄어 같은 인원의 밀도가 오른다.
#[test]
fn e210_3_stalls_area() {
    let f = fx();
    let scn = scenario("variant"); // 노점 6곳은 16:00부터 운영
    let before = env_at(&scn, &f.space, 0.6, 16.0 * 3600.0 - 0.5);
    let after = env_at(&scn, &f.space, 0.6, 16.0 * 3600.0);
    let stalls: Vec<usize> = f.space.places.iter().enumerate().filter(|(_, p)| p.kind == PlaceKind::Stall).map(|(i, _)| i).collect();
    assert!(stalls.len() >= 6);
    assert!(stalls.iter().all(|&i| !before.stall_active[i] && after.stall_active[i]));
    let foot = std::f64::consts::PI * STALL_RADIUS_M * STALL_RADIUS_M;
    let mut per_edge: BTreeMap<u32, usize> = BTreeMap::new();
    for &i in &stalls {
        *per_edge.entry(f.space.places[i].edge).or_default() += 1;
    }
    for (&e, &k) in &per_edge {
        let es = &f.space.edges[e as usize];
        let areas: Vec<f64> = (0..=k).map(|j| es.walk_area(&after, j as f64 * foot)).collect();
        assert!(areas.windows(2).all(|x| x[1] < x[0]), "간선 {}: 노점 수에 따른 면적 {areas:?}", es.id);
    }
    // 노점 간선마다 몸 몇을 세워 두고 집단 상태의 간선 밀도를 비교
    let m = mid_quick();
    let tmpl = quiet_walker(m);
    let mut bodies = Vec::new();
    for (&e, _) in per_edge.iter() {
        for j in 0..4 {
            let mut b = tmpl.clone();
            b.id = bodies.len() as u32;
            b.cur_edge = Some(e);
            b.pos = f.space.edges[e as usize].poly[0] + V2::new(j as f64, 0.0);
            bodies.push(b);
        }
    }
    let r = 2.0;
    let (c0, c1) = (Collective::build(&bodies, &f.space, &before, r), Collective::build(&bodies, &f.space, &after, r));
    for e in 0..f.space.edges.len() {
        if per_edge.contains_key(&(e as u32)) {
            assert!(c1.edge_density[e] > c0.edge_density[e], "노점 간선 {}: 밀도 {} → {}", f.space.edges[e].id, c0.edge_density[e], c1.edge_density[e]);
        } else {
            assert_eq!(c1.edge_density[e], c0.edge_density[e]);
        }
    }
}

/// 차량 통행 시간대에만 차로 몫(carriage_share)이 유효 면적에서 빠지고 D-SAFE 요구가 생긴다.
/// 보행 전용 시간(#1)은 ped_zone 간선의 차량을 없애고, 차량 통행 시간 [시작, 끝)은 반열린 구간이다.
#[test]
fn e210_4_vehicle_hours() {
    let f = fx();
    let scn = scenario("variant"); // 보행 전용 16:00~20:00
    let veh_on = env_at(&scn, &f.space, 0.6, 16.0 * 3600.0 - 0.5);
    let ped_only = env_at(&scn, &f.space, 0.6, 16.0 * 3600.0);
    assert!(veh_on.vehicle_hours && !veh_on.ped_only && ped_only.ped_only);
    let mut off = veh_on.clone();
    off.vehicle_hours = false;
    let (mut n_mixed, mut n_zone) = (0, 0);
    for e in &f.space.edges {
        let full = e.len * if e.sidewalks { 2.0 * e.sw } else { e.width };
        let safe = |env: &EnvState| e.demand_of(true, env)[Dim::Safe.ix()];
        if e.vehicle_mixed && !e.sidewalks {
            n_mixed += 1;
            assert!((e.walk_area(&veh_on, 0.0) - full * (1.0 - e.carriage_share)).abs() < 1e-9, "간선 {} 차량 시간 면적", e.id);
            assert!(safe(&veh_on) > 0.0, "간선 {} 차량 시간 D-SAFE", e.id);
            assert_eq!(e.walk_area(&off, 0.0), full, "차량 시간 밖에는 차로 몫이 빠지지 않는다");
            assert_eq!(safe(&off), 0.0);
            if e.ped_zone {
                n_zone += 1;
                assert_eq!(e.walk_area(&ped_only, 0.0), full, "보행 전용 시간의 ped_zone");
                assert_eq!(safe(&ped_only), 0.0);
            } else {
                assert!(safe(&ped_only) > 0.0, "ped_zone이 아닌 혼재 간선은 보행 전용 시간에도 차가 다닌다");
            }
        } else if !e.is_crossing {
            assert_eq!(e.walk_area(&veh_on, 0.0), full, "혼재가 아닌 간선 {}", e.id);
            assert_eq!(safe(&veh_on), 0.0);
        }
    }
    assert!(n_mixed > 3 && n_zone > 0);
    // 차량 통행 시간 [10:00, 20:00)의 경계
    let hours = [(600.0, 1200.0)];
    let at = |min: f64| env_at_hours(&scn, &f.space, 0.6, &hours, min * 60.0).vehicle_hours;
    assert!(!at(599.99) && at(600.0) && at(1199.99) && !at(1200.0), "반열린 구간");
}

/// 대기열은 음수가 되지 않고, 도착이 처리 용량 이하이면 늘지 않는다: 처리율(0.5명/분)보다 드물게 오면 줄은 0에서
/// 커지지 않고, 더 자주 오면(대조) 쌓인다. 실행 기록에서도 장소마다 줄 선 수 = 입장 + 포기 + 남은 줄이다.
#[test]
fn e210_5_queue_conservation() {
    let m = mid_quick();
    let run = |every_ticks: u64, n_arrivals: u32| -> (usize, u64, usize) {
        let pr = prep_empty(with_window(scenario("quick"), "14:00", "15:00"));
        let w = pr.w;
        let mut sim = Sim::new(w, &[]);
        let mut sink = MemSink::default();
        let p = w.space.places.iter().position(|pl| pl.kind == PlaceKind::Shop && (pl.service_rate - 0.5).abs() < 1e-9 && pl.capacity >= 20).unwrap() as u32;
        sim.run_until(300, &mut sink).expect("run");
        let served0 = sim.state.queues[p as usize].served;
        let mut max_len = 0;
        for k in 0..n_arrivals {
            inject_queuer(&mut sim, m, p, k);
            let t = sim.state.tick;
            sim.run_until(t + every_ticks, &mut sink).expect("run");
            let q = &sim.state.queues[p as usize];
            max_len = max_len.max(q.waiting.len());
            assert!(q.inside.len() as u32 <= w.space.places[p as usize].capacity, "처리 용량");
        }
        let q = &sim.state.queues[p as usize];
        (max_len, q.served - served0, q.waiting.len())
    };
    // 2.5분마다 1명(처리 간격 2분): 줄이 생기지 않는다
    let (max_len, served, left) = run(300, 10);
    assert_eq!((max_len, served, left), (0, 10, 0), "처리 용량 이하 도착에서 줄 최대 {max_len}, 입장 {served}, 남은 줄 {left}");
    // 30초마다 1명(대조): 줄이 쌓인다
    let (max_len, served, left) = run(60, 10);
    assert!(max_len >= 5 && served as usize + left == 10, "처리 용량을 넘는 도착: 줄 최대 {max_len}, 입장 {served}, 남은 줄 {left}");
    // 실행 기록의 보존: 장소마다 줄 선 수 = 입장(enter 사건, 마지막 틱에 들어가 아직 구매 결정 전인 몸 포함) + 포기 + 남은 줄
    let o = observed_quick();
    let mut count: BTreeMap<(u32, &str), i64> = BTreeMap::new();
    for e in &o.sink.events {
        // 줄에서 들어간 몸의 queue_leave(served)는 enter와 같은 일이라 포기(abandoned)만 센다(S-E55)
        if e.kind == "queue_leave" && e.info != "abandoned" {
            continue;
        }
        if let (Some(p), "queue_join" | "queue_leave" | "enter") = (e.place, e.kind.as_str()) {
            *count.entry((p, e.kind.as_str())).or_default() += 1;
        }
    }
    let mut n = 0;
    for (&(p, _), &j) in count.iter().filter(|(k, _)| k.1 == "queue_join") {
        let q = &o.state.queues[p as usize];
        let entered = count.get(&(p, "enter")).copied().unwrap_or(0);
        let left = count.get(&(p, "queue_leave")).copied().unwrap_or(0);
        let admitted_now = o.state.bodies.iter().filter(|b| b.alive && !b.is_background() && b.activity == Activity::Inside && b.act_place == Some(p) && b.pending.is_some()).count() as i64;
        assert_eq!(j, entered + admitted_now + left + q.waiting.len() as i64, "장소 {p}: 줄 선 {j} = 입장 {entered} + 막 들어감 {admitted_now} + 포기 {left} + 남은 줄 {}", q.waiting.len());
        n += j;
    }
    assert!(n > 20, "줄 선 수 {n}");
}

/// 혼잡을 보지 못한 몸의 판단 입력(D-CROWD·대기)은 실제 밀도와 무관한 시간대 사전 기대이고, 본 값은 신선도 기한
/// (observe.fresh_s) 동안만 쓰이며 기한이 지나면 사전 기대로 돌아간다.
#[test]
fn e210_7_unseen_crowd_prior() {
    let m = mid_quick();
    let w = m.w();
    let tick = m.state.tick;
    let c = ctx(w, &m.state.env, tick);
    let fresh = c.fresh_ticks();
    assert_eq!(fresh, (w.p.perception.fresh_s / w.dt()).round() as u64);
    let mut b = quiet_walker(m);
    b.obs_edges.clear();
    let prof = prof_of(w, &b);
    let u = w.rng.u("EXPLORE", &b.entity, tick, 0, 0);
    let unseen = next_situation(&c, prof, &b, None, None, u);
    for ch in &unseen {
        if let Some(path) = &ch.path {
            for e in path_exposures(&c, &b, path, 1.0).0 {
                assert_eq!(e.demand[Dim::Crowd.ix()], c.prior_density, "보지 못한 간선은 사전 기대");
            }
        }
    }
    // 실제 밀도가 바뀌어도(상황 구성은 실제 집단 상태를 읽지 않는다) 같은 상황
    let e0 = unseen.iter().find_map(|ch| ch.path.as_ref().and_then(|p| p.edges.first().map(|x| x.0))).expect("경로가 있는 대안");
    for (age, want_seen) in [(0u64, true), (fresh / 2, true), (fresh, true), (fresh + 1, false), (10 * fresh, false)] {
        let mut bb = b.clone();
        bb.obs_edges = vec![ObservedEdge { edge: e0, density: 3.3, tick: tick - age.min(tick) }];
        let got = c.believed_density(&bb, e0);
        if want_seen {
            assert_eq!(got, 3.3, "본 지 {age}틱: 본 값");
        } else {
            assert_eq!(got, c.prior_density, "본 지 {age}틱(기한 {fresh}틱): 사전 기대로 돌아감");
        }
    }
    // 줄: 본 줄 길이는 신선할 때만 대기로 쓴다
    let p = (0..w.space.places.len() as u32).find(|&p| w.space.places[p as usize].service_rate > 0.0 && b.aware_of(p).is_some()).expect("처리율이 있는 아는 장소");
    for (age, want) in [(0u64, true), (fresh, true), (fresh + 1, false)] {
        let mut bb = b.clone();
        let a = bb.aware.iter_mut().find(|a| a.place == p).unwrap();
        a.seen_queue = Some(12);
        a.last_seen_tick = Some(tick - age.min(tick));
        let wait = believed_wait_min(&bb, &c, p);
        let rate = w.space.places[p as usize].service_rate;
        assert_eq!(wait, if want { 12.0 / rate } else { 0.0 }, "본 지 {age}틱의 대기");
    }
}
