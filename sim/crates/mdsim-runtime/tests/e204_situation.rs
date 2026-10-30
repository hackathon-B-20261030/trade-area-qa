//! E204: 상황 구성(C-204의 런타임 쪽)의 완료 기준 ①·②·③·⑥.
//! (④ 시나리오 파일만으로 상황 만들기는 probe 쪽 시험이 맡는다.)
//!
//! 빠른 시나리오를 10분 돌린 상태의 실제 몸(위치·인지 집합·관찰 값)으로 맥락마다 상황을 만든다.
mod common;

use common::{ctx, fx, mid_quick, prof_of, same_choices, Mid};
use mdsim_core::defs::Aggregate;
use mdsim_core::dims::{Dim, ND};
use mdsim_core::scenario::DayType;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::perception::{look_situation, perceive};
use mdsim_runtime::situation::{aggregate, expected_dwell_min, next_situation, path_exposures, purchase_situation, rest_situation, route_situation, Action, AltTarget, Choice, Context, Ctx, Exposure};
use mdsim_runtime::space::{PlaceKind, Space};
use mdsim_runtime::state::{Body, Target};
use mdsim_runtime::visit::{visit_situation, VisitKind};
use std::collections::BTreeSet;

/// 몸 하나에 대해 다섯 맥락 + LOOK의 상황(VISIT은 인물 단위라 따로).
struct Situations {
    next: Vec<Choice>,
    route: Option<Vec<Choice>>,
    rest: Vec<Choice>,
    purchase: Vec<(u32, Vec<Choice>)>,
    look: Vec<Choice>,
}

fn situations(m: &Mid, c: &Ctx, coll: &Collective, b: &Body) -> Situations {
    let w = m.w();
    let prof = prof_of(w, b);
    let tick = m.state.tick;
    let u = w.rng.u("EXPLORE", &b.entity, tick, 0, 0);
    let next = next_situation(c, prof, b, None, None, u);
    let route = match b.target {
        Target::Place(p) => {
            let pl = &c.space.places[p as usize];
            Some(route_situation(c, b, pl.node_near, pl.anchor, b.target_purpose + b.choice_margin, pl.offer, b.fallback_u))
        }
        _ => None,
    };
    let rest = rest_situation(c, prof, b);
    let purchase = (0..c.space.places.len() as u32).filter(|&i| matches!(c.space.places[i as usize].kind, PlaceKind::Shop | PlaceKind::Visit | PlaceKind::Stall)).map(|i| (i, purchase_situation(c, b, i))).collect();
    let po = perceive(c.space, c.env, coll, &vec![0; c.space.places.len()], &w.p, prof, b, &w.rng, tick, c.dt);
    let rho = coll.local_density[b.id as usize];
    let look = look_situation(c.space, c.env, &w.p, &w.defs.dims, b, &po.visible, rho, tick, c.dt, prof.perc.head_turn_deg);
    Situations { next, route, rest, purchase, look }
}

fn actions(v: &[Choice]) -> BTreeSet<&'static str> {
    v.iter().map(|c| c.action.name()).collect()
}

fn visit_actions(kinds: &[VisitKind]) -> BTreeSet<&'static str> {
    kinds
        .iter()
        .map(|k| match k {
            VisitKind::Myeongdong { .. } => Action::VisitNow.name(),
            VisitKind::Competitor { .. } => Action::GoElsewhere.name(),
            VisitKind::Skip => Action::Skip.name(),
        })
        .collect()
}

fn coll_of(m: &Mid, space: &Space) -> Collective {
    Collective::build(&m.state.bodies, space, &m.state.env, m.w().p.crowd.local_radius_m)
}

