//! E205: 반응 함수(C-205)의 완료 기준 ①~⑦.
//!
//! 대안·성향·능력은 고정 자료의 인물 능력과 시험 전용 키드 난수("E205")로 만든다. 같은 시험은 늘 같은 입력을 쓴다.
mod common;

use common::{fx, prep, scenario};
use mdsim_core::bundle::Profile;
use mdsim_core::defs::LimitKind;
use mdsim_core::dims::{Dim, IntVec, ND, NI};
use mdsim_core::rng::KeyRng;
use mdsim_runtime::params::RtParams;
use mdsim_runtime::react::{explanation_full, react, terms_of, Alt, Caps, Person, Reaction};
use rayon::prelude::*;

/// 결정 하나의 입력.
#[derive(Clone, Debug)]
struct Case {
    prof: usize,
    caps: Caps,
    sat: IntVec,
    time_left: f64,
    alts: Vec<Alt>,
    u: f64,
}

fn params() -> RtParams {
    RtParams::load(&fx().defs.params).expect("params")
}

fn person(p: &Profile) -> Person<'_> {
    Person { sens: &p.sens, interest: &p.interest, risk: p.risk, punctuality: p.punctuality, tau: p.tau }
}

/// 인물의 능력 그대로, 시간 한계만 남은 분으로 둔다(경성 여부는 정의 파일).
fn caps_of(p: &Profile, time_left: f64) -> Caps {
    let defs = &fx().defs;
    let mut hard = [false; ND];
    for d in Dim::ALL {
        hard[d.ix()] = defs.dims[d.ix()].limit == LimitKind::Hard;
    }
    let mut comfort = p.comfort;
    let mut limit = p.limit;
    limit[Dim::Time.ix()] = time_left;
    comfort[Dim::Time.ix()] = comfort[Dim::Time.ix()].min(time_left);
    Caps { comfort, limit, hard }
}

/// 편안 상한 아래부터 한계 너머까지 고르게 흩은 대안.
fn gen_alt(r: &KeyRng, case: u64, k: u64, p: &Profile) -> Alt {
    let u = |j: u64| r.u("E205", "alt", case, k * 1000 + j, 0);
    let mut demand = [0.0; ND];
    for d in Dim::ALL {
        let i = d.ix();
        if matches!(d, Dim::Time | Dim::Money) {
            continue; // 시간·돈은 dt_min·cost_krw로 들어온다
        }
        demand[i] = if u(i as u64) < 0.3 { 0.0 } else { u(20 + i as u64) * p.limit[i].max(1e-3) * 1.6 };
    }
    let offer: IntVec = std::array::from_fn(|j| if u(40 + j as u64) < 0.5 { 0.0 } else { u(50 + j as u64) });
    Alt {
        demand,
        offer,
        purpose: u(60) * 2.0,
        achieve: u(61),
        dt_min: u(62) * 90.0,
        cost_krw: if u(63) < 0.4 { 0.0 } else { u(64) * p.limit[Dim::Money.ix()] * 1.2 },
        blocked: u(65) < 0.05,
    }
}

fn cases(n: u64) -> Vec<Case> {
    let f = fx();
    let r = KeyRng::new(205);
    (0..n)
        .map(|c| {
            let u = |j: u64| r.u("E205", "case", c, j, 0);
            let prof = ((u(0) * f.bundle.profiles.len() as f64) as usize).min(f.bundle.profiles.len() - 1);
            let p = &f.bundle.profiles[prof];
            let n_alts = 1 + (u(1) * 7.0) as u64;
            let time_left = 10.0 + u(2) * 230.0;
            let sat: IntVec = std::array::from_fn(|k| if u(10 + k as u64) < 0.5 { 0.0 } else { u(20 + k as u64) * 0.9 });
            Case { prof, caps: caps_of(p, time_left), sat, time_left, alts: (0..n_alts).map(|k| gen_alt(&r, c, k, p)).collect(), u: u(3) }
        })
        .collect()
}

