//! 상점 월 반응 (C-211, 02d I-211-r).
//!
//! 상점(store·cafe)이 달마다 그달 매출·손님으로 비용 부담(D-COST)과 수요 부족(D-DEMAND)을 보고
//! 유지·가격·영업시간·인력·업종 전환·폐업 중 하나를 사람과 같은 φ·소프트맥스로 고른다.
//! 재무 값이 없으면 첫 달 매출의 비율로 채운다(02c S-I7, 02d S-E38).

use crate::react::phi_curve;
use crate::record::EventRow;
use crate::space::{PlaceKind, Space};
use mdsim_core::bundle::Bundle;
use mdsim_core::defs::{hex, Params};
use mdsim_core::dims::{Dim, IntVec, NI};
use mdsim_core::rng::{canonical_key, KeyRng};
use mdsim_core::scenario::DayType;
use mdsim_core::{Result, SimError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// 월 고리가 도는 하루 유형(일수 배열의 순서).
pub const DAY_TYPES: [DayType; 3] = [DayType::Weekday, DayType::Weekend, DayType::EventDay];

/// 상점 행동(02c contexts.yaml STORE).
pub const STORE_ACTIONS: [&str; 9] = ["KEEP", "PRICE_UP", "PRICE_DOWN", "HOURS_UP", "HOURS_DOWN", "STAFF_UP", "STAFF_DOWN", "SWITCH_CATEGORY", "CLOSE"];

/// 구성별 상점 행동(02c I-207-b `configurations.yaml`과 같은 표).
pub fn configuration_actions(configuration: &str) -> Result<&'static [&'static str]> {
    match configuration {
        "static_stores" => Ok(&[]),
        "policy_experiment" => Ok(&["KEEP", "STAFF_UP", "STAFF_DOWN", "SWITCH_CATEGORY", "CLOSE"]),
        "store_adaptation" => Ok(&STORE_ACTIONS),
        o => Err(SimError::Run(format!("모르는 configuration '{o}'(static_stores·policy_experiment·store_adaptation)"))),
    }
}

/// 요일(0 = 일요일, Sakamoto).
fn weekday_of(y: i32, m: u32, d: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d as i32).rem_euclid(7)) as u32
}

fn month_len(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// 달의 하루 유형별 일수 [평일, 주말, 행사일](02c I-204-c `days_in_month`와 같은 규칙).
pub fn days_in_month(year: i32, month: u32, event_days: u32) -> Result<[u32; 3]> {
    let (mut w, mut e) = (0u32, 0u32);
    for d in 1..=month_len(year, month) {
        match weekday_of(year, month, d) {
            0 | 6 => e += 1,
            _ => w += 1,
        }
    }
    if event_days > e {
        return Err(SimError::Run(format!("event_days_per_month {event_days}가 그 달 토·일 수 {e}보다 크다")));
    }
    Ok([w, e - event_days, event_days])
}

/// 월 고리 하루 실행의 시드: SHA-256("{seed}:{month}:{day_type}") 앞 8바이트(02d S-E39).
pub fn month_seed(seed: u64, month: u32, day: DayType) -> u64 {
    let d = Sha256::digest(format!("{seed}:{month}:{}", day.as_str()).as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b)
}

/// `shop.*` 계수(02d I-211-r, 모두 가정값).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShopParams {
    pub rent_share: f64,
    pub labor_share: f64,
    pub other_share: f64,
    pub staff_default: f64,
    pub min_revenue_krw: f64,
    pub cost_comfort: f64,
    pub cost_limit: f64,
    pub demand_comfort: f64,
    pub demand_limit: f64,
    pub price_step: f64,
    pub price_elasticity: f64,
    pub hours_step_min: f64,
    pub hours_revenue_elasticity: f64,
    pub staff_recover_share: f64,
    pub staff_down_loss: f64,
    pub switch_efficiency: f64,
    pub switch_cost: f64,
    pub close_cost: f64,
    pub change_cost: f64,
    pub profit_weight: f64,
    /// 결정에 쓰는 매출 기억(지수 평균의 이전 값 비중). 한 달 표본의 잡음에 바로 반응하지 않게 한다.
    pub revenue_memory: f64,
    /// 업종 전환의 기대 매출 배율 상한(자기 업종 대비 가장 나은 업종의 평균 비).
    pub switch_max_ratio: f64,
    /// 관찰을 믿는 표본 크기의 기준(손님 수). 표본이 이보다 작으면 기준값 쪽에 머문다.
    pub evidence_customers: f64,
    pub tau: f64,
}