/// 같은 질의(위치·시각·맥락·목적·몸 상태)는 같은 상황을 만든다. 반복 호출과, 같은 입력으로 다시 지은 공간에서도 같다.
#[test]
fn e204_1_same_query_same_situation() {
    let m = mid_quick();
    let w = m.w();
    let f = fx();
    let env = &m.state.env;
    let c1 = ctx(w, env, m.state.tick);
    let space2 = Space::build(&f.defs, &f.bundle).expect("space");
    let c2 = Ctx { space: &space2, ..ctx(w, env, m.state.tick) };
    let (coll1, coll2) = (coll_of(m, w.space), coll_of(m, &space2));
    let bodies = m.street_domestic();
    assert!(bodies.len() >= 10, "표본 몸 {}", bodies.len());
    for b in bodies {
        let a = situations(m, &c1, &coll1, b);
        for (other, coll) in [(&c1, &coll1), (&c2, &coll2)] {
            let x = situations(m, other, coll, b);
            assert!(same_choices(&a.next, &x.next), "NEXT 몸 {}", b.id);
            assert_eq!(a.route.is_some(), x.route.is_some());
            if let (Some(r1), Some(r2)) = (&a.route, &x.route) {
                assert!(same_choices(r1, r2), "ROUTE 몸 {}", b.id);
            }
            assert!(same_choices(&a.rest, &x.rest), "REST 몸 {}", b.id);
            for ((p1, s1), (p2, s2)) in a.purchase.iter().zip(&x.purchase) {
                assert_eq!(p1, p2);
                assert!(same_choices(s1, s2), "PURCHASE 몸 {} 장소 {p1}", b.id);
            }
            assert!(same_choices(&a.look, &x.look), "LOOK 몸 {}", b.id);
        }
    }
    // VISIT: 인물마다 같은 질의는 같은 대안·기록
    for prof in f.bundle.profiles.iter().step_by(7) {
        let (a1, r1) = visit_situation(w.space, w.bundle, w.scn, &w.p, prof);
        let (a2, r2) = visit_situation(&space2, w.bundle, w.scn, &w.p, prof);
        assert_eq!((a1, r1), (a2, r2), "VISIT {}", prof.persona_id);
    }
}

