//! E221: 지각 — 시야·알아차림·시선(C-221)의 완료 기준 ①~⑦.
//!
//! 단위 시험은 고정 자료의 실제 몸(빠른 시나리오 10분 시점)을 복사해 위치·방향·시선을 정해 놓고 지각 함수를 부른다.
mod common;

use common::{lone, lone_coll, mid_quick, observed_quick, prep, prof_of, quiet_walker, sample_points, scenario, with_window, Mid};
use mdsim_core::bundle::Profile;
use mdsim_core::dims::{Dim, NI};
use mdsim_core::geom::{angle_between, point_at, V2};
use mdsim_runtime::behavior::effective_caps;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::env::env_at;
use mdsim_runtime::params::RtParams;
use mdsim_runtime::perception::{effective_fov, look_decide, look_situation, notice_pass_prob, notice_tick_prob, perceive, PerceptOut};
use mdsim_runtime::record::MemSink;
use mdsim_runtime::route::Path;
use mdsim_runtime::situation::{Action, AltTarget};
use mdsim_runtime::space::{PlaceKind, Space};
use mdsim_runtime::state::{Aware, Body, GazeTarget};
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::f64::consts::TAU;

fn coll_of(m: &Mid, b: &Body) -> Collective {
    lone_coll(m, b)
}

fn see(m: &Mid, p: &RtParams, prof: &Profile, b: &Body, tick: u64) -> PerceptOut {
    let w = m.w();
    perceive(w.space, &m.state.env, &coll_of(m, b), &vec![0; w.space.places.len()], p, prof, b, &w.rng, tick, w.dt())
}

/// 가시 판정(02d I-221-r): 초점(시선 기준 거리·각) 또는 주변(진행 방향 기준 거리·각) 안이고 건물에 가리지 않는다.
fn geometric(space: &Space, b: &Body, fov: &mdsim_runtime::perception::Fov, i: usize) -> (bool, bool) {
    let pl = &space.places[i];
    let to = pl.anchor - b.pos;
    let d = to.len();
    let in_focal = d <= fov.focal_range && (d < 0.5 || angle_between(to.angle(), b.gaze) <= fov.focal_angle_deg.to_radians() / 2.0);
    let in_peri = d <= fov.peri_range && (d < 0.5 || angle_between(to.angle(), b.heading) <= fov.peri_angle_deg.to_radians() / 2.0);
    (in_focal || in_peri, space.line_of_sight(b.pos, pl.anchor))
}