impl ShopParams {
    pub fn load(p: &Params) -> Result<ShopParams> {
        let f = |k: &str| p.f(&format!("shop.{k}"));
        Ok(ShopParams {
            rent_share: f("rent_share")?,
            labor_share: f("labor_share")?,
            other_share: f("other_share")?,
            staff_default: f("staff_default")?,
            min_revenue_krw: f("min_revenue_krw")?,
            cost_comfort: f("cost_comfort")?,
            cost_limit: f("cost_limit")?,
            demand_comfort: f("demand_comfort")?,
            demand_limit: f("demand_limit")?,
            price_step: f("price_step")?,
            price_elasticity: f("price_elasticity")?,
            hours_step_min: f("hours_step_min")?,
            hours_revenue_elasticity: f("hours_revenue_elasticity")?,
            staff_recover_share: f("staff_recover_share")?,
            staff_down_loss: f("staff_down_loss")?,
            switch_efficiency: f("switch_efficiency")?,
            switch_cost: f("switch_cost")?,
            close_cost: f("close_cost")?,
            change_cost: f("change_cost")?,
            profit_weight: f("profit_weight")?,
            revenue_memory: f("revenue_memory")?,
            switch_max_ratio: f("switch_max_ratio")?,
            evidence_customers: f("evidence_customers")?,
            tau: f("tau")?,
        })
    }
}

/// 상점 하나의 상태(달마다 바뀐다).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub place: u32,
    pub id: String,
    pub class_ix: usize,
    pub closed: bool,
    pub price: f64,
    pub discount: f64,
    pub open_min: f64,
    pub close_min: f64,
    pub service_rate: f64,
    pub category: String,
    pub offer: IntVec,
    pub staff: f64,
    pub wage: f64,
    pub rent: f64,
    /// 재무 값이 가정(첫 달 매출 비율)인가.
    pub finance_assumed: bool,
    /// 첫 달 매출·객단가(재무 기준).
    pub r0: f64,
    pub t0: f64,
}

/// 상점 자료(장소 표의 재무 열)가 있으면 그 값으로, 없으면 첫 달에 채울 상점 목록.
pub fn init_stores(space: &Space, bundle: &Bundle) -> Vec<Store> {
    let rows: BTreeMap<&str, &mdsim_core::bundle::PlaceRow> = bundle.places.iter().map(|p| (p.id.as_str(), p)).collect();
    space
        .places
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PlaceKind::Shop && p.can_buy)
        .map(|(i, p)| {
            let row = rows.get(p.id.as_str());
            let staff = row.and_then(|r| r.staff_count).filter(|v| *v > 0.0);
            let wage = row.and_then(|r| r.wage_krw_month).filter(|v| *v > 0.0);
            let rent = row.and_then(|r| r.rent_krw_month).filter(|v| *v >= 0.0);
            Store {
                place: i as u32,
                id: p.id.clone(),
                class_ix: p.class_ix,
                closed: false,
                price: p.base_price,
                discount: p.discount,
                open_min: p.open_min,
                close_min: p.close_min,
                service_rate: p.service_rate,
                category: p.category.clone(),
                offer: p.offer,
                staff: staff.unwrap_or(0.0),
                wage: wage.unwrap_or(0.0),
                rent: rent.unwrap_or(-1.0),
                finance_assumed: !(staff.is_some() && wage.is_some() && rent.is_some()),
                r0: 0.0,
                t0: 0.0,
            }
        })
        .collect()
}

