//! E216: 경쟁 상권(C-216의 런타임 쪽)의 완료 기준 ①·② (③·④ 출발지별 정의·라벨은 Python 쪽 시험).
mod common;

use common::{fx, scenario};
use mdsim_core::bundle::{Bundle, Mode};
use mdsim_core::dims::Dim;
use mdsim_core::rng::KeyRng;
use mdsim_core::scenario::Scenario;
use mdsim_runtime::params::RtParams;
use mdsim_runtime::visit::{visit_decisions, VisitDecision, VisitKind};

fn params() -> RtParams {
    RtParams::load(&fx().defs.params).expect("params")
}

fn decide(b: &Bundle, scn: &Scenario) -> Vec<VisitDecision> {
    let f = fx();
    visit_decisions(&f.defs, &f.space, b, scn, &params(), &KeyRng::new(scn.seed))
}

fn wait_of(mode: Mode, scn: &Scenario, p: &RtParams) -> f64 {
    // 경쟁 상권의 수단별 대기(visit.rs: 지하철은 기준 배차 6분 × #9 ÷ 2)
    match mode {
        Mode::Subway => 6.0 * scn.headway_multiplier / 2.0,
        Mode::Bus => p.visit.bus_wait_min,
        Mode::Taxi => p.visit.taxi_wait_min,
        Mode::Walk => 0.0,
    }
}

/// 방문지 선택에는 정해진 경쟁 상권 대안이 모두 하나씩 있고, 그 값은 경쟁 상권 표(업종 매력·혼잡 수준)와
/// 그 출발지에서 가장 빠른 수단의 접근 시간에서 온다.
#[test]
fn e216_1_competitors_present() {
    let f = fx();
    let p = params();
    let scn = scenario("quick");
    let dec = decide(&f.bundle, &scn);
    let mut n = 0;
    for d in &dec {
        let prof = &f.bundle.profiles[d.profile as usize];
        for c in &f.bundle.competitors {
            let ks: Vec<usize> = d.alts.iter().enumerate().filter(|(_, a)| matches!(&a.kind, VisitKind::Competitor { id, .. } if *id == c.id)).map(|(k, _)| k).collect();
            assert_eq!(ks.len(), 1, "{} 경쟁 상권 {} 대안 수", prof.persona_id, c.id);
            let (rec, alt) = (&d.alts[ks[0]], &d.alt_specs[ks[0]]);
            let VisitKind::Competitor { mode, .. } = rec.kind else { unreachable!() };
            let rows: Vec<_> = f.bundle.access.iter().filter(|a| a.origin_id == prof.origin_id && a.destination == c.id).collect();
            let best = rows.iter().map(|a| a.base_min + wait_of(a.mode, &scn, &p)).fold(f64::INFINITY, f64::min);
            assert!((rec.base_min + rec.wait_min - best).abs() < 1e-9, "가장 빠른 수단({mode:?})");
            assert_eq!(alt.offer, c.interest, "업종별 매력 = 경쟁 상권 interest_<k>");
            assert_eq!(alt.demand[Dim::Crowd.ix()], c.crowd_level, "D-CROWD = crowd_level");
            assert_eq!(alt.dt_min, rec.base_min + rec.wait_min);
            n += 1;
        }
    }
    assert_eq!(n, dec.len() * f.bundle.competitors.len());
}

/// 한 경쟁 상권의 접근 시간이 늘면 그 상권을 고를 개인 확률은 늘지 않고 명동을 고를 확률은 줄지 않는다.
#[test]
fn e216_2_access_monotone() {
    let f = fx();
    let scn = scenario("quick");
    let pick = |d: &VisitDecision, id: &str| -> (f64, f64) {
        let pc = d.alts.iter().filter(|a| matches!(&a.kind, VisitKind::Competitor { id: x, .. } if x == id)).map(|a| a.prob).sum();
        let pm = d.alts.iter().filter(|a| matches!(a.kind, VisitKind::Myeongdong { .. })).map(|a| a.prob).sum();
        (pc, pm)
    };
    for c in &f.bundle.competitors {
        let mut prev: Option<Vec<(f64, f64)>> = None;
        for extra in [0.0, 5.0, 15.0, 30.0, 60.0, 120.0] {
            let mut b = f.bundle.clone();
            for a in b.access.iter_mut().filter(|a| a.destination == c.id) {
                a.base_min += extra;
            }
            let cur: Vec<(f64, f64)> = decide(&b, &scn).iter().map(|d| pick(d, &c.id)).collect();
            if let Some(prev) = &prev {
                for (i, ((c0, m0), (c1, m1))) in prev.iter().zip(&cur).enumerate() {
                    let who = &f.bundle.profiles[i].persona_id;
                    assert!(c1 <= &(c0 + 1e-12), "{who}: {} 접근 +{extra}분에 그 상권 확률 {c0} → {c1}", c.id);
                    assert!(m1 >= &(m0 - 1e-12), "{who}: {} 접근 +{extra}분에 명동 확률 {m0} → {m1}", c.id);
                }
            }
            prev = Some(cur);
        }
    }
}