/// 시야 밖(거리·각 밖)이거나 건물에 가린 대상은 가시 집합에 없고 알아차릴 수 없다. 알아차림 계수를 크게 올려
/// (틱마다 거의 알아차리게) 같은 자리를 여러 틱 지켜봐도 가린 대상은 끝내 알아차리지 않는다.
#[test]
fn e221_1_out_of_view_invisible() {
    let m = mid_quick();
    let w = m.w();
    let tmpl = quiet_walker(m);
    let prof = prof_of(w, &tmpl);
    let mut p = w.p.clone();
    p.notice.b0 = 8.0; // 통과 확률 ≈ 1: 가시 집합 안이면 몇 틱 안에 알아차린다
    let pts = sample_points(w.space);
    let (mut n_vis, mut n_occl, mut n_out) = (0, 0, 0);
    let mut occluded_cases: Vec<(V2, f64, f64, u32)> = Vec::new();
    for (k, &pos) in pts.iter().enumerate() {
        for h in 0..8 {
            let heading = h as f64 * TAU / 8.0;
            let gaze = heading + [0.0, 0.7, -0.7][k % 3];
            let b = lone(m, pos, heading, gaze);
            let coll = coll_of(m, &b);
            let fov = effective_fov(prof, &b, &m.state.env, coll.local_density[0], &p, m.state.tick, w.dt());
            let po = see(m, &p, prof, &b, m.state.tick);
            assert_eq!(po.fov, Some(fov));
            let vis: BTreeSet<u32> = po.visible.iter().map(|x| x.0).collect();
            for i in 0..w.space.places.len() {
                let (in_view, los) = geometric(w.space, &b, &fov, i);
                assert_eq!(vis.contains(&(i as u32)), in_view && los, "위치 ({:.1},{:.1}) 방향 {h} 장소 {}", pos.x, pos.z, w.space.places[i].id);
                match (in_view, los) {
                    (true, true) => n_vis += 1,
                    (true, false) => {
                        n_occl += 1;
                        occluded_cases.push((pos, heading, gaze, i as u32));
                    }
                    _ => n_out += 1,
                }
            }
            assert!(po.notices.iter().all(|n| vis.contains(n)), "가시 집합 밖 알아차림");
        }
    }
    assert!(n_vis > 200 && n_occl > 20 && n_out > 1000, "보임 {n_vis} 가림 {n_occl} 밖 {n_out}");
    // 가린 대상: 200틱 동안 지켜봐도 알아차리지 않는다(같은 자리의 보이는 대상은 알아차린다)
    let mut noticed_visible = 0;
    for &(pos, heading, gaze, hidden) in occluded_cases.iter().step_by((occluded_cases.len() / 25).max(1)) {
        let b = lone(m, pos, heading, gaze);
        for t in 0..200 {
            let po = see(m, &p, prof, &b, m.state.tick + t);
            assert!(!po.notices.contains(&hidden), "가린 장소 {} 알아차림", w.space.places[hidden as usize].id);
            noticed_visible += po.notices.len();
        }
    }
    assert!(noticed_visible > 0, "보이는 대상은 알아차려야 시험이 의미가 있다");
}

/// 같은 위치·시선·인파에서 시야가 넓고 긴 몸의 가시 집합은 좁고 짧은 몸의 것을 포함한다.
#[test]
fn e221_2_wider_superset() {
    let m = mid_quick();
    let w = m.w();
    let tmpl = quiet_walker(m);
    let base = prof_of(w, &tmpl).clone();
    let mut narrow = base.clone();
    (narrow.perc.focal_range_m, narrow.perc.focal_angle_deg, narrow.perc.peripheral_range_m, narrow.perc.peripheral_angle_deg) = (12.0, 30.0, 5.0, 90.0);
    let mut wide = base.clone();
    (wide.perc.focal_range_m, wide.perc.focal_angle_deg, wide.perc.peripheral_range_m, wide.perc.peripheral_angle_deg) = (30.0, 80.0, 12.0, 180.0);
    let (mut strict, mut total) = (0, 0);
    for (k, &pos) in sample_points(w.space).iter().enumerate() {
        for h in 0..8 {
            let heading = h as f64 * TAU / 8.0;
            let b = lone(m, pos, heading, heading + [0.0, 0.5, -0.5][k % 3]);
            let vn: BTreeSet<u32> = see(m, &w.p, &narrow, &b, m.state.tick).visible.iter().map(|x| x.0).collect();
            let vw: BTreeSet<u32> = see(m, &w.p, &wide, &b, m.state.tick).visible.iter().map(|x| x.0).collect();
            assert!(vn.is_subset(&vw), "넓은 시야가 좁은 시야의 대상을 놓침: {:?}", vn.difference(&vw).collect::<Vec<_>>());
            strict += usize::from(vw.len() > vn.len());
            total += 1;
        }
    }
    assert!(strict > total / 10, "넓은 시야가 실제로 더 보는 경우 {strict}/{total}");
}