fn run(c: &Case, p: &RtParams) -> Reaction {
    let f = fx();
    react(&person(&f.bundle.profiles[c.prof]), &c.caps, &c.sat, c.time_left, &c.alts, &f.defs.dims, &p.react, c.u)
}

/// 같은 입력·난수 키는 처리 순서(정순·역순·병렬)와 무관하게 같은 반응을 낸다.
/// 대안 순서를 바꾸면 대안별 효용·가능 여부는 그대로 따라가고 확률은 합의 반올림 오차 안에서 같다.
#[test]
fn e205_1_order_invariance() {
    let p = params();
    let cs = cases(400);
    let fwd: Vec<Reaction> = cs.iter().map(|c| run(c, &p)).collect();
    let mut rev: Vec<Reaction> = cs.iter().rev().map(|c| run(c, &p)).collect();
    rev.reverse();
    let par: Vec<Reaction> = cs.par_iter().map(|c| run(c, &p)).collect();
    assert_eq!(fwd, rev, "역순 처리");
    assert_eq!(fwd, par, "병렬 처리");
    for (c, r) in cs.iter().zip(&fwd) {
        let mut back = c.clone();
        back.alts.reverse();
        let r2 = run(&back, &p);
        let n = c.alts.len();
        for i in 0..n {
            assert_eq!(r.utils[i], r2.utils[n - 1 - i], "대안 순서와 무관한 효용");
            assert_eq!(r.feasible[i], r2.feasible[n - 1 - i]);
            if !r.forced {
                assert!((r.probs[i] - r2.probs[n - 1 - i]).abs() < 1e-12, "확률 {} vs {}", r.probs[i], r2.probs[n - 1 - i]);
            }
        }
    }
}

/// 식별자를 소문자 낱말로 쪼갠다(밑줄·낙타 표기).
fn words(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut ident = String::new();
    let flush = |ident: &mut String, out: &mut Vec<String>| {
        for part in ident.split('_') {
            let mut w = String::new();
            let chars: Vec<char> = part.chars().collect();
            for (i, &ch) in chars.iter().enumerate() {
                let boundary = ch.is_uppercase() && i > 0 && (chars[i - 1].is_lowercase() || chars.get(i + 1).map(|c| c.is_lowercase()).unwrap_or(false));
                if boundary && !w.is_empty() {
                    out.push(w.to_lowercase());
                    w.clear();
                }
                w.push(ch);
            }
            if !w.is_empty() {
                out.push(w.to_lowercase());
            }
        }
        ident.clear();
    };
    for ch in code.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            ident.push(ch);
        } else if !ident.is_empty() {
            flush(&mut ident, &mut out);
        }
    }
    if !ident.is_empty() {
        flush(&mut ident, &mut out);
    }
    out
}

/// react.rs는 차원 ID 밖의 사람·환경·맥락 개념 이름을 참조하지 않는다(02d I-205-r의 정적 검사).
/// 주석을 뺀 코드의 식별자와 문자열 낱말을 금지 목록과 대조하고, 개념 모듈(상황·몸·공간·번들)을 가져오지 않는지 본다.
/// `Person`은 02d I-205-r이 정한 반응 입력 구조체 이름(성향 값 묶음)이라 금지 목록에 넣지 않는다.
#[test]
fn e205_2_static_no_person_names() {
    let src = include_str!("../src/react.rs");
    let code: String = src.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
    let forbidden = [
        // 사람 속성(DR-214, C-205 ②)
        "age", "sex", "gender", "nationality", "nation", "income", "persona", "profile", "group", "tourist", "domestic", "foreign", "korean", "elderly", "child", "stroller", "luggage", "origin",
        // 결정 맥락과 맥락별 행동
        "visit", "next", "route", "rest", "purchase", "look", "store", "buy", "leave", "proceed", "pace", "abandon", "footing", "explore", "watch",
        // 환경 클래스·환경 개념
        "shop", "cafe", "stall", "event", "landmark", "lodging", "bench", "restaurant", "place", "portal", "edge", "segment", "steps", "building", "vehicle", "weather", "rain", "night", "light", "slope", "grade", "crossing", "sidewalk", "street", "queue",
    ];
    let found: Vec<String> = words(&code).into_iter().filter(|w| forbidden.contains(&w.as_str())).collect();
    assert!(found.is_empty(), "react.rs가 개념 이름을 참조함: {found:?}");
    for line in code.lines().filter(|l| l.trim_start().starts_with("use ")) {
        for m in ["crate::situation", "crate::state", "crate::space", "crate::visit", "crate::behavior", "crate::perception", "crate::step", "crate::env", "mdsim_core::bundle", "mdsim_core::scenario"] {
            assert!(!line.contains(m), "react.rs가 개념 모듈을 가져옴: {line}");
        }
    }
    // 검사기 자체의 확인: 금지 낱말이 섞인 식별자는 잡힌다
    assert!(words("let persona_id = b.ageBand; Context::Visit").iter().any(|w| w == "persona"));
    assert!(words("ageBand").contains(&"age".to_string()));
    assert!(words("Context::Visit").contains(&"visit".to_string()));
}