/// 노출 형식: 대안의 요구는 C-203 인스턴스(간선·장소)의 요구와 노출량(양·시간 가중)을 차원이 정한 규칙
/// (합·최댓값·시간 가중 평균)으로 모은 것이다.
#[test]
fn e204_2_exposure_form() {
    let m = mid_quick();
    let w = m.w();
    let dims = &w.defs.dims;
    // (1) 집계 규칙: 손으로 계산한 값과 같다
    let mut a = [0.0; ND];
    let mut b = [0.0; ND];
    let mut z = [0.0; ND];
    for d in Dim::ALL {
        a[d.ix()] = 1.0 + d.ix() as f64;
        b[d.ix()] = 10.0 - d.ix() as f64;
        z[d.ix()] = 100.0;
    }
    let ex = [Exposure { demand: a, amount: 2.0, weight: 3.0 }, Exposure { demand: b, amount: 0.5, weight: 1.0 }, Exposure { demand: z, amount: 0.0, weight: 0.0 }];
    let agg = aggregate(&ex, dims);
    for d in Dim::ALL {
        let i = d.ix();
        let want = match dims[i].aggregate {
            Aggregate::Sum => 2.0 * a[i] + 0.5 * b[i],
            Aggregate::Max => a[i].max(b[i]), // 양 0인 노출은 최댓값에서 빠진다
            Aggregate::TimeWeightedMean => (3.0 * a[i] + 1.0 * b[i]) / 4.0,
        };
        assert!((agg[i] - want).abs() < 1e-12, "{}: {} vs {}", d.id(), agg[i], want);
    }
    assert_eq!(aggregate(&[], dims), [0.0; ND], "노출이 없으면 0");
    // (2) 실제 상황: 대안 요구 = 경로 간선 인스턴스(+ 장소 인스턴스)의 노출을 모은 값
    let env = &m.state.env;
    let c = ctx(w, env, m.state.tick);
    let coll = coll_of(m, w.space);
    let mut n_checked = 0;
    for body in m.street_domestic() {
        let s = situations(m, &c, &coll, body);
        let mut all: Vec<&Choice> = s.next.iter().collect();
        if let Some(r) = &s.route {
            all.extend(r.iter());
        }
        for ch in all {
            let Some(path) = &ch.path else { continue };
            let (ex, _, _) = path_exposures(&c, body, path, 1.0);
            for (e, &(edge, fwd)) in ex.iter().zip(&path.edges) {
                let es = &w.space.edges[edge as usize];
                let inst = es.demand_of(fwd, env);
                for d in Dim::ALL {
                    if d == Dim::Crowd {
                        assert_eq!(e.demand[d.ix()], c.believed_density(body, edge), "D-CROWD는 본 밀도 또는 사전 기대");
                    } else {
                        assert_eq!(e.demand[d.ix()], inst[d.ix()], "간선 {} {}", es.id, d.id());
                    }
                }
                assert_eq!(e.amount, 1.0, "간선 하나를 다 지난다");
            }
            let want = match (ch.action, &ch.target) {
                (Action::GoTo | Action::Watch, AltTarget::Place(p)) => {
                    let pl = &w.space.places[*p as usize];
                    let mut ex = ex.clone();
                    let mut d = pl.demand;
                    d[Dim::Money.ix()] = 0.0;
                    d[Dim::Time.ix()] = 0.0;
                    ex.push(Exposure { demand: d, amount: 1.0, weight: expected_dwell_min(&w.p, pl.kind) * 60.0 });
                    Some(aggregate(&ex, &w.defs.dims))
                }
                (Action::Leave | Action::Explore, _) => Some(aggregate(&ex, &w.defs.dims)),
                (Action::Proceed | Action::Detour | Action::Defer, _) => {
                    let Target::Place(p) = body.target else { unreachable!() };
                    let pl = &w.space.places[p as usize];
                    let mut d = aggregate(&ex, &w.defs.dims);
                    d[Dim::Walk.ix()] += pl.anchor.dist(w.space.graph.node_pos[pl.node_near as usize]);
                    if ch.action == Action::Defer {
                        // 기다린 뒤의 혼잡은 지금 본 값 대신 사전 기대를 넘지 않는다(S-E36)
                        d[Dim::Crowd.ix()] = d[Dim::Crowd.ix()].min(c.prior_density);
                    }
                    Some(d)
                }
                _ => None,
            };
            if let Some(want) = want {
                assert_eq!(ch.alt.demand, want, "{} {:?}", ch.action.name(), ch.target);
                n_checked += 1;
            }
        }
    }
    assert!(n_checked > 100, "검사한 대안 {n_checked}");
}

/// 맥락별 필수 대안이 항상 있다(C-204 ③: 진행·감속·연기·포기, 떠나기, 구매 안 함, 안 감 + LOOK의 앞길 보기).
/// 필수 목록은 정의 파일(contexts.yaml)의 `always`이고, 엔진의 `Context::required()`가 그것을 모두 담아야 한다.
#[test]
fn e204_3_required_alts() {
    let m = mid_quick();
    let w = m.w();
    let f = fx();
    for ctx_ in [Context::Visit, Context::Next, Context::Route, Context::Rest, Context::Purchase, Context::Look] {
        let req: BTreeSet<&str> = ctx_.required().iter().map(|a| a.name()).collect();
        for a in &w.defs.contexts[ctx_.name()].always {
            assert!(req.contains(a.as_str()), "{}: 정의의 always {a}가 엔진 필수 목록 {req:?}에 없음", ctx_.name());
        }
    }
    let has = |set: &BTreeSet<&'static str>, ctx_: Context| ctx_.required().iter().all(|a| set.contains(a.name()));
    let env = &m.state.env;
    let c = ctx(w, env, m.state.tick);
    let coll = coll_of(m, w.space);
    let mut n_route = 0;
    for b in m.street_domestic() {
        let s = situations(m, &c, &coll, b);
        assert!(has(&actions(&s.next), Context::Next), "NEXT 몸 {}: {:?}", b.id, actions(&s.next));
        if let Some(r) = &s.route {
            assert!(has(&actions(r), Context::Route), "ROUTE 몸 {}: {:?}", b.id, actions(r));
            n_route += 1;
        }
        assert!(has(&actions(&s.rest), Context::Rest), "REST 몸 {}", b.id);
        for (p, ch) in &s.purchase {
            assert!(has(&actions(ch), Context::Purchase), "PURCHASE 장소 {p}");
        }
        assert!(has(&actions(&s.look), Context::Look), "LOOK 몸 {}", b.id);
    }
    assert!(n_route > 0);
    for day in ["weekday", "weekend", "event_day"] {
        let mut scn = w.scn.clone();
        scn.day_type = day.into();
        for prof in &f.bundle.profiles {
            let (_, recs) = visit_situation(w.space, w.bundle, &scn, &w.p, prof);
            let kinds: Vec<VisitKind> = recs.iter().map(|r| r.kind.clone()).collect();
            assert!(has(&visit_actions(&kinds), Context::Visit), "VISIT {} {day}", prof.persona_id);
        }
    }
}

