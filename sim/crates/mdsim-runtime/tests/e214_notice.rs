//! E214: 인지와 주목(C-214)의 완료 기준 ①~⑥.
mod common;

use common::{ctx, fx, lone, lone_coll, mid_quick, observed_quick, prof_of, sample_points, Mid};
use mdsim_core::dims::Dim;
use mdsim_core::geom::angle_between;
use mdsim_core::rng::KeyRng;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::perception::{look_situation, notice_pass_prob, notice_tick_prob, perceive};
use mdsim_runtime::situation::{next_situation, Action, AltTarget};
use mdsim_runtime::space::Space;
use mdsim_runtime::state::{Aware, Body};
use std::collections::{BTreeMap, BTreeSet};

fn street_bodies(st: &mdsim_runtime::step::SimState) -> Vec<&Body> {
    st.bodies.iter().filter(|b| b.alive && !b.is_background() && b.activity.on_street()).collect()
}

/// 행선지 대안은 고려 집합(인지 집합) 안에서만 생긴다(떠나기 제외). 실행 기록의 모든 NEXT 결정에서 장소 대안은
/// 그 결정 틱에 이미 인지 집합에 있던 장소다(인지 집합은 방문 동안 줄지 않으므로 끝 상태의 first_tick으로 본다).
#[test]
fn e214_1_next_within_consideration() {
    let m = mid_quick();
    let w = m.w();
    let c = ctx(w, &m.state.env, m.state.tick);
    let mut n_unit = 0;
    for b in street_bodies(&m.state) {
        let u = w.rng.u("EXPLORE", &b.entity, m.state.tick, 0, 0);
        for ch in next_situation(&c, prof_of(w, b), b, None, None, u) {
            match ch.target {
                AltTarget::Place(p) => {
                    assert!(b.aware_of(p).is_some(), "몸 {}: 고려 집합 밖 장소 {p}", b.id);
                    n_unit += 1;
                }
                AltTarget::Portal(_) => assert_eq!(ch.action, Action::Leave),
                AltTarget::Node(_) => assert_eq!(ch.action, Action::Explore, "장소가 아닌 교차점 둘러보기"),
                ref t => panic!("NEXT의 예상 밖 대상 {t:?}"),
            }
        }
    }
    assert!(n_unit > 50);
    let o = observed_quick();
    let space = o.prep.w.space;
    let alts_of: BTreeMap<u64, Vec<&mdsim_runtime::record::AltRow>> = o.sink.alts.iter().fold(BTreeMap::new(), |mut m, a| {
        m.entry(a.decision).or_insert_with(Vec::new).push(a);
        m
    });
    let mut n_run = 0;
    for d in o.sink.decisions.iter().filter(|d| d.context == "NEXT") {
        let b = &o.state.bodies[d.body as usize];
        for a in &alts_of[&d.id] {
            let Some(id) = a.target.strip_prefix("place:") else { continue };
            let p = space.place_ix(id).expect("장소 ID") as u32;
            let aw = b.aware_of(p).unwrap_or_else(|| panic!("결정 {}: 몸 {}의 고려 집합 밖 장소 {id}", d.id, b.id));
            assert!(aw.first_tick <= d.tick, "결정 {}: 장소 {id}를 결정 뒤에야 알았음", d.id);
            n_run += 1;
        }
    }
    assert!(n_run > 200, "실행 기록의 장소 대안 {n_run}");
}