/// 단조성(무작위 입력의 속성 시험):
/// (a) 한 대안의 요구가 커지면 그 대안의 확률은 커지지 않는다.
/// (b) 능력(편안 상한·한계)이 커지면 그 부담을 지는 대안의 확률은 작아지지 않는다. 다른 대안은 그 차원 요구가 없다
///     (여러 대안이 같은 차원을 다르게 지면 능력이 오를 때 대안마다 비용이 다르게 줄어 개인 확률의 방향이 정해지지 않으므로, 01a §7.4 5의
///     "개인·한 결정 단위" 단조성을 그 차원을 지는 대안 하나로 읽는다).
/// (c) 목적 가치가 커지면 포기(목적을 이루지 않는 대안) 확률은 커지지 않는다.
/// (d) 한 대안의 가격이 내려가면 그 대안의 확률은 작아지지 않는다.
/// φ는 연속이고 단조 증가하며 볼록하다(S-E46): 한계(x = 1)에서 값이 뛰지 않고 넘김 기울기는 감당 구간 끝의 기울기 γ
/// 이상이다. 계수는 02c의 검증 범위(γ ≥ 1, β ≥ 0, P ≥ 0) 안의 여러 조합, 무리 감수 성향은 0~1. 경성이면 x ≥ 1은 불가다.
#[test]
fn e205_3_phi_continuous_convex() {
    use mdsim_runtime::react::phi_curve;
    for &(g, beta, pen) in &[(2.0, 4.0, 1.0), (1.0, 0.0, 0.0), (3.0, 1.0, 0.5), (1.5, 2.0, 3.0), (2.0, 0.5, 0.0)] {
        for &risk in &[0.0, 0.3, 1.0] {
            let f = |x: f64| phi_curve(x, g, beta, pen, false, risk).unwrap();
            assert!((f(1.0 - 1e-9) - f(1.0)).abs() < 1e-6, "({g},{beta},{pen},{risk}): x = 1에서 뜀 {} → {}", f(1.0 - 1e-9), f(1.0));
            let xs: Vec<f64> = (-20..=300).map(|i| i as f64 * 0.01).collect();
            for w in xs.windows(3) {
                let (a, b, c) = (f(w[0]), f(w[1]), f(w[2]));
                assert!(b >= a - 1e-12, "({g},{beta},{pen},{risk}): x = {:.2}에서 감소", w[1]);
                assert!(a + c - 2.0 * b >= -1e-9, "({g},{beta},{pen},{risk}): x = {:.2}에서 오목", w[1]);
            }
            assert!(phi_curve(1.0, g, beta, pen, true, risk).is_none(), "경성 한계");
            assert_eq!(phi_curve(0.999, g, beta, pen, true, risk), Some(0.999f64.powf(g)));
        }
    }
}