/// 상점 상태를 공간 사본의 장소 값에 반영한다(가격·D-MONEY 요구·영업시간·처리 용량·업종·폐업).
pub fn apply_to_space(space: &mut Space, stores: &[Store]) {
    for s in stores {
        let pl = &mut space.places[s.place as usize];
        if s.closed {
            pl.go_to = false;
            pl.can_buy = false;
            pl.offer = [0.0; NI];
            pl.open_min = 0.0;
            pl.close_min = 0.0;
            pl.service_rate = 0.0;
            continue;
        }
        pl.base_price = s.price;
        pl.price = s.price * (1.0 - s.discount / 100.0);
        pl.demand[Dim::Money.ix()] = pl.price;
        pl.open_min = s.open_min;
        pl.close_min = s.close_min;
        pl.service_rate = s.service_rate;
        pl.category = s.category.clone();
        pl.offer = s.offer;
    }
}

/// 하루 실행의 장소별 매출·손님·줄 포기.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DayTotals {
    pub revenue: Vec<f64>,
    pub customers: Vec<f64>,
    pub abandoned: Vec<f64>,
}

pub fn day_totals(events: &[EventRow], n_places: usize) -> DayTotals {
    let mut t = DayTotals { revenue: vec![0.0; n_places], customers: vec![0.0; n_places], abandoned: vec![0.0; n_places] };
    for e in events {
        let Some(p) = e.place else { continue };
        let p = p as usize;
        if p >= n_places {
            continue;
        }
        match e.kind.as_str() {
            "purchase" => {
                t.revenue[p] += e.value;
                t.customers[p] += 1.0;
            }
            "queue_leave" if e.info == "abandoned" => t.abandoned[p] += 1.0,
            _ => {}
        }
    }
    t
}

/// 한 상점의 그달 관찰(하루 유형별 하루 값 × 일수).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MonthObs {
    pub revenue: f64,
    pub customers: f64,
    pub abandoned: f64,
    pub revenue_day: [f64; 3],
    pub days: [u32; 3],
    /// 실제로 시뮬레이션한 손님 수(일수를 곱하지 않은 표본 크기).
    pub sampled_customers: f64,
}

pub fn month_obs(days: [u32; 3], per_day: &[Option<DayTotals>; 3], place: u32) -> MonthObs {
    let mut o = MonthObs { days, ..Default::default() };
    for k in 0..3 {
        if let Some(t) = &per_day[k] {
            let p = place as usize;
            o.revenue_day[k] = t.revenue[p];
            o.revenue += t.revenue[p] * days[k] as f64;
            o.customers += t.customers[p] * days[k] as f64;
            o.abandoned += t.abandoned[p] * days[k] as f64;
            o.sampled_customers += t.customers[p];
        }
    }
    o
}

/// 상점 대안 하나의 다음 달 기대값과 효용 항.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreAlt {
    pub action: String,
    pub revenue: f64,
    pub customers: f64,
    pub fixed: f64,
    /// 이익 항(효용 단위).
    pub profit: f64,
    pub cost_burden: f64,
    pub demand_gap: f64,
    pub phi_cost: f64,
    pub phi_demand: f64,
    pub action_cost: f64,
    pub utility: f64,
    pub prob: f64,
}

/// 상점 결정 하나의 기록.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreDecision {
    pub month: u32,
    pub place: u32,
    pub place_id: String,
    pub alts: Vec<StoreAlt>,
    pub chosen: usize,
    pub rng_key: String,
    /// 이번 달 값(기록용).
    pub obs: MonthObs,
    pub cost_burden: f64,
    pub demand_gap: f64,
}

/// 결정에 쓰는 관찰(S-E42): 이번 달 값을 표본 크기만큼만 기준 쪽에서 옮기고(수축 w = n ÷ (n + shop.evidence_customers),
/// n = 실제로 시뮬레이션한 손님 수), 지난 기억과 지수 평균한다. 기록(store_months)은 이번 달 실현값이다.
pub fn remembered(prev: Option<&MonthObs>, now: &MonthObs, s: &Store, sp: &ShopParams) -> MonthObs {
    let w = now.sampled_customers / (now.sampled_customers + sp.evidence_customers.max(0.0)).max(1e-9);
    let base_c = s.r0 / s.t0.max(1.0);
    let shrunk = MonthObs { revenue: s.r0 + (now.revenue - s.r0) * w, customers: base_c + (now.customers - base_c) * w, abandoned: now.abandoned * w, ..now.clone() };
    match prev {
        None => shrunk,
        Some(p) => {
            let m = sp.revenue_memory.clamp(0.0, 1.0);
            MonthObs { revenue: m * p.revenue + (1.0 - m) * shrunk.revenue, customers: m * p.customers + (1.0 - m) * shrunk.customers, abandoned: m * p.abandoned + (1.0 - m) * shrunk.abandoned, ..shrunk }
        }
    }
}