/// 시계성·광고 자극도(·주목 성향)가 오르면 통과 확률이, 시선이 대상 쪽일수록(초점 > 주변 > 시야 밖) 틱 확률이
/// 내려가지 않는다(C-214 ② 유지). 인파 밀도는 반대 방향이다.
#[test]
fn e221_3_monotone_notice_gaze() {
    let np = &mid_quick().w().p.notice;
    let grid: Vec<f64> = (0..=10).map(|i| i as f64 / 10.0).collect();
    let rhos = [0.0, 0.3, 1.0, 2.5];
    for &rho in &rhos {
        for &att in &grid {
            for &ad in &grid {
                let mut prev = -1.0;
                for &vis in &grid {
                    let p = notice_pass_prob(np, vis, ad, att, rho);
                    assert!(p >= prev, "시계성 증가에 통과 확률 감소");
                    prev = p;
                }
            }
            for &vis in &grid {
                let mut prev = -1.0;
                for &ad in &grid {
                    let p = notice_pass_prob(np, vis, ad, att, rho);
                    assert!(p >= prev, "광고 자극도 증가에 통과 확률 감소");
                    prev = p;
                }
            }
        }
    }
    for &vis in &grid {
        let ps: Vec<f64> = rhos.iter().map(|&r| notice_pass_prob(np, vis, 0.5, 0.5, r)).collect();
        assert!(ps.windows(2).all(|x| x[1] <= x[0]), "밀도가 오르면 통과 확률은 오르지 않는다");
        let pa: Vec<f64> = grid.iter().map(|&a| notice_pass_prob(np, vis, 0.5, a, 0.3)).collect();
        assert!(pa.windows(2).all(|x| x[1] >= x[0]), "주목 성향");
    }
    let dt = 0.5;
    for i in 1..20 {
        let p = i as f64 / 20.0;
        let focal = notice_tick_prob(np, p, 1.0, dt);
        let peri = notice_tick_prob(np, p, np.w_peripheral, dt);
        let out = notice_tick_prob(np, p, 0.0, dt);
        assert!(focal >= peri && peri >= out && out == 0.0, "초점 {focal} ≥ 주변 {peri} ≥ 시야 밖 {out} = 0");
        assert!(notice_tick_prob(np, (p + 0.05).min(0.99), 1.0, dt) >= focal);
    }
    assert!(np.w_peripheral > 0.0 && np.w_peripheral < 1.0, "주변 시야 가중은 초점보다 작다");
}

/// 시선이 중립인 보통 통과 한 번(초점 안에 T_ref초)의 누적 알아차림 확률이 C-214의 통과 확률과 같다(교정 보존).
/// 해석적으로 같고, 키드 난수로 3,000번 통과를 돌린 비율도 이항 표준오차의 4배 안에서 같다.
#[test]
fn e221_4_notice_pass_calibration() {
    let m = mid_quick();
    let w = m.w();
    let np = &w.p.notice;
    let dt = w.dt();
    let n = (np.t_ref_s / dt).round() as i32;
    for i in 1..50 {
        let p = i as f64 / 50.0;
        let q = notice_tick_prob(np, p, 1.0, dt);
        let cum = 1.0 - (1.0 - q).powi(n);
        assert!((cum - p).abs() < 1e-9, "통과 확률 {p} vs 누적 {cum}");
    }
    // 실제 지각 함수로: 길 위 가게 앞 6m에서 가게를 정면으로(시선 = 진행 방향) T_ref초 지켜본다
    let tmpl = quiet_walker(m);
    let prof = prof_of(w, &tmpl);
    let (pi, pos) = w
        .space
        .places
        .iter()
        .enumerate()
        .filter(|(_, pl)| pl.kind == PlaceKind::Shop)
        .find_map(|(i, pl)| {
            let es = &w.space.edges[pl.edge as usize];
            let (s, _) = mdsim_core::geom::project_on_polyline(&es.poly, &es.cum, pl.anchor);
            let (c, _) = point_at(&es.poly, &es.cum, (s + 6.0).min(es.len));
            (w.space.walkable_static(c) && c.dist(pl.anchor) > 3.0 && w.space.line_of_sight(c, pl.anchor)).then_some((i as u32, c))
        })
        .expect("가게 앞 자리");
    let anchor = w.space.places[pi as usize].anchor;
    let heading = (anchor - pos).angle();
    let b0 = lone(m, pos, heading, heading);
    let coll = coll_of(m, &b0);
    let pl = &w.space.places[pi as usize];
    let p_pass = notice_pass_prob(np, pl.visibility, pl.ad, prof.attention, coll.local_density[0]);
    let trials = 3000;
    let mut hit = 0;
    for k in 0..trials {
        let mut b = b0.clone();
        b.entity = format!("calib#{k}");
        for t in 0..n as u64 {
            let po = see(m, &w.p, prof, &b, m.state.tick + t);
            assert!(po.visible.contains(&(pi, true)), "초점 안");
            if po.notices.contains(&pi) {
                hit += 1;
                break;
            }
        }
    }
    let frac = hit as f64 / trials as f64;
    let se = (p_pass * (1.0 - p_pass) / trials as f64).sqrt();
    assert!((frac - p_pass).abs() <= 4.0 * se, "통과 {trials}번의 알아차림 비율 {frac:.4} vs C-214 확률 {p_pass:.4}(±{:.4})", 4.0 * se);
}