#[test]
fn e205_3_monotonicity_props() {
    let p = params();
    let f = fx();
    let eps = 1e-12;
    let mut checked = [0usize; 4];
    for c in cases(300) {
        let prof = &f.bundle.profiles[c.prof];
        let n = c.alts.len();
        let base = run(&c, &p);
        for i in 0..n.min(3) {
            // (a) 요구 증가
            for d in Dim::ALL {
                let mut more = c.clone();
                match d {
                    Dim::Time => more.alts[i].dt_min += 15.0,
                    Dim::Money => more.alts[i].cost_krw += 0.3 * prof.limit[d.ix()],
                    _ => more.alts[i].demand[d.ix()] += 0.3 * prof.limit[d.ix()].max(1e-3),
                }
                let r = run(&more, &p);
                assert!(r.probs[i] <= base.probs[i] + eps, "(a) {:?} 요구 증가로 확률 증가 {} → {}", d, base.probs[i], r.probs[i]);
                checked[0] += 1;
            }
            // (b) 능력 증가: 대안 i만 그 차원을 진다
            for d in Dim::ALL {
                let k = d.ix();
                let mut solo = c.clone();
                for (j, a) in solo.alts.iter_mut().enumerate() {
                    if j != i {
                        match d {
                            Dim::Time => a.dt_min = 0.0,
                            Dim::Money => a.cost_krw = 0.0,
                            _ => a.demand[k] = 0.0,
                        }
                    }
                }
                let before = run(&solo, &p);
                let mut cap = solo.clone();
                if d == Dim::Time {
                    cap.time_left += 60.0;
                    cap.caps.limit[k] = cap.time_left;
                } else {
                    let gap = (cap.caps.limit[k] - cap.caps.comfort[k]).max(1e-3);
                    cap.caps.comfort[k] += 0.25 * gap;
                    cap.caps.limit[k] += 0.25 * gap;
                }
                let after = run(&cap, &p);
                assert!(after.probs[i] >= before.probs[i] - eps, "(b) {:?} 능력 증가로 확률 감소 {} → {}", d, before.probs[i], after.probs[i]);
                checked[1] += 1;
            }
            // (d) 가격 인하
            if c.alts[i].cost_krw > 0.0 {
                let mut cheap = c.clone();
                cheap.alts[i].cost_krw *= 0.5;
                let r = run(&cheap, &p);
                assert!(r.probs[i] >= base.probs[i] - eps, "(d) 가격 인하로 확률 감소 {} → {}", base.probs[i], r.probs[i]);
                checked[3] += 1;
            }
        }
        // (c) 목적 가치 증가: 모든 대안이 같은 목적을 achieve만큼 이루고, 0번(포기)은 이루지 않는다
        if n >= 2 {
            let mut goal = c.clone();
            for (j, a) in goal.alts.iter_mut().enumerate() {
                a.purpose = 1.0;
                if j == 0 {
                    a.achieve = 0.0;
                }
            }
            let before = run(&goal, &p);
            for a in goal.alts.iter_mut() {
                a.purpose = 2.5;
            }
            let after = run(&goal, &p);
            assert!(after.probs[0] <= before.probs[0] + eps, "(c) 목적 가치 증가로 포기 확률 증가 {} → {}", before.probs[0], after.probs[0]);
            checked[2] += 1;
        }
    }
    assert!(checked.iter().all(|&k| k > 50), "검사 수 {checked:?}");
}