/// 시계성이나 광고 자극도가 오르면 개인의 알아차림 확률은 내려가지 않는다. 같은 키로 지각을 돌리면
/// 시계성(또는 장소 앞 광고 자극도)을 올린 세계에서 알아차린 것은 원래 세계에서 알아차린 것을 포함한다.
#[test]
fn e214_2_notice_monotone() {
    let m = mid_quick();
    let w = m.w();
    let np = &w.p.notice;
    for att in [0.0, 0.5, 1.0] {
        for rho in [0.0, 0.5, 2.0] {
            for i in 0..10 {
                let base = notice_pass_prob(np, i as f64 / 10.0, 0.3, att, rho);
                assert!(notice_pass_prob(np, (i + 1) as f64 / 10.0, 0.3, att, rho) >= base);
                assert!(notice_pass_prob(np, i as f64 / 10.0, 0.4, att, rho) >= base);
            }
        }
    }
    // 같은 키의 결합: 시계성·광고를 올리면 매 틱 알아차림 집합이 커지기만 한다(보행 공간 곳곳에 혼자 선 몸)
    let mut brighter: Space = w.space.clone();
    for pl in brighter.places.iter_mut() {
        pl.visibility = (pl.visibility + 0.3).min(1.0);
        pl.ad = (pl.ad + 0.3).min(1.0);
    }
    let (mut n_lo, mut n_hi) = (0, 0);
    for (b, coll) in placements(m) {
        let prof = prof_of(w, &b);
        let ql = vec![0; w.space.places.len()];
        for t in 0..10 {
            let tick = m.state.tick + t;
            let lo = perceive(w.space, &m.state.env, &coll, &ql, &w.p, prof, &b, &w.rng, tick, w.dt());
            let hi = perceive(&brighter, &m.state.env, &coll, &ql, &w.p, prof, &b, &w.rng, tick, w.dt());
            assert_eq!(lo.visible, hi.visible, "가시 집합은 기하만으로 정해진다");
            let hs: BTreeSet<u32> = hi.notices.iter().copied().collect();
            assert!(lo.notices.iter().all(|p| hs.contains(p)), "틱 {tick}: 더 잘 보이는 세계에서 알아차림이 빠짐");
            n_lo += lo.notices.len();
            n_hi += hs.len();
        }
    }
    assert!(n_hi > n_lo && n_lo > 20, "알아차림 {n_lo} → {n_hi}");
}

/// 보행 공간의 표본 위치마다 네 방향으로 혼자 선 몸(아무 장소도 모름)과 그 집단 상태.
fn placements(m: &Mid) -> Vec<(Body, Collective)> {
    let mut out = Vec::new();
    for (k, &pos) in sample_points(m.w().space).iter().enumerate().step_by(2) {
        for h in 0..4 {
            let heading = h as f64 * std::f64::consts::FRAC_PI_2 + 0.3 * (k % 3) as f64;
            let b = lone(m, pos, heading, heading);
            let c = lone_coll(m, &b);
            out.push((b, c));
        }
    }
    out
}