/// 시선은 고개 회전 한계를 넘지 않고(그 틱을 시작할 때의 진행 방향 기준), 최소 유지 시간 전에는 바뀌지 않는다
/// (예외: 대상 → 앞길, 즉 충돌 위험·회전 한계). 한계 밖 대상의 LOOK_AT은 막혀 확률 0이다.
#[test]
fn e221_5_head_turn_and_dwell() {
    let o = observed_quick();
    let w = o.prep.w;
    let dt = w.dt();
    let (mut n_place, mut n_change) = (0, 0);
    let mut last_change: std::collections::BTreeMap<u32, (u64, GazeTarget)> = Default::default();
    for t in 0..o.snaps.len() - 1 {
        let (now, next) = (&o.snaps[t], &o.snaps[t + 1]);
        for s in next.iter().filter(|s| s.alive && s.domestic) {
            let prof = &w.bundle.profiles[o.state.bodies[s.id as usize].profile.unwrap() as usize];
            if let (Some(before), GazeTarget::Place(_)) = (now.get(s.id as usize), s.gaze_target) {
                if before.alive && before.activity.on_street() {
                    let a = angle_between(s.gaze, before.heading).to_degrees();
                    assert!(a <= prof.perc.head_turn_deg + 1e-9, "몸 {} 틱 {t}: 시선이 진행 방향에서 {a:.1}°", s.id);
                    n_place += 1;
                }
            }
            let dwell = (prof.perc.min_dwell_s / dt).round().max(1.0) as u64;
            let e = last_change.entry(s.id).or_insert((o.state.bodies[s.id as usize].arrival_tick, GazeTarget::Path));
            if e.1 != s.gaze_target {
                let held = t as u64 - e.0;
                let exception = matches!(e.1, GazeTarget::Place(_)) && s.gaze_target == GazeTarget::Path;
                assert!(held >= dwell || exception, "몸 {} 틱 {t}: {:?} → {:?}를 {held}틱 만에 바꿈(최소 {dwell})", s.id, e.1, s.gaze_target);
                *e = (t as u64, s.gaze_target);
                n_change += 1;
            }
        }
    }
    assert!(n_place > 1000 && n_change > 500, "장소 보기 {n_place}, 시선 전환 {n_change}");
    // 단위: 진행 방향 뒤쪽의 대상은 LOOK_AT이 막히고 확률 0
    let m = mid_quick();
    let tmpl = quiet_walker(m);
    let prof = prof_of(m.w(), &tmpl);
    let pl_ix = m.w().space.places.iter().position(|pl| pl.kind == PlaceKind::Shop).unwrap() as u32;
    let anchor = m.w().space.places[pl_ix as usize].anchor;
    for back in [false, true] {
        let pos = anchor + V2::new(4.0, 0.0);
        let to = (anchor - pos).angle();
        let heading = if back { to + std::f64::consts::PI } else { to };
        let mut b = lone(m, pos, heading, heading);
        b.aware.push(Aware { place: pl_ix, first_tick: 0, last_seen_tick: Some(0), seen_open: Some(true), seen_queue: Some(0), known_before: true });
        let ch = look_situation(m.w().space, &m.state.env, &m.w().p, &m.w().defs.dims, &b, &[(pl_ix, true)], 0.1, m.state.tick, m.w().dt(), prof.perc.head_turn_deg);
        let k = ch.iter().position(|c| c.target == AltTarget::Place(pl_ix)).expect("LOOK_AT 대안");
        assert_eq!(ch[k].alt.blocked, back, "회전 한계 밖이면 막힘");
        let caps = effective_caps(m.w().defs, prof, &b, 1e6);
        let r = look_decide(&ch, prof, &b, &m.w().defs.dims, &m.w().p, &m.w().rng, m.state.tick, &caps);
        if back {
            assert_eq!(r.probs[k], 0.0);
        }
    }
}