/// 기준 달(첫 달)의 기억: 재무 기준 R0와 그에 맞춘 손님 수(R0 ÷ T0)로 시작한다.
pub fn baseline_memory(s: &Store, now: &MonthObs) -> MonthObs {
    MonthObs { revenue: s.r0, customers: s.r0 / s.t0.max(1.0), abandoned: now.abandoned, ..now.clone() }
}

/// 첫 달에 재무 기준(R0, T0)과 가정 재무를 채운다.
pub fn set_baseline(s: &mut Store, obs: &MonthObs, sp: &ShopParams) {
    s.r0 = obs.revenue.max(sp.min_revenue_krw);
    let customers = obs.customers.max(1.0);
    s.t0 = (s.r0 / customers).max(1.0);
    if s.finance_assumed {
        if s.rent < 0.0 {
            s.rent = sp.rent_share * s.r0;
        }
        if s.staff <= 0.0 {
            s.staff = sp.staff_default.max(1.0);
        }
        if s.wage <= 0.0 {
            s.wage = sp.labor_share * s.r0 / s.staff;
        }
    }
}

/// 이번 달 고정비(임대 지원·인건비 지수 반영).
fn fixed_cost(rent: f64, staff: f64, wage: f64, hours_factor: f64, support_pct: f64, labor_index: f64) -> f64 {
    rent * (1.0 - support_pct / 100.0) + labor_index * staff * wage * hours_factor
}

fn burdens(revenue: f64, customers: f64, fixed: f64, s: &Store, sp: &ShopParams) -> (f64, f64) {
    let cost_burden = fixed / revenue.max(1.0);
    let breakeven = fixed / (s.t0 * (1.0 - sp.other_share)).max(1e-9);
    (cost_burden, breakeven / customers.max(1.0))
}

fn phi_store(v: f64, comfort: f64, limit: f64) -> f64 {
    let x = (v - comfort) / (limit - comfort).max(1e-9);
    // 사람 차원과 같은 곡선(γ 2, β 4, P 1, 02c store_dimensions), 성향은 중립 0.5
    phi_curve(x, 2.0, 4.0, 1.0, false, 0.5).unwrap_or(0.0)
}

/// 그달 업종별 상점당 매출(SWITCH_CATEGORY의 기대값).
pub fn category_stats(stores: &[Store], obs: &[MonthObs]) -> BTreeMap<(usize, String), (f64, IntVec, u32)> {
    let mut m: BTreeMap<(usize, String), (f64, IntVec, u32)> = BTreeMap::new();
    for (s, o) in stores.iter().zip(obs) {
        if s.closed {
            continue;
        }
        let e = m.entry((s.class_ix, s.category.clone())).or_insert((0.0, [0.0; NI], 0));
        e.0 += o.revenue;
        for k in 0..NI {
            e.1[k] += s.offer[k];
        }
        e.2 += 1;
    }
    for v in m.values_mut() {
        let n = v.2.max(1) as f64;
        v.0 /= n;
        for k in 0..NI {
            v.1[k] /= n;
        }
    }
    m
}