/// 경성 한계를 넘는 대안(그리고 막힌 대안)의 확률은 0이다. 가능 여부는 독립 계산과 같다.
#[test]
fn e205_4_hard_limit_zero() {
    let p = params();
    let f = fx();
    let mut infeasible_seen = 0;
    for c in cases(400) {
        let r = run(&c, &p);
        for (i, a) in c.alts.iter().enumerate() {
            let mut ok = !a.blocked;
            for d in Dim::ALL {
                let k = d.ix();
                let mut pv = a.demand[k];
                if d == Dim::Time {
                    pv += a.dt_min;
                }
                if d == Dim::Money {
                    pv += a.cost_krw;
                }
                // 02d §1.5: x = (P − c)/max(l − c, 1e-9), 경성 차원에서 x ≥ 1이면 불가
                let x = (pv - c.caps.comfort[k]) / (c.caps.limit[k] - c.caps.comfort[k]).max(1e-9);
                if c.caps.hard[k] && pv > 0.0 && x >= 1.0 {
                    ok = false;
                }
            }
            assert_eq!(r.feasible[i], ok, "가능 여부(대안 {i})");
            if !ok {
                infeasible_seen += 1;
                if !r.forced {
                    assert_eq!(r.probs[i], 0.0, "경성 한계를 넘는 대안의 확률");
                }
            }
        }
    }
    assert!(infeasible_seen > 50, "불가 대안 {infeasible_seen}");
    // 손으로 만든 경우: 시간(마감)·돈(예산)·단차(유모차라 경성)
    let prof = &f.bundle.profiles[0];
    let mut caps = caps_of(prof, 30.0);
    caps.hard[Dim::Step.ix()] = true;
    caps.comfort[Dim::Step.ix()] = 0.0;
    caps.limit[Dim::Step.ix()] = 1e-6;
    let ok = Alt { demand: [0.0; ND], offer: [0.0; NI], purpose: 0.0, achieve: 1.0, dt_min: 5.0, cost_krw: 0.0, blocked: false };
    let late = Alt { dt_min: 31.0, purpose: 50.0, ..ok.clone() };
    let pricey = Alt { cost_krw: prof.limit[Dim::Money.ix()] * 1.01, purpose: 50.0, ..ok.clone() };
    let mut stairs = Alt { purpose: 50.0, ..ok.clone() };
    stairs.demand[Dim::Step.ix()] = 15.0;
    let r = react(&person(prof), &caps, &[0.0; NI], 30.0, &[ok, late, pricey, stairs], &f.defs.dims, &p.react, 0.99);
    assert_eq!(r.probs, vec![1.0, 0.0, 0.0, 0.0], "목적 가치가 커도 경성 한계를 넘으면 0");
    assert_eq!(r.chosen, 0);
}

/// 설명: 선택과 차선(확률 2위) 사이 항별 기여의 합이 두 효용의 차이와 같고, 상위 3개는 그 중 절댓값이 큰 것이다.
#[test]
fn e205_5_explanation_sum() {
    let p = params();
    for c in cases(400) {
        let r = run(&c, &p);
        let full = explanation_full(&r);
        let sum: f64 = full.iter().map(|x| x.1).sum();
        let n = r.probs.len();
        let others: Vec<usize> = (0..n).filter(|&i| i != r.chosen).collect();
        let diff = if others.is_empty() {
            r.utils[r.chosen] // 대안이 하나면 차선은 0
        } else {
            let pmax = others.iter().map(|&i| r.probs[i]).fold(f64::NEG_INFINITY, f64::max);
            let cands: Vec<f64> = others.iter().filter(|&&i| r.probs[i] == pmax).map(|&i| r.utils[r.chosen] - r.utils[i]).collect();
            *cands.iter().min_by(|a, b| (*a - sum).abs().total_cmp(&(*b - sum).abs())).unwrap()
        };
        let scale = 1.0 + r.utils.iter().map(|u| u.abs()).fold(0.0, f64::max);
        assert!((sum - diff).abs() <= 1e-9 * scale, "설명 합 {sum} ≠ 효용 차 {diff}");
        let mut sorted = full.clone();
        sorted.sort_by(|x, y| y.1.abs().total_cmp(&x.1.abs()).then(x.0.cmp(&y.0)));
        sorted.truncate(3);
        assert_eq!(r.top3, sorted, "상위 3 기여 항");
    }
}