/// 광고 자극도가 오르면 그 구간의 D-SENSE 요구가 내려가지 않는다: 간선 인스턴스의 요구(클래스 식), 그 간선을 지나는
/// 행선지 대안의 모은 요구, 가게 정면을 보는 LOOK_AT의 요구 모두.
#[test]
fn e214_3_sense_monotone() {
    let f = fx();
    let m = mid_quick();
    let w = m.w();
    let build = |k: f64| -> Space {
        let mut b = f.bundle.clone();
        for e in b.edges.iter_mut() {
            let ad = (e.ad_intensity + k).clamp(0.0, 1.0);
            e.ad_intensity = ad;
            e.attrs.insert("ad_intensity".into(), ad);
        }
        Space::build(&f.defs, &b).expect("space")
    };
    let spaces: Vec<Space> = [-0.3, 0.0, 0.2, 0.5].iter().map(|&k| build(k)).collect();
    for pair in spaces.windows(2) {
        for (a, b) in pair[0].edges.iter().zip(&pair[1].edges) {
            assert!(b.ad >= a.ad);
            for dir in 0..2 {
                for veh in 0..2 {
                    assert!(b.demand[dir][veh][Dim::Sense.ix()] >= a.demand[dir][veh][Dim::Sense.ix()], "간선 {} D-SENSE", a.id);
                }
            }
        }
    }
    let st = &m.state;
    let mut n = 0;
    for b in street_bodies(st) {
        let prof = prof_of(w, b);
        let u = w.rng.u("EXPLORE", &b.entity, st.tick, 0, 0);
        let sits: Vec<_> = spaces
            .iter()
            .map(|s| {
                let c = mdsim_runtime::situation::Ctx { space: s, ..ctx(w, &st.env, st.tick) };
                next_situation(&c, prof, b, None, None, u)
            })
            .collect();
        for pair in sits.windows(2) {
            for (x, y) in pair[0].iter().zip(&pair[1]) {
                assert_eq!((x.action, &x.target), (y.action, &y.target), "같은 대안 순서");
                assert!(y.alt.demand[Dim::Sense.ix()] >= x.alt.demand[Dim::Sense.ix()] - 1e-12, "몸 {} {:?} 대안 D-SENSE", b.id, x.target);
                n += 1;
            }
        }
        // LOOK_AT: 보이는 가게를 아는 것으로 두고 정면 자극을 본다
        let target = (0..w.space.places.len() as u32).find(|&p| w.space.places[p as usize].kind == mdsim_runtime::space::PlaceKind::Shop).unwrap();
        let mut bb = b.clone();
        if bb.aware_of(target).is_none() {
            let i = bb.aware.partition_point(|a| a.place < target);
            bb.aware.insert(i, Aware { place: target, first_tick: 0, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: true });
        }
        let senses: Vec<f64> = spaces
            .iter()
            .map(|s| {
                let ch = look_situation(s, &st.env, &w.p, &w.defs.dims, &bb, &[(target, true)], 0.2, st.tick, w.dt(), 180.0);
                ch.iter().find(|c| c.target == AltTarget::Place(target)).unwrap().alt.demand[Dim::Sense.ix()]
            })
            .collect();
        assert!(senses.windows(2).all(|x| x[1] >= x[0]), "LOOK_AT D-SENSE {senses:?}");
    }
    assert!(n > 100, "비교한 대안 {n}");
}

