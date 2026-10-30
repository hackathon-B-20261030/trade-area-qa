//! E209: 방문지 선택과 도착 계획(C-209의 런타임 쪽)의 완료 기준 ①~⑦.
mod common;

use common::{fx, observed_quick, prep, scenario};
use mdsim_core::bundle::{Avail, Bundle, Group, Mode, PortalKind};
use mdsim_core::rng::KeyRng;
use mdsim_core::scenario::{DayType, Scenario};
use mdsim_runtime::params::RtParams;
use mdsim_runtime::visit::{exit_walk_min, plan_arrivals, plan_background, representation_ratio, visit_decisions, SpawnKind, VisitDecision, VisitKind};
use std::collections::BTreeMap;

fn params() -> RtParams {
    RtParams::load(&fx().defs.params).expect("params")
}

fn decide(b: &Bundle, scn: &Scenario) -> Vec<VisitDecision> {
    let f = fx();
    visit_decisions(&f.defs, &f.space, b, scn, &params(), &KeyRng::new(scn.seed))
}

/// 인물별 명동(모든 슬롯 × 수단) 방문 확률 합.
fn p_myeongdong(d: &VisitDecision) -> f64 {
    d.alts.iter().filter(|a| matches!(a.kind, VisitKind::Myeongdong { .. })).map(|a| a.prob).sum()
}

fn with_day(day: &str) -> Scenario {
    let mut s = scenario("quick");
    s.day_type = day.into();
    s
}

/// 모든 인물에 하루 유형마다 방문지 선택(대안별 확률)과 그 근거(설명 상위 항, 접근 시간 분해)가 남는다.
#[test]
fn e209_1_all_profiles_decided() {
    let f = fx();
    for day in ["weekday", "weekend", "event_day"] {
        let dec = decide(&f.bundle, &with_day(day));
        assert_eq!(dec.len(), f.bundle.profiles.len(), "{day}: 인물마다 결정 하나");
        for (i, d) in dec.iter().enumerate() {
            assert_eq!(d.profile as usize, i);
            let r = &d.reaction;
            assert_eq!(r.probs.len(), d.alts.len());
            assert_eq!(d.alt_specs.len(), d.alts.len());
            let s: f64 = r.probs.iter().sum();
            assert!((s - 1.0).abs() < 1e-9, "{day} 인물 {i}: 확률 합 {s}");
            assert!(!r.top3.is_empty(), "{day} 인물 {i}: 근거(설명 항) 없음");
            for (a, rec) in d.alts.iter().zip(&r.probs) {
                assert_eq!(a.prob, *rec, "기록 확률 = 반응 확률");
                assert!(a.base_min.is_finite() && a.wait_min >= 0.0 && a.exit_walk_min >= 0.0);
            }
            assert!(d.alts.iter().any(|a| matches!(a.kind, VisitKind::Skip)), "안 감 대안");
        }
    }
}

/// 몸의 도착 시각(틱 → 시각)이 그 인물의 그날 가용 시간 창 [시작, 끝) 안이다.
fn arrivals_outside(b: &Bundle, scn: &Scenario) -> (usize, Vec<String>) {
    let f = fx();
    let p = params();
    let rng = KeyRng::new(scn.seed);
    let dec = visit_decisions(&f.defs, &f.space, b, scn, &p, &rng);
    let ratio = representation_ratio(&dec, scn, &p);
    let spawns = plan_arrivals(&f.space, b, scn, &p, &rng, &dec, ratio);
    let (w0, _) = scn.window_s();
    let day = scn.day();
    let mut bad = Vec::new();
    for s in &spawns {
        let SpawnKind::Domestic { profile, .. } = s.kind else { continue };
        let prof = &b.profiles[profile as usize];
        let t_min = (w0 + s.tick as f64 * scn.dt_s) / 60.0;
        // 틱은 도착 초를 내림한 것이므로 틱 시각은 실제 도착보다 최대 Δt 이르다
        let ok = prof.availability.iter().filter(|a| a.day_type == day).any(|a| t_min >= a.start_min - scn.dt_s / 60.0 && t_min < a.end_min);
        if !ok {
            bad.push(format!("{} {:?} {:.2}분", prof.persona_id, s.mode, t_min));
        }
    }
    (spawns.len(), bad)
}