/// 다섯 결정 맥락(+LOOK)마다 대안 생성 규칙이 있고(02d I-204-r 표의 행동과 엔진이 더한 ROUTE의 DETOUR·DEFER(S-E36)만 나온다), 행선지 대안은 고려 집합 안에서
/// 영업 중이라고 믿는 장소로만 만든다. 영업 종료를 본 장소와 고려 집합 밖 장소는 빠진다.
/// VISIT 슬롯은 그 인물의 그날 가용 시간 창과 실행 창 안에만 있다.
#[test]
fn e204_6_contexts_and_exclusions() {
    let m = mid_quick();
    let w = m.w();
    let f = fx();
    let env = &m.state.env;
    let c = ctx(w, env, m.state.tick);
    let coll = coll_of(m, w.space);
    let vocab: [(&str, &[&str]); 5] = [
        ("NEXT", &["GO_TO", "WATCH", "EXPLORE", "LEAVE", "REST_SEAT"]),
        ("ROUTE", &["PROCEED", "DETOUR", "PACE", "DEFER", "ABANDON"]),
        ("REST", &["REST_SEAT", "REST_PAID", "CONTINUE", "LEAVE"]),
        ("PURCHASE", &["NO_BUY", "BUY"]),
        ("LOOK", &["PATH", "FOOTING", "LOOK_AT"]),
    ];
    let within = |ctx_: &str, set: &BTreeSet<&'static str>| {
        let allowed = vocab.iter().find(|v| v.0 == ctx_).unwrap().1;
        assert!(!set.is_empty(), "{ctx_} 대안 없음");
        assert!(set.iter().all(|a| allowed.contains(a)), "{ctx_}: {set:?}");
    };
    let mut excluded_closed = 0;
    let mut excluded_unaware = 0;
    let (mut n_seat, mut n_paid) = (0, 0);
    for b in m.street_domestic() {
        let prof = prof_of(w, b);
        let s = situations(m, &c, &coll, b);
        within("NEXT", &actions(&s.next));
        if let Some(r) = &s.route {
            within("ROUTE", &actions(r));
        }
        within("REST", &actions(&s.rest));
        within("LOOK", &actions(&s.look));
        for (p, ch) in &s.purchase {
            within("PURCHASE", &actions(ch));
            let pl = &w.space.places[*p as usize];
            let n_buy = ch.iter().filter(|x| x.action == Action::Buy).count();
            let main = (0..pl.offer.len()).max_by(|&i, &j| pl.offer[i].total_cmp(&pl.offer[j]).then(j.cmp(&i))).unwrap();
            let want = if !pl.can_buy || pl.price <= 0.0 { 0 } else if main == 0 { 1 } else { w.p.purchase.band_mult.len() };
            assert_eq!(n_buy, want, "장소 {} 구매 대안 수", pl.id);
        }
        // 행선지 대안 ⊆ 고려 집합 ∩ 영업 중이라고 믿는 장소
        let places: Vec<u32> = s.next.iter().filter_map(|ch| if let AltTarget::Place(p) = ch.target { Some(p) } else { None }).collect();
        for &p in &places {
            let a = b.aware_of(p).unwrap_or_else(|| panic!("몸 {}: 고려 집합 밖 장소 {p}", b.id));
            assert_ne!(a.seen_open, Some(false), "닫힌 것을 본 장소 {p}");
        }
        // 고려 집합의 한 장소를 닫힌 것으로 본 몸, 고려 집합에서 뺀 몸
        let u = w.rng.u("EXPLORE", &b.entity, m.state.tick, 0, 0);
        if let Some(&p) = places.iter().find(|&&p| w.space.places[p as usize].kind != PlaceKind::Bench) {
            let mut closed = b.clone();
            let a = closed.aware.iter_mut().find(|a| a.place == p).unwrap();
            a.seen_open = Some(false);
            a.last_seen_tick = Some(m.state.tick);
            let s2 = next_situation(&c, prof, &closed, None, None, u);
            assert!(!s2.iter().any(|ch| ch.target == AltTarget::Place(p)), "영업 종료를 본 장소가 대안에 남음");
            excluded_closed += 1;
            let mut unaware = b.clone();
            unaware.aware.retain(|a| a.place != p);
            let s3 = next_situation(&c, prof, &unaware, None, None, u);
            assert!(!s3.iter().any(|ch| ch.target == AltTarget::Place(p)), "고려 집합 밖 장소가 대안에 남음");
            excluded_unaware += 1;
        }
        // 쉬기(S-E53): 아는 무료 쉼터(REST_SEAT 허용)가 rest.max_bench_m 안에 있을 때만 앉아 쉬기, 열려 있다고 믿는
        // 유료 쉼터(REST_PAID만 허용, 카페)가 있을 때만 카페에서 쉬기. 대안의 대상은 그 행동을 허용하는 장소이고 유료면 돈이 든다.
        let near = |paid: bool| {
            b.aware.iter().any(|a| {
                let pl = &w.space.places[a.place as usize];
                let ok = if paid { pl.rest_paid && !pl.rest_seat && mdsim_runtime::situation::believed_open(b, a.place) } else { pl.rest_seat };
                ok && pl.anchor.dist(b.pos) <= w.p.rest.max_bench_m
            })
        };
        assert_eq!(actions(&s.rest).contains("REST_SEAT"), near(false), "몸 {} 벤치", b.id);
        assert_eq!(actions(&s.rest).contains("REST_PAID"), near(true), "몸 {} 유료 쉼터", b.id);
        for ch in &s.rest {
            let AltTarget::Place(p) = ch.target else { continue };
            let pl = &w.space.places[p as usize];
            match ch.action {
                Action::RestSeat => {
                    assert!(pl.rest_seat && ch.alt.cost_krw == 0.0, "REST_SEAT 대상 {}", pl.id);
                    n_seat += 1;
                }
                Action::RestPaid => {
                    assert!(pl.rest_paid && !pl.rest_seat, "REST_PAID 대상 {}", pl.id);
                    assert!(ch.alt.cost_krw > 0.0 || pl.price <= 0.0, "유료 쉬기 {}의 비용", pl.id);
                    n_paid += 1;
                }
                _ => {}
            }
        }
    }
    assert!(excluded_closed > 5 && excluded_unaware > 5);
    // 벤치 곁에 둔 몸(모든 장소를 앎): 무료 쉬기 대안이 그 벤치를 가리키고 돈이 들지 않는다
    let bench = w.space.places.iter().position(|p| p.rest_seat && p.kind == PlaceKind::Bench).expect("벤치") as u32;
    let mut nb = common::quiet_walker(m);
    nb.pos = w.space.places[bench as usize].anchor;
    nb.cur_edge = Some(w.space.places[bench as usize].edge);
    if nb.aware_of(bench).is_none() {
        let i = nb.aware.partition_point(|a| a.place < bench);
        nb.aware.insert(i, mdsim_runtime::state::Aware { place: bench, first_tick: 0, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: true });
    }
    let rs = rest_situation(&c, prof_of(w, &nb), &nb);
    let seat = rs.iter().find(|ch| ch.action == Action::RestSeat).expect("벤치 곁의 무료 쉬기");
    assert_eq!((seat.target.clone(), seat.alt.cost_krw), (AltTarget::Place(bench), 0.0));
    n_seat += 1;
    assert!(n_seat > 0 && n_paid > 0, "쉬기 대안 무료 {n_seat}, 유료 {n_paid}");
    // VISIT: 슬롯 × 수단은 가용 창 ∩ 실행 창, 경쟁 상권마다 하나, 안 감 하나
    let (w0, w1) = w.scn.window_min().unwrap();
    for prof in &f.bundle.profiles {
        let (alts, recs) = visit_situation(w.space, w.bundle, w.scn, &w.p, prof);
        assert_eq!(alts.len(), recs.len());
        let day = w.scn.day();
        for r in &recs {
            if let VisitKind::Myeongdong { slot_start_min, .. } = r.kind {
                let inside = prof.availability.iter().filter(|a| a.day_type == day).any(|a| slot_start_min >= a.start_min.max(w0) - 1e-9 && slot_start_min < a.end_min.min(w1));
                assert!(inside, "{} 슬롯 {slot_start_min}이 가용·실행 창 밖", prof.persona_id);
            }
        }
        assert_eq!(recs.iter().filter(|r| matches!(r.kind, VisitKind::Skip)).count(), 1);
        assert_eq!(recs.iter().filter(|r| matches!(r.kind, VisitKind::Competitor { .. })).count(), w.bundle.competitors.len());
    }
    // 그날 가용 창이 실행 창과 겹치지 않는 인물에게는 명동 대안이 없다
    let mut late = f.bundle.profiles[0].clone();
    late.availability.retain(|a| a.day_type != DayType::Weekend);
    late.availability.push(mdsim_core::bundle::Avail { day_type: DayType::Weekend, start_min: 18.0 * 60.0, end_min: 20.0 * 60.0 });
    let (_, recs) = visit_situation(w.space, w.bundle, w.scn, &w.p, &late);
    assert!(!recs.iter().any(|r| matches!(r.kind, VisitKind::Myeongdong { .. })), "창 밖 인물의 명동 대안");
}

/// 엔진이 만드는 행동은 정의 파일(contexts.yaml) 그 맥락의 `actions` 안에 있다(DR-215: 정의 한 벌 공유).
#[test]
fn e204_6_actions_within_definitions() {
    let m = mid_quick();
    let w = m.w();
    let env = &m.state.env;
    let c = ctx(w, env, m.state.tick);
    let coll = coll_of(m, w.space);
    let mut extra = BTreeSet::new();
    for b in m.street_domestic() {
        let s = situations(m, &c, &coll, b);
        let mut check = |ctx_: &'static str, set: BTreeSet<&'static str>| {
            for a in set {
                if !w.defs.contexts[ctx_].actions.iter().any(|x| x == a) {
                    extra.insert(format!("{ctx_}:{a}"));
                }
            }
        };
        check("NEXT", actions(&s.next));
        if let Some(r) = &s.route {
            check("ROUTE", actions(r));
        }
        check("REST", actions(&s.rest));
        for (_, ch) in &s.purchase {
            check("PURCHASE", actions(ch));
        }
        check("LOOK", actions(&s.look));
    }
    assert!(extra.is_empty(), "정의에 없는 행동: {extra:?}");
}