/// LOOK의 단조성: 관심이 클수록 그 대상을 볼 확률이, 앞길의 발밑 요구가 클수록 발밑을 볼 확률이 작아지지 않는다.
/// 발밑 요구는 경로 간선의 D-STEP(0~40cm, 클래스 속성 범위)과 D-FALL(0~1, 식의 clamp 범위)을 바꿔 가며 본다.
#[test]
fn e221_6_look_monotone() {
    let (n_interest, bad_footing) = look_monotone_counts();
    assert!(n_interest > 0);
    assert!(bad_footing.is_empty(), "발밑 요구가 커졌는데 FOOTING 확률 감소 {}건, 예: {:?}", bad_footing.len(), &bad_footing[..bad_footing.len().min(5)]);
}

/// 관심 쪽 단조성(E221-6의 앞 절반)은 성립한다.
#[test]
fn e221_6_look_interest_monotone() {
    let (n_interest, _) = look_monotone_counts();
    assert!(n_interest > 100, "검사 수 {n_interest}");
}

/// (관심 단조성 검사 수, 발밑 단조성 위반 목록). 관심 단조성 위반은 그 자리에서 실패시킨다.
fn look_monotone_counts() -> (usize, Vec<String>) {
    let m = mid_quick();
    let w = m.w();
    let tick = m.state.tick;
    // 평지 간선 하나를 따라가는 경로(앞 lookahead_s초가 그 간선 안)
    let e0 = w.space.edges.iter().position(|e| !e.sidewalks && !e.is_steps && e.grade_ab.abs() < 1e-9 && e.len > 40.0).unwrap() as u32;
    let es = &w.space.edges[e0 as usize];
    let path = Path { nodes: vec![es.a, es.b], edges: vec![(e0, true)], length: es.len };
    let (start, dir) = point_at(&es.poly, &es.cum, 5.0);
    let heading = dir.angle();
    // 관심: 앞쪽 10m 남짓의 가게 하나를 볼지
    let mut n_interest = 0;
    let shops: Vec<u32> = w.space.places.iter().enumerate().filter(|(_, pl)| pl.kind == PlaceKind::Shop).map(|(i, _)| i as u32).collect();
    for prof0 in w.bundle.profiles.iter().step_by(23) {
        for &pi in shops.iter().step_by(9) {
            let pl = &w.space.places[pi as usize];
            let k = (0..NI).max_by(|&i, &j| pl.offer[i].total_cmp(&pl.offer[j])).unwrap();
            let pos = pl.anchor + V2::from_angle(heading + std::f64::consts::PI) * 8.0;
            let mut b = lone(m, pos, heading, heading);
            b.sat = [0.0; NI];
            b.aware.push(Aware { place: pi, first_tick: 0, last_seen_tick: Some(0), seen_open: Some(true), seen_queue: Some(0), known_before: true });
            let mut prev = -1.0;
            for step in 0..=10 {
                let mut prof = prof0.clone();
                prof.interest[k] = step as f64 / 10.0;
                let ch = look_situation(w.space, &m.state.env, &w.p, &w.defs.dims, &b, &[(pi, true)], 0.1, tick, w.dt(), prof.perc.head_turn_deg);
                let caps = effective_caps(w.defs, &prof, &b, 1e6);
                let r = look_decide(&ch, &prof, &b, &w.defs.dims, &w.p, &w.rng, tick, &caps);
                let i = ch.iter().position(|c| c.target == AltTarget::Place(pi)).unwrap();
                assert!(r.probs[i] >= prev - 1e-12, "{} 관심 {:.1}에서 장소 {} 볼 확률 {prev} → {}", prof.persona_id, step as f64 / 10.0, pl.id, r.probs[i]);
                prev = r.probs[i];
                n_interest += 1;
            }
        }
    }
    // 발밑: 경로 간선의 D-STEP·D-FALL을 키운 공간
    let mut bad = Vec::new();
    let levels: [(Dim, Vec<f64>); 2] = [(Dim::Step, (0..=16).map(|i| i as f64 * 2.5).collect()), (Dim::Fall, (0..=20).map(|i| i as f64 * 0.05).collect())];
    for (dim, vals) in &levels {
        for prof in w.bundle.profiles.iter().step_by(5) {
            let mut b = lone(m, start, heading, heading);
            b.route = Some(common::route_plan(&path, es.poly[es.poly.len() - 1]));
            b.cur_edge = Some(e0);
            let caps = effective_caps(w.defs, prof, &b, 1e6);
            let mut prev: Option<f64> = None;
            for &v in vals {
                let mut space = w.space.clone();
                for dir_ in 0..2 {
                    for veh in 0..2 {
                        space.edges[e0 as usize].demand[dir_][veh][dim.ix()] = v;
                    }
                }
                let ch = look_situation(&space, &m.state.env, &w.p, &w.defs.dims, &b, &[], 0.1, tick, w.dt(), prof.perc.head_turn_deg);
                let r = look_decide(&ch, prof, &b, &w.defs.dims, &w.p, &w.rng, tick, &caps);
                let i = ch.iter().position(|c| c.action == Action::Footing).unwrap();
                if let Some(pv) = prev {
                    if r.probs[i] < pv - 1e-12 {
                        bad.push(format!("{} {} {v:.2}: {pv:.4} → {:.4}", prof.persona_id, dim.id(), r.probs[i]));
                    }
                }
                prev = Some(r.probs[i]);
            }
        }
    }
    (n_interest, bad)
}