/// 가용 시간 창 경계: 고정 자료(창이 10분 눈금에 맞음)와, 창 끝이 10분 슬롯 가운데에 걸리는 인물(14:05~14:25)
/// 모두에서 도착 시각이 창 안에 있다.
#[test]
fn e209_2_availability_bounds() {
    let f = fx();
    let (n, bad) = arrivals_outside(&f.bundle, &scenario("quick"));
    assert!(n > 100);
    assert!(bad.is_empty(), "고정 자료에서 창 밖 도착: {bad:?}");
    let mut b = f.bundle.clone();
    for prof in b.profiles.iter_mut() {
        prof.availability.retain(|a| a.day_type != DayType::Weekend);
        prof.availability.push(Avail { day_type: DayType::Weekend, start_min: 14.0 * 60.0 + 5.0, end_min: 14.0 * 60.0 + 25.0 });
    }
    let mut scn = scenario("quick");
    scn.representation_ratio = Some(3.0); // 인물마다 몸을 여럿 만들어 경계를 자주 시험한다
    let (n, bad) = arrivals_outside(&b, &scn);
    assert!(n > 100, "도착 {n}");
    assert!(bad.is_empty(), "창(14:05~14:25) 밖 도착 {}건, 예: {:?}", bad.len(), &bad[..bad.len().min(5)]);
}

/// 고정 자료의 가용 창(10분 눈금)에서는 도착이 모두 창 안이다(경계 시험의 회귀 확인용 부분).
#[test]
fn e209_2_availability_bounds_aligned_windows() {
    let f = fx();
    for scn in [scenario("quick"), scenario("variant")] {
        let (n, bad) = arrivals_outside(&f.bundle, &scn);
        assert!(n > 100);
        assert!(bad.is_empty(), "창 밖 도착: {bad:?}");
    }
}

/// 접근 시간은 기본 이동 시간 + 배차 대기 + 출구 보행으로 나뉘어 기록되고, 대안의 D-TIME 노출(dt_min)이 그 합이다.
#[test]
fn e209_3_access_decomposition() {
    let f = fx();
    let p = params();
    for scn in [scenario("quick"), { let mut s = scenario("quick"); s.headway_multiplier = 1.7; s }] {
        let dec = decide(&f.bundle, &scn);
        let mut n = 0;
        for d in &dec {
            let prof = &f.bundle.profiles[d.profile as usize];
            for (rec, alt) in d.alts.iter().zip(&d.alt_specs) {
                match &rec.kind {
                    VisitKind::Myeongdong { mode, .. } => {
                        let acc = f.bundle.access.iter().find(|a| a.origin_id == prof.origin_id && a.destination == "myeongdong" && a.mode == *mode).expect("접근 행");
                        assert_eq!(rec.base_min, acc.base_min);
                        let portal = rec.portal.expect("명동 대안의 출입구") as usize;
                        let want_wait = match mode {
                            Mode::Subway => f.space.portals[portal].headway_min.max(1.0) * scn.headway_multiplier / 2.0,
                            Mode::Bus => p.visit.bus_wait_min,
                            Mode::Taxi => p.visit.taxi_wait_min,
                            Mode::Walk => 0.0,
                        };
                        assert!((rec.wait_min - want_wait).abs() < 1e-12, "{:?} 대기 {} vs {}", mode, rec.wait_min, want_wait);
                        assert_eq!(rec.exit_walk_min, exit_walk_min(&f.space, prof, portal as u32, &p));
                        assert_eq!(alt.dt_min, rec.base_min + rec.wait_min + rec.exit_walk_min, "D-TIME = 기본 + 대기 + 출구 보행");
                        n += 1;
                    }
                    VisitKind::Competitor { .. } => {
                        assert_eq!(rec.exit_walk_min, 0.0);
                        assert_eq!(alt.dt_min, rec.base_min + rec.wait_min);
                    }
                    VisitKind::Skip => assert_eq!(alt.dt_min, 0.0),
                }
            }
        }
        assert!(n > 1000, "명동 대안 {n}");
    }
}

