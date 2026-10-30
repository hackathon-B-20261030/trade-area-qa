//! 반응 함수 (C-205, 02d I-205-r, 02d §1.5).
//!
//! 모든 결정 맥락이 같은 함수를 쓴다. 이 파일은 차원 ID 외의 사람·환경·맥락 개념 이름을 쓰지 않는다
//! (E205-2 정적 검사 대상). 대안은 요구·제공·목적·시간·돈 값으로만 들어온다.

use crate::params::ReactParams;
use mdsim_core::defs::DimDef;
use mdsim_core::dims::{Dim, DimVec, IntVec, Interest, ND, NI};
use serde::{Deserialize, Serialize};

/// 대안 하나의 값. 시간·돈은 dt_min·cost_krw로 받아 D-TIME·D-MONEY 요구에 더한다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alt {
    pub demand: DimVec,
    pub offer: IntVec,
    pub purpose: f64,
    pub achieve: f64,
    pub dt_min: f64,
    pub cost_krw: f64,
    /// 허용·차단 간선으로 막힌 대안(01a 연결 규칙 4).
    pub blocked: bool,
}

/// 유효 능력. hard는 그 순간 경성으로 다룰 차원.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Caps {
    pub comfort: DimVec,
    pub limit: DimVec,
    pub hard: [bool; ND],
}

/// 반응에 쓰는 성향 값.
#[derive(Clone, Copy, Debug)]
pub struct Person<'a> {
    pub sens: &'a DimVec,
    pub interest: &'a IntVec,
    pub risk: f64,
    pub punctuality: f64,
    pub tau: f64,
}

/// 효용의 항별 기여(양수는 효용을 올린다).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Terms {
    pub purpose: f64,
    pub interest: IntVec,
    /// 차원 비용(효용에서 빼는 양, 0 이상)
    pub cost: DimVec,
    pub time: f64,
    pub money: f64,
}

impl Terms {
    pub fn utility(&self) -> f64 {
        self.purpose + self.interest.iter().sum::<f64>() - self.cost.iter().sum::<f64>() - self.time - self.money
    }

    /// (항 이름, 효용에 대한 부호 있는 기여) 목록. 이름은 기록 표에 그대로 쓴다(purpose, interest:A-…, cost:D-…, time, money).
    pub fn signed(&self) -> Vec<(String, f64)> {
        let mut v = vec![("purpose".to_string(), self.purpose)];
        for k in Interest::ALL {
            v.push((format!("interest:{}", k.id()), self.interest[k.ix()]));
        }
        for d in Dim::ALL {
            v.push((format!("cost:{}", d.id()), -self.cost[d.ix()]));
        }
        v.push(("time".to_string(), -self.time));
        v.push(("money".to_string(), -self.money));
        v
    }

    pub fn interest_total(&self) -> f64 {
        self.interest.iter().sum()
    }