/// 알아차림은 키드 난수(NOTICE·몸·틱·장소)로만 정해진다: 지각 함수의 결과가 `u(NOTICE, 몸, 틱, 장소) < q`와 같고,
/// 같은 입력은 같은 결과, 시드를 바꾸면 가시 집합은 그대로이고 결과만 달라진다. 실행 중 알아차림은 모두 사건으로 남는다.
/// (02d §1.4는 slot을 계약 place_id로 적었지만 장소 ID가 문자열이므로 엔진은 조밀한 장소 인덱스를 쓴다.)
#[test]
fn e214_4_notice_keys_logged() {
    let m = mid_quick();
    let w = m.w();
    let np = &w.p.notice;
    let other = KeyRng::new(w.rng.seed + 1);
    let ql = vec![0; w.space.places.len()];
    let (mut n_draws, mut n_hit, mut n_diff) = (0, 0, 0);
    for (b, coll) in placements(m) {
        let prof = prof_of(w, &b);
        let rho = coll.local_density[0];
        for t in 0..5 {
            let tick = m.state.tick + t;
            let po = perceive(w.space, &m.state.env, &coll, &ql, &w.p, prof, &b, &w.rng, tick, w.dt());
            let again = perceive(w.space, &m.state.env, &coll, &ql, &w.p, prof, &b, &w.rng, tick, w.dt());
            assert_eq!((&po.visible, &po.notices), (&again.visible, &again.notices));
            let mut want = Vec::new();
            for &(i, focal) in &po.visible {
                let pl = &w.space.places[i as usize];
                let q = notice_tick_prob(np, notice_pass_prob(np, pl.visibility, pl.ad, prof.attention, rho), if focal { 1.0 } else { np.w_peripheral }, w.dt());
                if w.rng.u("NOTICE", &b.entity, tick, i as u64, 0) < q {
                    want.push(i);
                }
                n_draws += 1;
            }
            assert_eq!(po.notices, want, "틱 {tick}: 알아차림 = u(NOTICE, 몸, 틱, 장소) < q");
            n_hit += want.len();
            let alt = perceive(w.space, &m.state.env, &coll, &ql, &w.p, prof, &b, &other, tick, w.dt());
            assert_eq!(alt.visible, po.visible);
            n_diff += usize::from(alt.notices != po.notices);
        }
    }
    assert!(n_draws > 1000 && n_hit > 20 && n_diff > 0, "판정 {n_draws}, 알아차림 {n_hit}, 시드에 따라 달라진 경우 {n_diff}");
    // 이미 아는 장소는 다시 판정하지 않는다
    let b = {
        let (mut b, _) = placements(m).into_iter().find(|(b, c)| !perceive(w.space, &m.state.env, c, &ql, &w.p, prof_of(w, b), b, &w.rng, m.state.tick, w.dt()).visible.is_empty()).unwrap();
        let vis = perceive(w.space, &m.state.env, &lone_coll(m, &b), &ql, &w.p, prof_of(w, &b), &b, &w.rng, m.state.tick, w.dt()).visible;
        for (i, _) in vis {
            b.aware.push(Aware { place: i, first_tick: 0, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: true });
        }
        b.aware.sort_by_key(|a| a.place);
        b
    };
    for t in 0..50 {
        assert!(perceive(w.space, &m.state.env, &lone_coll(m, &b), &ql, &w.p, prof_of(w, &b), &b, &w.rng, m.state.tick + t, w.dt()).notices.is_empty());
    }
    // 실행: 알아차림 사건 ↔ 인지 집합(노출로 들어온 항목) 1:1, 몸 통계와 같다
    let o = observed_quick();
    let mut ev: BTreeMap<(u32, u32), u64> = BTreeMap::new();
    for e in o.sink.events.iter().filter(|e| e.kind == "notice") {
        let key = (e.body as u32, e.place.expect("알아차림 사건의 장소"));
        assert!(ev.insert(key, e.tick).is_none(), "같은 장소를 두 번 알아차림 {key:?}");
    }
    let mut n_aw = 0;
    for b in &o.state.bodies {
        let mine = ev.iter().filter(|((bid, _), _)| *bid == b.id).count();
        assert_eq!(b.stats.notices as usize, mine, "몸 {} 알아차림 수", b.id);
        for a in b.aware.iter().filter(|a| !a.known_before) {
            assert_eq!(ev.get(&(b.id, a.place)), Some(&a.first_tick), "몸 {} 장소 {}: 사건 없이 인지 집합에 들어옴", b.id, a.place);
            n_aw += 1;
        }
    }
    assert_eq!(n_aw, ev.len());
    assert!(n_aw > 100);
}

/// 목적·익숙함으로 이미 아는 장소는 노출 없이도 처음부터 고려 집합에 있다(목적 장소는 모두, 도착 틱에, 알아차림 사건 없이).
/// 도착 직후 첫 NEXT 결정은 그 장소들을 대안으로 볼 수 있다.
#[test]
fn e214_5_known_places() {
    let o = observed_quick();
    let w = o.prep.w;
    let notices: BTreeSet<(u32, u32)> = o.sink.events.iter().filter(|e| e.kind == "notice").map(|e| (e.body as u32, e.place.unwrap())).collect();
    let mut n_targets = 0;
    for b in o.state.bodies.iter().filter(|b| !b.is_background()) {
        let prof = &w.bundle.profiles[b.profile.unwrap() as usize];
        for pu in &prof.purposes {
            let Some(p) = pu.target_place_id.as_deref().and_then(|id| w.space.place_ix(id)) else { continue };
            let a = b.aware_of(p as u32).unwrap_or_else(|| panic!("몸 {}: 목적 장소 {} 모름", b.id, w.space.places[p].id));
            assert!(a.known_before && a.first_tick == b.arrival_tick, "몸 {}: 목적 장소는 도착할 때부터 안다", b.id);
            n_targets += 1;
        }
        for a in b.aware.iter().filter(|a| a.known_before) {
            assert_eq!(a.first_tick, b.arrival_tick);
            assert!(!notices.contains(&(b.id, a.place)), "이미 아는 장소에 알아차림 사건");
        }
    }
    assert!(n_targets > 20, "목적 장소 {n_targets}");
    // 도착 계기의 첫 NEXT: 이미 아는 목적 장소(영업 중)가 대안에 있다
    let mut seen_in_first = 0;
    for d in o.sink.decisions.iter().filter(|d| d.context == "NEXT" && d.trigger == "도착") {
        let b = &o.state.bodies[d.body as usize];
        assert_eq!(d.tick, b.arrival_tick, "도착 계기 결정은 도착 틱");
        let prof = &w.bundle.profiles[b.profile.unwrap() as usize];
        let env = mdsim_runtime::env::env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(d.tick));
        let targets: BTreeSet<String> = o.sink.alts.iter().filter(|a| a.decision == d.id).map(|a| a.target.clone()).collect();
        for pu in &prof.purposes {
            let Some(p) = pu.target_place_id.as_deref().and_then(|id| w.space.place_ix(id)) else { continue };
            if env.open[p] && w.space.places[p].kind != mdsim_runtime::space::PlaceKind::Bench {
                assert!(targets.contains(&format!("place:{}", w.space.places[p].id)), "몸 {} 첫 결정에 목적 장소 {} 없음", b.id, w.space.places[p].id);
                seen_in_first += 1;
            }
        }
    }
    assert!(seen_in_first > 10);
}