/// 대안을 만들고 고른다. 정책 실험 구성의 가격·영업시간 대안은 `allowed`에서 빠진다.
#[allow(clippy::too_many_arguments)]
pub fn decide_store(s: &Store, obs: &MonthObs, month: u32, allowed: &[&str], support_pct: f64, labor_index: f64, cats: &BTreeMap<(usize, String), (f64, IntVec, u32)>, sp: &ShopParams, rng: &KeyRng) -> StoreDecision {
    let ticket = if obs.customers > 0.0 { obs.revenue / obs.customers } else { s.t0 };
    let hours = (s.close_min - s.open_min).max(1.0);
    let base_fixed = fixed_cost(s.rent, s.staff, s.wage, 1.0, support_pct, labor_index);
    let (cb_now, dg_now) = burdens(obs.revenue, obs.customers, base_fixed, s, sp);
    let r0 = s.r0.max(1.0);
    let mut alts = Vec::new();
    let mut push = |action: &str, revenue: f64, customers: f64, fixed: f64, action_cost: f64, closing: bool| {
        let (cb, dg) = if closing { (0.0, 0.0) } else { burdens(revenue, customers, fixed, s, sp) };
        let profit = sp.profit_weight * (revenue * (1.0 - sp.other_share) - fixed) / r0;
        let (pc, pd) = if closing { (0.0, 0.0) } else { (phi_store(cb, sp.cost_comfort, sp.cost_limit), phi_store(dg, sp.demand_comfort, sp.demand_limit)) };
        let utility = profit - pc - pd - action_cost;
        alts.push(StoreAlt { action: action.into(), revenue, customers, fixed, profit, cost_burden: cb, demand_gap: dg, phi_cost: pc, phi_demand: pd, action_cost, utility, prob: 0.0 });
    };
    for &a in allowed {
        match a {
            "KEEP" => push(a, obs.revenue, obs.customers, base_fixed, 0.0, false),
            "PRICE_UP" | "PRICE_DOWN" => {
                let f = if a == "PRICE_UP" { 1.0 + sp.price_step } else { 1.0 - sp.price_step };
                let c = obs.customers * f.powf(-sp.price_elasticity);
                push(a, c * ticket * f, c, base_fixed, sp.change_cost, false);
            }
            "HOURS_UP" | "HOURS_DOWN" => {
                let h = if a == "HOURS_UP" { sp.hours_step_min.min(1440.0 - hours) } else { -sp.hours_step_min.min(hours - 120.0) };
                if h.abs() < 1e-9 {
                    continue;
                }
                let g = 1.0 + h / hours * sp.hours_revenue_elasticity;
                let fixed = fixed_cost(s.rent, s.staff, s.wage, 1.0 + h / hours, support_pct, labor_index);
                push(a, obs.revenue * g, obs.customers * g, fixed, sp.change_cost, false);
            }
            "STAFF_UP" => {
                let fixed = fixed_cost(s.rent, s.staff + 1.0, s.wage, 1.0, support_pct, labor_index);
                let gain = obs.abandoned * ticket * sp.staff_recover_share;
                push(a, obs.revenue + gain, obs.customers + obs.abandoned * sp.staff_recover_share, fixed, sp.change_cost, false);
            }
            "STAFF_DOWN" => {
                if s.staff <= 1.0 {
                    continue;
                }
                let fixed = fixed_cost(s.rent, s.staff - 1.0, s.wage, 1.0, support_pct, labor_index);
                let k = 1.0 - sp.staff_down_loss;
                push(a, obs.revenue * k, obs.customers * k, fixed, sp.change_cost, false);
            }
            "SWITCH_CATEGORY" => {
                // 상대 기대: 자기 매출 × (가장 나은 다른 업종의 상점당 평균 ÷ 자기 업종 평균) × 전환 효율.
                // 자기 위치(입지·가시성)는 업종을 바꿔도 따라간다.
                let own = cats.get(&(s.class_ix, s.category.clone())).map(|v| v.0).unwrap_or(0.0);
                let best = cats.iter().filter(|((c, cat), _)| *c == s.class_ix && *cat != s.category).max_by(|a, b| a.1 .0.total_cmp(&b.1 .0).then(b.0 .1.cmp(&a.0 .1)));
                if let (Some((_, (rev_best, _, _))), true) = (best, own > 0.0) {
                    let k = (rev_best / own).min(sp.switch_max_ratio) * sp.switch_efficiency;
                    push(a, obs.revenue * k, obs.customers * k, base_fixed, sp.switch_cost, false);
                }
            }
            "CLOSE" => push(a, 0.0, 0.0, 0.0, sp.close_cost, true),
            _ => {}
        }
    }
    // 소프트맥스(C-205와 같은 규칙, τ = shop.tau). 같은 키 → 같은 선택.
    let tau = sp.tau.max(1e-3);
    let umax = alts.iter().map(|a| a.utility).fold(f64::NEG_INFINITY, f64::max);
    let z: f64 = alts.iter().map(|a| ((a.utility - umax) / tau).exp()).sum();
    for a in alts.iter_mut() {
        a.prob = ((a.utility - umax) / tau).exp() / z;
    }
    let u = rng.u("STORE", &s.id, month as u64, 0, 0);
    let mut acc = 0.0;
    let mut chosen = alts.len().saturating_sub(1);
    for (i, a) in alts.iter().enumerate() {
        acc += a.prob;
        if u < acc {
            chosen = i;
            break;
        }
    }
    StoreDecision { month, place: s.place, place_id: s.id.clone(), alts, chosen, rng_key: canonical_key(rng.seed, "STORE", &s.id, month as u64, 0, 0), obs: obs.clone(), cost_burden: cb_now, demand_gap: dg_now }
}