    pub fn cost_total(&self) -> f64 {
        self.cost.iter().sum()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reaction {
    pub probs: Vec<f64>,
    pub utils: Vec<f64>,
    pub feasible: Vec<bool>,
    pub terms: Vec<Terms>,
    pub chosen: usize,
    pub forced: bool,
    pub top3: Vec<(String, f64)>,
}

/// 부담 위치 x.
pub fn burden(p: f64, comfort: f64, limit: f64) -> f64 {
    (p - comfort) / (limit - comfort).max(1e-9)
}

/// 비용 곡선 φ. None이면 경성 한계를 넘어 불가.
pub fn phi(x: f64, def: &DimDef, hard: bool, risk: f64) -> Option<f64> {
    phi_curve(x, def.gamma, def.beta, def.penalty, hard, risk)
}

/// φ의 수식(차원 정의 없이 계수로). 상점 차원(D-COST·D-DEMAND)도 같은 곡선을 쓴다(C-211).
/// 한계를 넘으면 넘김 벌점이 기울기에 더해진다: 곡선은 연속이고 볼록하다(S-E46). 넘는 순간 값이
/// 뛰면 같은 차원을 다른 배율로 지닌 두 대안의 선호가 요구가 커지는 도중 뒤집힌다(E221-6).
pub fn phi_curve(x: f64, gamma: f64, beta: f64, penalty: f64, hard: bool, risk: f64) -> Option<f64> {
    if x <= 0.0 {
        Some(0.0)
    } else if x < 1.0 {
        Some(x.powf(gamma))
    } else if hard {
        None
    } else {
        let over_k = (beta + penalty * (1.0 - risk.clamp(0.0, 1.0))).max(gamma);
        Some(1.0 + over_k * (x - 1.0))
    }
}

pub fn time_weight(p: &ReactParams, punctuality: f64, time_left_min: f64) -> f64 {
    p.time_weight * (1.0 + punctuality) * (1.0 + p.deadline_k * (1.0 - time_left_min / p.deadline_horizon_min.max(1e-9)).max(0.0))
}

/// 대안의 항과 가능 여부(경성 한계 초과면 false). 가능하지 않아도 항은 경성을 무시하고 계산해 둔다.
pub fn terms_of(person: &Person, caps: &Caps, sat: &IntVec, time_left_min: f64, a: &Alt, dims: &[DimDef; ND], p: &ReactParams) -> (Terms, bool) {
    let mut t = Terms { purpose: a.purpose * a.achieve, ..Default::default() };
    for k in 0..NI {
        t.interest[k] = person.interest[k] * (1.0 - sat[k]).max(0.0) * a.offer[k];
    }
    let mut feasible = !a.blocked;
    for d in Dim::ALL {
        let i = d.ix();
        let mut pv = a.demand[i];
        if d == Dim::Time {
            pv += a.dt_min;
        }
        if d == Dim::Money {
            pv += a.cost_krw;
        }
        if pv <= 0.0 {
            continue;
        }
        let x = burden(pv, caps.comfort[i], caps.limit[i]);
        if dims[i].linear {
            // cost_curve: linear(D-TIME·D-MONEY): φ 항 없이 경성 한계만 본다. 비용은 아래 선형 항(time·money)이 맡는다(02c B4).
            if caps.hard[i] && x >= 1.0 {
                feasible = false;
            }
            continue;
        }
        match phi(x, &dims[i], caps.hard[i], person.risk) {
            Some(c) => t.cost[i] = person.sens[i] * c,
            None => {
                feasible = false;
                t.cost[i] = person.sens[i] * phi(x, &dims[i], false, person.risk).unwrap();
            }
        }
    }
    t.time = time_weight(p, person.punctuality, time_left_min) * a.dt_min.max(0.0);
    t.money = person.sens[Dim::Money.ix()] / caps.comfort[Dim::Money.ix()].max(1.0) * a.cost_krw.max(0.0);
    (t, feasible)
}

/// 반응. u는 키드 난수의 균등값. 한 결정의 대안별 확률은 u와 무관하다(C-205 ⑦).
pub fn react(person: &Person, caps: &Caps, sat: &IntVec, time_left_min: f64, alts: &[Alt], dims: &[DimDef; ND], p: &ReactParams, u: f64) -> Reaction {
    let n = alts.len();
    let mut terms = Vec::with_capacity(n);
    let mut feasible = Vec::with_capacity(n);
    let mut utils = Vec::with_capacity(n);
    for a in alts {
        let (t, f) = terms_of(person, caps, sat, time_left_min, a, dims, p);
        utils.push(t.utility());
        terms.push(t);
        feasible.push(f);
    }
    let tau = person.tau.max(1e-3);
    let mut probs = vec![0.0; n];
    let mut forced = false;
    let any = feasible.iter().any(|&f| f);
    let chosen;
    if n == 0 {
        return Reaction { probs, utils, feasible, terms, chosen: 0, forced: true, top3: Vec::new() };
    }
    if any {
        let umax = (0..n).filter(|&i| feasible[i]).map(|i| utils[i]).fold(f64::NEG_INFINITY, f64::max);
        let mut z = 0.0;
        for i in 0..n {
            if feasible[i] {
                probs[i] = ((utils[i] - umax) / tau).exp();
                z += probs[i];
            }
        }
        for pr in probs.iter_mut() {
            *pr /= z;
        }
        let mut acc = 0.0;
        let mut pick = None;
        let mut last = 0;
        for i in 0..n {
            if feasible[i] {
                last = i;
                acc += probs[i];
                if u < acc {
                    pick = Some(i);
                    break;
                }
            }
        }
        chosen = pick.unwrap_or(last);
    } else {
        // 모두 경성 한계를 넘으면 효용 최댓값 대안을 강제로 고른다(마감을 넘긴 떠나기 등).
        forced = true;
        let mut best = 0;
        for i in 1..n {
            if utils[i] > utils[best] {
                best = i;
            }
        }
        chosen = best;
        probs[best] = 1.0;
    }
    // 설명: 선택과 차선(확률 2위, 동률이면 앞 순서) 사이 항별 차이의 절댓값 상위 3(C-205 ⑤).
    let second = (0..n).filter(|&i| i != chosen).max_by(|&i, &j| probs[i].total_cmp(&probs[j]).then(utils[i].total_cmp(&utils[j])).then(j.cmp(&i)));
    let a = terms[chosen].signed();
    let b = second.map(|s| terms[s].signed()).unwrap_or_else(|| Terms::default().signed());
    let mut diffs: Vec<(String, f64)> = a.iter().zip(b.iter()).map(|((name, x), (_, y))| (name.clone(), x - y)).collect();
    diffs.sort_by(|x, y| y.1.abs().total_cmp(&x.1.abs()).then(x.0.cmp(&y.0)));
    diffs.truncate(3);
    Reaction { probs, utils, feasible, terms, chosen, forced, top3: diffs }
}

/// 선택과 차선의 효용 차이를 이루는 전체 항 차이(설명 합 검사용).
pub fn explanation_full(r: &Reaction) -> Vec<(String, f64)> {
    let n = r.probs.len();
    let second = (0..n).filter(|&i| i != r.chosen).max_by(|&i, &j| r.probs[i].total_cmp(&r.probs[j]).then(r.utils[i].total_cmp(&r.utils[j])).then(j.cmp(&i)));
    let a = r.terms[r.chosen].signed();
    let b = second.map(|s| r.terms[s].signed()).unwrap_or_else(|| Terms::default().signed());
    a.iter().zip(b.iter()).map(|((name, x), (_, y))| (name.clone(), x - y)).collect()
}