/// 시야 밖(거리·각 밖이거나 건물에 가린) 상점은 알아차리지 않는다: 실행의 모든 알아차림 사건은 그 틱을 시작할 때
/// 몸의 자리·진행 방향·시선에서 시야(감쇠 전 최대 시야로 넉넉히 본다) 안이고 가리지 않은 장소다.
/// 시선이 중립인 보통 통과(초점 안 T_ref초)의 누적 위험률은 알아차림 함수 값과 같다.
#[test]
fn e214_6_out_of_view_and_calibration() {
    let o = observed_quick();
    let w = o.prep.w;
    let mut n = 0;
    let mut at_spawn = 0;
    for e in o.sink.events.iter().filter(|e| e.kind == "notice") {
        // 도착한 틱의 지각은 ② 도착 뒤의 자리(출입구 흩기)에서 보므로 step 전 스냅샷에 없다. 그 기하는 e221_1이 본다.
        let Some(s) = o.snaps[e.tick as usize].iter().find(|s| s.id == e.body as u32) else {
            assert_eq!(o.state.bodies[e.body as usize].arrival_tick, e.tick, "스냅샷에 없는 몸은 그 틱에 도착한 몸");
            at_spawn += 1;
            continue;
        };
        let prof = &w.bundle.profiles[o.state.bodies[s.id as usize].profile.unwrap() as usize];
        let pl = &w.space.places[e.place.unwrap() as usize];
        let to = pl.anchor - s.pos;
        let d = to.len();
        let focal = d <= prof.perc.focal_range_m && (d < 0.5 || angle_between(to.angle(), s.gaze) <= prof.perc.focal_angle_deg.to_radians() / 2.0);
        let peri = d <= prof.perc.peripheral_range_m && (d < 0.5 || angle_between(to.angle(), s.heading) <= prof.perc.peripheral_angle_deg.to_radians() / 2.0);
        assert!(focal || peri, "몸 {} 틱 {}: 시야 밖 장소 {}({d:.1}m) 알아차림", s.id, e.tick, pl.id);
        assert!(w.space.line_of_sight(s.pos, pl.anchor), "가린 장소 {} 알아차림", pl.id);
        n += 1;
    }
    assert!(n > 100 && at_spawn < n / 4, "알아차림 {n}, 도착 틱 {at_spawn}");
    let np = &w.p.notice;
    let k = (np.t_ref_s / w.dt()).round() as i32;
    for vis in [0.1, 0.5, 0.9] {
        for ad in [0.0, 0.5, 1.0] {
            let p = notice_pass_prob(np, vis, ad, 0.5, 0.3);
            let q = notice_tick_prob(np, p, 1.0, w.dt());
            assert!((1.0 - (1.0 - q).powi(k) - p).abs() < 1e-9);
        }
    }
}