/// 가능한 대안이 하나뿐이면 그 대안을 고른다(확률 1). 모두 불가하면 효용 최댓값을 강제로 고르고 forced로 남긴다.
/// 그 대안의 부담은 사라지지 않고 항(cost)으로 남아 상태 갱신(C-206)에 넘어간다.
#[test]
fn e205_6_single_alt() {
    let p = params();
    let f = fx();
    let mut n_only = 0;
    for c in cases(300) {
        let prof = &f.bundle.profiles[c.prof];
        // (a) 대안 하나
        let mut one = c.clone();
        one.alts.truncate(1);
        let r = run(&one, &p);
        if r.feasible[0] {
            assert_eq!((r.chosen, r.probs.clone(), r.forced), (0, vec![1.0], false));
        } else {
            assert_eq!((r.chosen, r.probs.clone(), r.forced), (0, vec![1.0], true), "불가한 유일 대안도 고른다");
        }
        let (t, _) = terms_of(&person(prof), &one.caps, &one.sat, one.time_left, &one.alts[0], &f.defs.dims, &p.react);
        assert_eq!(r.terms[0], t, "부담 항이 그대로 남는다");
        // (b) 여럿 중 가능한 것이 하나
        if c.alts.len() >= 2 {
            let mut only = c.clone();
            let keep = c.alts.len() - 1;
            for (j, a) in only.alts.iter_mut().enumerate() {
                a.blocked = j != keep;
            }
            only.alts[keep].blocked = false;
            only.alts[keep].dt_min = 0.0;
            only.alts[keep].cost_krw = 0.0;
            for d in Dim::ALL {
                if only.caps.hard[d.ix()] {
                    only.alts[keep].demand[d.ix()] = 0.0;
                }
            }
            let r = run(&only, &p);
            assert_eq!(r.chosen, keep);
            assert_eq!(r.probs[keep], 1.0);
            assert!(!r.forced);
            n_only += 1;
        }
        // (c) 모두 불가
        let mut none = c.clone();
        for a in none.alts.iter_mut() {
            a.blocked = true;
        }
        let r = run(&none, &p);
        assert!(r.forced);
        let best = (0..r.utils.len()).fold(0, |b, i| if r.utils[i] > r.utils[b] { i } else { b });
        assert_eq!(r.chosen, best);
        assert_eq!(r.probs[best], 1.0);
    }
    assert!(n_only > 100);
}

/// 한 결정의 대안별 확률·효용·항은 난수 키(u)와 무관하다. u는 누적 확률 구간으로 선택만 정한다.
/// 방문지 선택을 다른 시드로 돌려도 대안별 확률은 같다.
#[test]
fn e205_7_probs_key_free() {
    let p = params();
    let grid = 10_000usize;
    for c in cases(80) {
        let base = run(&c, &p);
        for u in [0.0, 0.123, 0.5, 0.987_654, 1.0 - 1e-12] {
            let r = run(&Case { u, ..c.clone() }, &p);
            assert_eq!((&r.probs, &r.utils, &r.feasible, &r.terms), (&base.probs, &base.utils, &base.feasible, &base.terms));
        }
        if base.forced {
            continue;
        }
        // 고른 u 격자에서 대안별 선택 비율 = 확률(격자 해상도 안)
        let mut count = vec![0usize; c.alts.len()];
        for g in 0..grid {
            let u = (g as f64 + 0.5) / grid as f64;
            count[run(&Case { u, ..c.clone() }, &p).chosen] += 1;
        }
        for (i, &k) in count.iter().enumerate() {
            assert!((k as f64 / grid as f64 - base.probs[i]).abs() <= 2.0 / grid as f64 + 1e-12, "대안 {i}: 비율 {} 확률 {}", k as f64 / grid as f64, base.probs[i]);
        }
    }
    // 실행 수준: 시드만 다른 두 시나리오의 VISIT 확률
    let a = prep(scenario("quick"));
    let mut s2 = scenario("quick");
    s2.seed += 7;
    let b = prep(s2);
    assert_eq!(a.dec.len(), b.dec.len());
    let mut chosen_differs = 0;
    for (x, y) in a.dec.iter().zip(&b.dec) {
        assert_eq!(x.reaction.probs, y.reaction.probs, "인물 {}", x.profile);
        chosen_differs += usize::from(x.reaction.chosen != y.reaction.chosen);
    }
    assert!(chosen_differs > 0, "시드가 바뀌면 고른 대안은 달라질 수 있다");
}