/// 관찰 값은 본 틱을 갖고 그 틱의 참값(영업 여부·줄 길이·간선 밀도)만 담는다(그 뒤의 참값을 담지 않는다).
/// 15시에 닫는 가게가 있는 14:40~15:10 실행에서, 끝난 뒤 남은 모든 관찰 값을 그 틱의 참값과 대조한다.
#[test]
fn e221_7_observation_no_future() {
    let pr = prep(with_window(scenario("base"), "14:40", "15:10"));
    let w = pr.w;
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    let n = w.scn.n_ticks();
    let mut edge_density: Vec<Vec<f64>> = Vec::with_capacity(n as usize);
    let mut queue_len: Vec<Vec<u32>> = Vec::with_capacity(n as usize);
    while sim.state.tick < n {
        let t = sim.state.tick;
        // 새로 도착한 몸은 cur_edge가 없어 간선 밀도에 들지 않으므로, step 전 몸들로 ③의 간선 밀도를 다시 만들 수 있다
        let env = env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(t));
        edge_density.push(Collective::build(&sim.state.bodies, w.space, &env, w.p.crowd.local_radius_m).edge_density);
        queue_len.push(sim.state.queues.iter().map(|q| q.waiting.len() as u32).collect());
        sim.run_until(t + 1, &mut sink).expect("step");
    }
    let closing = w.space.places.iter().position(|pl| pl.close_min == 900.0).expect("15시 마감 장소") as u32;
    let (mut n_open_obs, mut n_stale_open, mut n_edges) = (0, 0, 0);
    for b in &sim.state.bodies {
        for a in &b.aware {
            let Some(t) = a.last_seen_tick else { continue };
            assert!(t < n, "관찰 틱은 실행 안");
            let env = env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(t));
            assert_eq!(a.seen_open, Some(env.open[a.place as usize]), "몸 {} 장소 {}: 본 틱 {t}의 영업 여부", b.id, a.place);
            if a.place == closing {
                n_open_obs += 1;
                // 15시 전에 열린 것을 보고 그 뒤로 보지 않았다면, 실제로는 닫혔어도 관찰 값은 열림 그대로다
                if a.seen_open == Some(true) && w.t_s(t) < 900.0 * 60.0 && b.alive {
                    n_stale_open += 1;
                }
            }
            if let Some(q) = a.seen_queue {
                assert_eq!(q, queue_len[t as usize][a.place as usize], "몸 {} 장소 {}: 본 틱 {t}의 줄 길이", b.id, a.place);
            }
        }
        for o in &b.obs_edges {
            assert!(o.tick < n);
            assert_eq!(o.density, edge_density[o.tick as usize][o.edge as usize], "몸 {} 간선 {}: 본 틱 {}의 밀도", b.id, o.edge, o.tick);
            n_edges += 1;
        }
    }
    assert!(n_open_obs > 0 && n_edges > 100, "닫히는 가게 관찰 {n_open_obs}, 간선 관찰 {n_edges}");
    assert!(n_stale_open > 0, "15시 전에 본 '열림'은 닫힌 뒤에도 관찰 값으로 남는다(미래 참값을 담지 않음)");
}