/// 배차 간격(#9)이 늘면 개인의 명동 방문 확률은 늘지 않는다(다른 조건 동일).
#[test]
fn e209_4_headway_monotone() {
    let f = fx();
    let mut prev: Option<Vec<f64>> = None;
    for mult in [0.5, 0.8, 1.0, 1.2, 1.5, 2.0, 3.0] {
        let mut scn = scenario("quick");
        scn.headway_multiplier = mult;
        let pm: Vec<f64> = decide(&f.bundle, &scn).iter().map(p_myeongdong).collect();
        if let Some(prev) = &prev {
            for (i, (a, b)) in prev.iter().zip(&pm).enumerate() {
                assert!(*b <= *a + 1e-12, "인물 {}: 배차 ×{mult}에서 명동 확률 {a} → {b}", f.bundle.profiles[i].persona_id);
            }
        }
        prev = Some(pm);
    }
}

/// 같은 단조성을 경쟁 상권에 가장 빠른 수단이 지하철인 인물에서도 본다(배차 간격 #9는 명동 역의 값이다).
#[test]
fn e209_4_headway_monotone_subway_competitor() {
    let f = fx();
    let mut b = f.bundle.clone();
    // 경쟁 상권까지 택시·버스를 느리게 해 가장 빠른 수단이 지하철이 되게 한다
    for a in b.access.iter_mut().filter(|a| a.destination != "myeongdong" && a.mode != Mode::Subway) {
        a.base_min += 120.0;
    }
    let mut prev: Option<Vec<f64>> = None;
    let mut worst = (0.0f64, String::new());
    for mult in [0.5, 1.0, 1.5, 2.0, 3.0] {
        let mut scn = scenario("quick");
        scn.headway_multiplier = mult;
        let pm: Vec<f64> = decide(&b, &scn).iter().map(p_myeongdong).collect();
        if let Some(prev) = &prev {
            for (i, (a, x)) in prev.iter().zip(&pm).enumerate() {
                if x - a > worst.0 {
                    worst = (x - a, format!("{} ×{mult}: {a:.6} → {x:.6}", b.profiles[i].persona_id));
                }
            }
        }
        prev = Some(pm);
    }
    assert!(worst.0 <= 1e-12, "배차 간격이 늘었는데 명동 확률 증가: {}", worst.1);
}

/// 방문 수요(#10)·외국인 유입(#11) 배율은 개인 확률과 대표 비율을 바꾸지 않고 몸의 수만 바꾼다.
#[test]
fn e209_5_multiplier_bodies_only() {
    let f = fx();
    let p = params();
    let count = |scn: &Scenario| -> (Vec<VisitDecision>, f64, BTreeMap<u32, usize>, usize) {
        let rng = KeyRng::new(scn.seed);
        let dec = visit_decisions(&f.defs, &f.space, &f.bundle, scn, &p, &rng);
        let ratio = representation_ratio(&dec, scn, &p);
        let mut by: BTreeMap<u32, usize> = BTreeMap::new();
        for s in plan_arrivals(&f.space, &f.bundle, scn, &p, &rng, &dec, ratio) {
            if let SpawnKind::Domestic { profile, .. } = s.kind {
                *by.entry(profile).or_default() += 1;
            }
        }
        let bg = plan_background(&f.space, &f.bundle, scn, &p, &rng).len();
        (dec, ratio, by, bg)
    };
    let base = scenario("quick");
    let (d1, r1, n1, bg1) = count(&base);
    let total = |m: &BTreeMap<u32, usize>, g: Group| -> usize { m.iter().filter(|(i, _)| f.bundle.profiles[**i as usize].group == g).map(|(_, n)| *n).sum() };
    for (dm, fm) in [(0.5, 1.0), (2.0, 1.0), (1.0, 0.5), (1.0, 2.0), (2.0, 2.0)] {
        let mut scn = base.clone();
        scn.demand_multiplier = dm;
        scn.foreign_inflow_multiplier = fm;
        let (d2, r2, n2, bg2) = count(&scn);
        for (a, b) in d1.iter().zip(&d2) {
            assert_eq!(a.reaction.probs, b.reaction.probs, "배율이 개인 확률을 바꿈");
        }
        assert_eq!(r1, r2, "대표 비율");
        // 같은 키드 균등값의 Poisson 역누적은 λ에 단조: 인물마다 몸 수가 배율 방향으로만 움직인다
        for (i, prof) in f.bundle.profiles.iter().enumerate() {
            let (a, b) = (n1.get(&(i as u32)).copied().unwrap_or(0), n2.get(&(i as u32)).copied().unwrap_or(0));
            let m = if prof.group == Group::Domestic { dm } else { fm };
            if m > 1.0 {
                assert!(b >= a, "{}: 배율 {m}에 몸 {a} → {b}", prof.persona_id);
            } else if m < 1.0 {
                assert!(b <= a);
            } else {
                assert_eq!(a, b, "{}: 다른 집단 배율이 몸 수를 바꿈", prof.persona_id);
            }
        }
        // 기대값은 배율에 비례한다(내국인 약 300명: 비율 오차 ±0.35는 Poisson 표준오차의 4배 이상)
        let (a, b) = (total(&n1, Group::Domestic) as f64, total(&n2, Group::Domestic) as f64);
        assert!((b / a - dm).abs() <= 0.35 * dm.max(1.0), "내국인 몸 {a} → {b} (배율 {dm})");
        if fm > 1.0 {
            assert!(bg2 >= bg1, "배경 보행자도 #11에 따라 는다");
        } else if fm < 1.0 {
            assert!(bg2 <= bg1);
        }
    }
}