/// 고른 행동을 다음 달 상태에 반영한다.
pub fn apply_decision(s: &mut Store, d: &StoreDecision, cats: &BTreeMap<(usize, String), (f64, IntVec, u32)>, sp: &ShopParams) {
    let action = d.alts[d.chosen].action.as_str();
    let hours = (s.close_min - s.open_min).max(1.0);
    match action {
        "PRICE_UP" => s.price *= 1.0 + sp.price_step,
        "PRICE_DOWN" => s.price *= 1.0 - sp.price_step,
        "HOURS_UP" => s.close_min += sp.hours_step_min.min(1440.0 - hours),
        "HOURS_DOWN" => s.close_min -= sp.hours_step_min.min(hours - 120.0),
        "STAFF_UP" => {
            s.service_rate *= (s.staff + 1.0) / s.staff.max(1.0);
            s.staff += 1.0;
        }
        "STAFF_DOWN" => {
            s.service_rate *= (s.staff - 1.0).max(1.0) / s.staff.max(1.0);
            s.staff = (s.staff - 1.0).max(1.0);
        }
        "SWITCH_CATEGORY" => {
            let best = cats.iter().filter(|((c, cat), _)| *c == s.class_ix && *cat != s.category).max_by(|a, b| a.1 .0.total_cmp(&b.1 .0).then(b.0 .1.cmp(&a.0 .1)));
            if let Some(((_, cat), (_, offer, _))) = best {
                s.category = cat.clone();
                s.offer = *offer;
            }
        }
        "CLOSE" => s.closed = true,
        _ => {}
    }
}

/// 재무 값의 출처 표시(가정이면 그 사실과 계수 이름).
pub fn finance_label(stores: &[Store]) -> String {
    let n = stores.iter().filter(|s| s.finance_assumed).count();
    if n == 0 {
        "store_data".into()
    } else {
        format!("assumed:shop.rent_share,shop.labor_share,shop.other_share,shop.staff_default ({n}/{})", stores.len())
    }
}

/// 상태 요약 해시(결정성 확인용).
pub fn stores_digest(stores: &[Store]) -> String {
    hex(&Sha256::digest(serde_json::to_vec(stores).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_in_month_matches_python_rule() {
        // 02c I-204-c 테스트: 2026-11, v08 = 2 → weekday 21, weekend 7, event_day 2
        assert_eq!(days_in_month(2026, 11, 2).unwrap(), [21, 7, 2]);
        assert_eq!(days_in_month(2026, 11, 0).unwrap(), [21, 9, 0]);
        assert_eq!(days_in_month(2024, 2, 0).unwrap().iter().sum::<u32>(), 29);
        assert!(days_in_month(2026, 11, 10).is_err());
        assert_eq!(weekday_of(2026, 11, 7), 6, "2026-11-07은 토요일");
    }

    #[test]
    fn month_seed_differs_by_day_and_month() {
        let a = month_seed(1, 1, DayType::Weekday);
        assert_ne!(a, month_seed(1, 1, DayType::Weekend));
        assert_ne!(a, month_seed(1, 2, DayType::Weekday));
        assert_eq!(a, month_seed(1, 1, DayType::Weekday));
    }

    #[test]
    fn configurations_match_02c_table() {
        assert!(configuration_actions("static_stores").unwrap().is_empty());
        let pe = configuration_actions("policy_experiment").unwrap();
        assert!(!pe.iter().any(|a| a.starts_with("PRICE") || a.starts_with("HOURS")));
        assert_eq!(configuration_actions("store_adaptation").unwrap().len(), 9);
        assert!(configuration_actions("x").is_err());
    }
}