/// 같은 S(t)와 키는 몸 처리 순서(정순·역순·병렬·몸 번호 다시 매기기)와 무관하게 같은 알아차림·관찰 값을 낸다(E221-7 뒤 절반).
#[test]
fn e221_7_observation_order_invariance() {
    let m = mid_quick();
    let w = m.w();
    let st = &m.state;
    let coll = Collective::build(&st.bodies, w.space, &st.env, w.p.crowd.local_radius_m);
    let ql: Vec<u32> = st.queues.iter().map(|q| q.waiting.len() as u32).collect();
    let ids: Vec<usize> = st.bodies.iter().filter(|b| b.alive && !b.is_background() && b.activity.on_street()).map(|b| b.id as usize).collect();
    let run = |bodies: &[Body], coll: &Collective, i: usize| -> PerceptOut { perceive(w.space, &st.env, coll, &ql, &w.p, prof_of(w, &bodies[i]), &bodies[i], &w.rng, st.tick, w.dt()) };
    let key = |po: &PerceptOut| (po.visible.clone(), po.notices.clone(), po.seen.clone(), po.seen_edges.iter().map(|x| (x.0, x.1.to_bits())).collect::<Vec<_>>(), po.fov);
    let fwd: Vec<_> = ids.iter().map(|&i| key(&run(&st.bodies, &coll, i))).collect();
    let rev: Vec<_> = ids.iter().rev().map(|&i| key(&run(&st.bodies, &coll, i))).collect::<Vec<_>>().into_iter().rev().collect();
    let par: Vec<_> = ids.par_iter().map(|&i| key(&run(&st.bodies, &coll, i))).collect();
    assert_eq!(fwd, rev);
    assert_eq!(fwd, par);
    // 몸 번호를 거꾸로 매긴 같은 세계
    let nb = st.bodies.len();
    let mut relabeled: Vec<Body> = st.bodies.iter().rev().cloned().collect();
    for (k, b) in relabeled.iter_mut().enumerate() {
        b.id = k as u32;
    }
    let coll2 = Collective::build(&relabeled, w.space, &st.env, w.p.crowd.local_radius_m);
    for (j, &i) in ids.iter().enumerate() {
        assert_eq!(coll2.local_density[nb - 1 - i], coll.local_density[i], "국소 밀도");
        assert_eq!(key(&run(&relabeled, &coll2, nb - 1 - i)), fwd[j], "몸 {i}(새 번호 {})의 지각", nb - 1 - i);
    }
    assert_eq!(coll2.edge_density, coll.edge_density);
}