/// 몸은 경계 안의 출구·숙소(출입구)에서만 생긴다: 도착 계획의 출입구가 수단에 맞고, 생성 사건의 출입구와 첫 위치가 그곳이다.
#[test]
fn e209_6_spawn_at_portals() {
    let f = fx();
    let pr = prep(scenario("quick"));
    for s in pr.spawns {
        let po = f.space.portals.get(s.portal as usize).expect("출입구 인덱스");
        assert!(f.space.bbox.contains(po.pos) && f.space.walkable_static(po.pos), "출입구 {}가 보행 공간 안", po.id);
        if let SpawnKind::Domestic { profile, .. } = s.kind {
            let prof = &f.bundle.profiles[profile as usize];
            assert!(po.groups.is_empty() || po.groups.contains(&prof.group), "{}: 그 집단이 쓰지 않는 출입구 {}", prof.persona_id, po.id);
            match s.mode {
                Mode::Subway => assert_eq!(po.kind, PortalKind::Subway),
                Mode::Walk => assert!(matches!(po.kind, PortalKind::Boundary | PortalKind::Hotel) || po.id == prof.origin_id, "도보 출입구 {:?}", po.kind),
                Mode::Bus | Mode::Taxi => assert!(matches!(po.kind, PortalKind::Bus | PortalKind::Boundary | PortalKind::Subway), "버스·택시 출입구 {:?}", po.kind),
            }
        }
    }
    let o = observed_quick();
    let spawn_ev: BTreeMap<i64, &mdsim_runtime::record::EventRow> = o.sink.events.iter().filter(|e| e.kind == "spawn").map(|e| (e.body, e)).collect();
    assert_eq!(spawn_ev.len(), o.state.bodies.len());
    let vmax = 1.45 * o.prep.w.p.loco.tobler_fmax * o.prep.w.dt();
    for b in &o.state.bodies {
        let po = &f.space.portals[b.portal_in as usize];
        assert_eq!(spawn_ev[&(b.id as i64)].info, po.id, "생성 사건의 출입구");
        let first = o.snaps[b.arrival_tick as usize + 1].iter().find(|s| s.id == b.id).expect("도착 다음 틱");
        // 출입구에서 1.5m 안에 흩어 생기고(step.rs의 SPAWN 흩기) 첫 틱에 한 걸음까지 움직인다
        assert!(first.pos.dist(po.pos) <= 1.5 + vmax + 1e-9, "몸 {} 첫 위치가 출입구 {}에서 {:.2}m", b.id, po.id, first.pos.dist(po.pos));
    }
}

/// 방문지 선택에는 정해진 경쟁 상권 대안이 모두(하나씩) 있다(하루 유형마다).
#[test]
fn e209_7_competitors_present() {
    let f = fx();
    let ids: Vec<String> = f.bundle.competitors.iter().map(|c| c.id.clone()).collect();
    assert!(ids.len() >= 2);
    for day in ["weekday", "weekend", "event_day"] {
        for d in decide(&f.bundle, &with_day(day)) {
            let mut got: Vec<String> = d.alts.iter().filter_map(|a| if let VisitKind::Competitor { id, .. } = &a.kind { Some(id.clone()) } else { None }).collect();
            got.sort();
            let mut want = ids.clone();
            want.sort();
            assert_eq!(got, want, "{day} 인물 {}", d.profile);
        }
    }
}
