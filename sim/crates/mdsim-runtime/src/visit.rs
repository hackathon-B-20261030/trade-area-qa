//! 방문지 선택과 도착 계획 (C-209·C-216의 런타임 쪽, 02d I-209-r), 배경 보행자 계획(S-F18).

use crate::params::RtParams;
use crate::react::{react, Alt, Caps, Person, Reaction};
use crate::space::{PlaceKind, Space};
use crate::state::BgItinerary;
use mdsim_core::bundle::{Bundle, Mode, PortalKind, Profile};
use mdsim_core::defs::{Defs, LimitKind};
use mdsim_core::dims::{Dim, IntVec, ND, NI};
use mdsim_core::rng::{poisson, KeyRng};
use mdsim_core::scenario::Scenario;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SpawnKind {
    Domestic { profile: u32, replicate: u16 },
    Background { itinerary: BgItinerary },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    pub tick: u64,
    pub kind: SpawnKind,
    pub portal: u32,
    pub mode: Mode,
    pub deadline_tick: u64,
    pub entity: String,
}

impl Spawn {
    fn order_key(&self) -> (u64, u8, String) {
        (self.tick, if matches!(self.kind, SpawnKind::Domestic { .. }) { 0 } else { 1 }, self.entity.clone())
    }
}

/// 몸 ID = (도착 틱, 종류, 개체 ID) 정렬 순번(S-E4).
pub fn sort_spawns(mut v: Vec<Spawn>) -> Vec<Spawn> {
    v.sort_by_key(|a| a.order_key());
    v
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum VisitKind {
    Myeongdong { slot_start_min: f64, mode: Mode },
    Competitor { id: String, mode: Mode },
    Skip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisitAltRecord {
    pub kind: VisitKind,
    pub prob: f64,
    pub base_min: f64,
    pub wait_min: f64,
    pub exit_walk_min: f64,
    pub portal: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct VisitDecision {
    pub profile: u32,
    pub reaction: Reaction,
    pub alts: Vec<VisitAltRecord>,
    pub alt_specs: Vec<Alt>,
}

/// 수단별 대기(분). 지하철은 배차 간격 × 배율 ÷ 2다. 배율(#9)은 명동 역의 배차에만 곱하고 경쟁 상권은 1이다(S-E48).
fn mode_wait_min(mode: Mode, headway_min: f64, headway_mult: f64, p: &RtParams) -> f64 {
    match mode {
        Mode::Subway => headway_min * headway_mult / 2.0,
        Mode::Bus => p.visit.bus_wait_min,
        Mode::Taxi => p.visit.taxi_wait_min,
        Mode::Walk => 0.0,
    }
}

fn portal_kind_of(mode: Mode) -> PortalKind {
    match mode {
        Mode::Subway => PortalKind::Subway,
        Mode::Bus | Mode::Taxi => PortalKind::Bus,
        Mode::Walk => PortalKind::Boundary,
    }
}

/// 그 수단으로 도착할 출입구(수단별 출입구 중 첫 목적 장소에 가장 가까운 곳; 도보는 출발지 방향).
pub fn pick_portal(space: &Space, b: &Bundle, prof: &Profile, mode: Mode) -> Option<u32> {
    // 명동 안 숙소 출발지(Q:…)는 그 출입구에서 걸어 나온다(02c §2.4 (1) origin_id).
    if mode == Mode::Walk {
        if let Some(i) = space.portal_ix(&prof.origin_id) {
            return Some(i as u32);
        }
    }
    let want = portal_kind_of(mode);
    let usable = |p: &crate::space::PortalInst| p.groups.is_empty() || p.groups.contains(&prof.group);
    let mut cands: Vec<usize> = space.portals.iter().enumerate().filter(|(_, p)| p.kind == want && usable(p)).map(|(i, _)| i).collect();
    if cands.is_empty() {
        // 버스·택시는 경계 출입구로, 그것도 없으면 아무 출입구
        cands = space.portals.iter().enumerate().filter(|(_, p)| matches!(p.kind, PortalKind::Boundary | PortalKind::Bus | PortalKind::Subway) && usable(p)).map(|(i, _)| i).collect();
    }
    if cands.is_empty() {
        return None;
    }
    if mode == Mode::Walk {
        // 출발지 좌표(origins, Rust 선택 입력)가 없으면 인물 ID의 고정 해시로 경계 출입구를 고른다(02c B8, S-E26).
        let Some(origin) = b.origins.iter().find(|o| o.id == prof.origin_id).map(|o| o.pos) else {
            return Some(cands[stable_ix(&prof.persona_id, cands.len())] as u32);
        };
        let center = space.graph.node_pos.iter().fold(mdsim_core::geom::V2::ZERO, |a, p| a + *p) * (1.0 / space.graph.node_pos.len().max(1) as f64);
        let dir = (origin - center).angle();
        return cands.iter().copied().min_by(|&i, &j| {
            let ai = mdsim_core::geom::angle_between((space.portals[i].pos - center).angle(), dir);
            let aj = mdsim_core::geom::angle_between((space.portals[j].pos - center).angle(), dir);
            ai.total_cmp(&aj).then(i.cmp(&j))
        }).map(|i| i as u32);
    }
    let target = prof.purposes.iter().filter_map(|pu| pu.target_place_id.as_deref()).find_map(|id| space.place_ix(id)).map(|i| space.places[i].anchor);
    match target {
        Some(t) => cands.iter().copied().min_by(|&i, &j| space.portals[i].pos.dist(t).total_cmp(&space.portals[j].pos.dist(t)).then(i.cmp(&j))).map(|i| i as u32),
        None => Some(cands[0] as u32),
    }
}

/// 문자열의 고정 해시(FNV-1a 64)로 고른 0..n 인덱스.
pub fn stable_ix(s: &str, n: usize) -> usize {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h % n.max(1) as u64) as usize
}

/// 출입구에서 첫 목적 장소까지 걷는 시간(분).
pub fn exit_walk_min(space: &Space, prof: &Profile, portal: u32, p: &RtParams) -> f64 {
    let po = &space.portals[portal as usize];
    let target = prof.purposes.iter().filter_map(|pu| pu.target_place_id.as_deref()).find_map(|id| space.place_ix(id));
    match target {
        Some(ti) => {
            let pl = &space.places[ti];
            match space.graph.shortest(po.node, pl.node_near, &|e, _| space.edges[e as usize].len) {
                Some(path) => (path.length + po.pos.dist(space.graph.node_pos[po.node as usize]) + pl.anchor.dist(space.graph.node_pos[pl.node_near as usize])) / prof.base_speed_mps.max(0.3) / 60.0,
                None => p.visit.exit_walk_default_min,
            }
        }
        None => p.visit.exit_walk_default_min,
    }
}

/// 슬롯(분)에 영업 중인 장소의 업종별 평균 제공과 평균 가격.
pub fn myeongdong_offer(space: &Space, scn: &Scenario, minute: f64) -> (IntVec, [f64; NI]) {
    let mut offer = [0.0; NI];
    let mut price_sum = [0.0; NI];
    let mut price_n = [0.0; NI];
    let mut n = 0.0;
    for (i, pl) in space.places.iter().enumerate() {
        if matches!(pl.kind, PlaceKind::Bench) || !crate::env::place_open_at(scn, space, i, minute) {
            continue;
        }
        n += 1.0;
        for k in 0..NI {
            offer[k] += pl.offer[k];
            if pl.offer[k] > 0.0 && pl.price > 0.0 {
                price_sum[k] += pl.price;
                price_n[k] += 1.0;
            }
        }
    }
    if n > 0.0 {
        for o in offer.iter_mut() {
            *o /= n;
        }
    }
    // 업종 평균이 0이 되지 않도록 관심이 있는 업종의 상위 제공을 쓴다: 장소 수가 많을수록 평균은 낮아지므로
    // 정규화는 최댓값 기준으로 한다.
    let mx = offer.iter().cloned().fold(0.0, f64::max);
    if mx > 0.0 {
        for o in offer.iter_mut() {
            *o /= mx;
        }
    }
    let mut price = [0.0; NI];
    for k in 0..NI {
        price[k] = if price_n[k] > 0.0 { price_sum[k] / price_n[k] } else { 0.0 };
    }
    (offer, price)
}

pub fn prior_density(b: &Bundle, p: &RtParams, minute: f64) -> f64 {
    match &b.prior_crowd {
        Some(v) => v[((minute / 10.0).floor() as usize).min(143)],
        None => p.crowd.prior_density,
    }
}

/// 인물 하나의 방문지 선택 상황(C-209). 대안: 슬롯 × 수단(명동), 경쟁 상권별 하나, 안 감.
pub fn visit_situation(space: &Space, b: &Bundle, scn: &Scenario, p: &RtParams, prof: &Profile) -> (Vec<Alt>, Vec<VisitAltRecord>) {
    let (w0, w1) = scn.window_min().unwrap();
    let day = scn.day();
    let mut alts = Vec::new();
    let mut recs = Vec::new();
    let slot = p.visit.slot_min.max(1.0);
    let n_purposes = prof.purposes.len() as f64;
    let stay = (p.visit.stay_base_min + p.visit.stay_per_purpose_min * n_purposes).min(p.visit.stay_max_min);
    let interest_total: f64 = prof.interest.iter().sum::<f64>().max(1e-9);
    for av in prof.availability.iter().filter(|a| a.day_type == day) {
        let mut s = (av.start_min.max(w0) / slot).ceil() * slot;
        while s + 1e-9 < av.end_min.min(w1) {
            let (offer, price) = myeongdong_offer(space, scn, s + slot / 2.0);
            let expected_spend: f64 = (0..NI).map(|k| prof.interest[k] / interest_total * price[k]).sum::<f64>() * p.visit.expected_spend_share * 2.0;
            let purpose: f64 = prof
                .purposes
                .iter()
                .map(|pu| {
                    let ach = match pu.target_place_id.as_deref().and_then(|id| space.place_ix(id)) {
                        Some(i) => {
                            if crate::env::place_open_at(scn, space, i, s + slot / 2.0) {
                                1.0
                            } else {
                                0.5
                            }
                        }
                        None => 1.0,
                    };
                    pu.value * ach
                })
                .sum();
            for mode in Mode::ALL {
                let Some(acc) = b.access.iter().find(|a| a.origin_id == prof.origin_id && a.destination == "myeongdong" && a.mode == mode) else { continue };
                let Some(portal) = pick_portal(space, b, prof, mode) else { continue };
                let headway = space.portals[portal as usize].headway_min.max(if mode == Mode::Subway { 1.0 } else { 0.0 });
                let wait = mode_wait_min(mode, headway, scn.headway_multiplier, p);
                let walk = exit_walk_min(space, prof, portal, p);
                let mut demand = [0.0; ND];
                demand[Dim::Walk.ix()] = p.visit.expected_walk_m;
                demand[Dim::Crowd.ix()] = prior_density(b, p, s);
                demand[Dim::Way.ix()] = p.visit.myeongdong_way;
                demand[Dim::Climb.ix()] = p.visit.myeongdong_climb;
                let dt = acc.base_min + wait + walk;
                alts.push(Alt {
                    demand,
                    offer,
                    purpose,
                    achieve: 1.0,
                    dt_min: dt,
                    cost_krw: acc.fare_krw + expected_spend,
                    blocked: false,
                });
                recs.push(VisitAltRecord { kind: VisitKind::Myeongdong { slot_start_min: s, mode }, prob: 0.0, base_min: acc.base_min, wait_min: wait, exit_walk_min: walk, portal: Some(portal) });
                let _ = stay;
            }
            s += slot;
        }
    }
    for c in &b.competitors {
        let best = b.access.iter().filter(|a| a.origin_id == prof.origin_id && a.destination == c.id).min_by(|x, y| (x.base_min + mode_wait_min(x.mode, 6.0, 1.0, p)).total_cmp(&(y.base_min + mode_wait_min(y.mode, 6.0, 1.0, p))).then(x.mode.cmp(&y.mode)));
        let Some(acc) = best else { continue };
        let wait = mode_wait_min(acc.mode, 6.0, 1.0, p);
        let price_ref: f64 = 20_000.0;
        let expected_spend: f64 = (0..NI).map(|k| prof.interest[k] / interest_total * price_ref).sum::<f64>() * p.visit.expected_spend_share * c.price_level;
        let mut d = [0.0; ND];
        d[Dim::Walk.ix()] = p.visit.expected_walk_m;
        d[Dim::Crowd.ix()] = c.crowd_level;
        d[Dim::Way.ix()] = p.visit.myeongdong_way;
        alts.push(Alt { demand: d, offer: c.interest, purpose: 0.0, achieve: 1.0, dt_min: acc.base_min + wait, cost_krw: acc.fare_krw + expected_spend, blocked: false });
        recs.push(VisitAltRecord { kind: VisitKind::Competitor { id: c.id.clone(), mode: acc.mode }, prob: 0.0, base_min: acc.base_min, wait_min: wait, exit_walk_min: 0.0, portal: None });
    }
    // 안 감: 효용 = theta_skip(요구·제공 없음)
    alts.push(Alt { demand: [0.0; ND], offer: [0.0; NI], purpose: p.visit.theta_skip, achieve: 1.0, dt_min: 0.0, cost_krw: 0.0, blocked: false });
    recs.push(VisitAltRecord { kind: VisitKind::Skip, prob: 0.0, base_min: 0.0, wait_min: 0.0, exit_walk_min: 0.0, portal: None });
    (alts, recs)
}

/// 방문지 선택에서의 유효 능력: 시간 한계는 그날 가장 긴 가용 창(분), 돈은 예산 그대로.
/// D-TIME은 프로필 능력 열이 없고 런타임이 정한다(02c §2.4 (1)).
pub fn visit_caps(defs: &Defs, prof: &Profile, day: mdsim_core::scenario::DayType) -> Caps {
    let mut hard = [false; ND];
    for d in Dim::ALL {
        hard[d.ix()] = defs.dims[d.ix()].limit == LimitKind::Hard;
    }
    let (mut comfort, mut limit) = (prof.comfort, prof.limit);
    let win = prof.availability.iter().filter(|a| a.day_type == day).map(|a| a.end_min - a.start_min).fold(0.0, f64::max);
    comfort[Dim::Time.ix()] = 0.0;
    limit[Dim::Time.ix()] = win.max(1e-6);
    Caps { comfort, limit, hard }
}

pub fn person_of(prof: &Profile) -> Person<'_> {
    Person { sens: &prof.sens, interest: &prof.interest, risk: prof.risk, punctuality: prof.punctuality, tau: prof.tau }
}

/// 모든 인물의 방문지 선택(정확한 확률, 키 VISIT·persona·step 0).
pub fn visit_decisions(defs: &Defs, space: &Space, b: &Bundle, scn: &Scenario, p: &RtParams, rng: &KeyRng) -> Vec<VisitDecision> {
    use rayon::prelude::*;
    b.profiles
        .par_iter()
        .enumerate()
        .map(|(i, prof)| {
            let (alts, mut recs) = visit_situation(space, b, scn, p, prof);
            let caps = visit_caps(defs, prof, scn.day());
            let u = rng.u("VISIT", &prof.persona_id, 0, 0, 0);
            let r = react(&person_of(prof), &caps, &[0.0; NI], 24.0 * 60.0, &alts, &defs.dims, &p.react, u);
            for (k, rec) in recs.iter_mut().enumerate() {
                rec.prob = r.probs[k];
            }
            VisitDecision { profile: i as u32, reaction: r, alts: recs, alt_specs: alts }
        })
        .collect()
}

/// 인물을 `chunk`명씩 나눠 방문지 선택을 하고, 묶음마다 `on_chunk`(기록 표 쓰기)를 부른 뒤
/// 항별 기여·대안 명세를 비워 메모리를 줄인다(6.5만 명 × 대안 수십 개, 02d §1.9). 확률·기록은 남는다.
#[allow(clippy::too_many_arguments)]
pub fn visit_decisions_streamed(defs: &Defs, space: &Space, b: &Bundle, scn: &Scenario, p: &RtParams, rng: &KeyRng, chunk: usize, on_chunk: &mut dyn FnMut(&[VisitDecision]) -> mdsim_core::Result<()>) -> mdsim_core::Result<Vec<VisitDecision>> {
    use rayon::prelude::*;
    let mut all = Vec::with_capacity(b.profiles.len());
    let n = b.profiles.len();
    let mut start = 0;
    while start < n {
        let end = (start + chunk.max(1)).min(n);
        let mut part: Vec<VisitDecision> = (start..end)
            .into_par_iter()
            .map(|i| {
                let prof = &b.profiles[i];
                let (alts, mut recs) = visit_situation(space, b, scn, p, prof);
                let caps = visit_caps(defs, prof, scn.day());
                let u = rng.u("VISIT", &prof.persona_id, 0, 0, 0);
                let r = react(&person_of(prof), &caps, &[0.0; NI], 24.0 * 60.0, &alts, &defs.dims, &p.react, u);
                for (k, rec) in recs.iter_mut().enumerate() {
                    rec.prob = r.probs[k];
                }
                VisitDecision { profile: i as u32, reaction: r, alts: recs, alt_specs: alts }
            })
            .collect();
        on_chunk(&part)?;
        for d in part.iter_mut() {
            d.reaction.terms = Vec::new();
            d.alt_specs = Vec::new();
        }
        all.extend(part);
        start = end;
    }
    Ok(all)
}

/// 명동 방문 확률 합(배율 1 기준).
pub fn myeongdong_prob_sum(dec: &[VisitDecision]) -> f64 {
    dec.iter().map(|d| d.alts.iter().filter(|a| matches!(a.kind, VisitKind::Myeongdong { .. })).map(|a| a.prob).sum::<f64>()).sum()
}

/// 대표 비율(S-E11): 시나리오 값, 없으면 목표 동시 인원 × 창 길이 ÷ 평균 체류 ÷ 명동 확률 합.
pub fn representation_ratio(dec: &[VisitDecision], scn: &Scenario, p: &RtParams) -> f64 {
    if let Some(r) = scn.representation_ratio {
        return r;
    }
    let (w0, w1) = scn.window_min().unwrap();
    let target_arrivals = scn.target_peak_concurrency * ((w1 - w0) / p.visit.mean_dwell_min).max(1.0);
    let s = myeongdong_prob_sum(dec);
    if s <= 0.0 {
        0.0
    } else {
        target_arrivals / s
    }
}

/// 도착 계획(DR-220 ③: 몸 수 = Poisson(P × 대표 비율 × 배율)).
pub fn plan_arrivals(space: &Space, b: &Bundle, scn: &Scenario, p: &RtParams, rng: &KeyRng, dec: &[VisitDecision], ratio: f64) -> Vec<Spawn> {
    let (w0, w1) = scn.window_s();
    let dt = scn.dt_s;
    let mut out = Vec::new();
    for d in dec {
        let prof = &b.profiles[d.profile as usize];
        let mult = match prof.group {
            mdsim_core::bundle::Group::Domestic => scn.demand_multiplier,
            mdsim_core::bundle::Group::Tourist => scn.foreign_inflow_multiplier,
        };
        let mut replicate: u16 = 0;
        for (k, a) in d.alts.iter().enumerate() {
            let VisitKind::Myeongdong { slot_start_min, mode } = a.kind else { continue };
            let lambda = a.prob * ratio * mult;
            let n = poisson(lambda, rng.u("SPAWN", &prof.persona_id, 0, k as u64, 0));
            for _ in 0..n {
                let ent = mdsim_core::rng::body_entity(&prof.persona_id, replicate);
                let avail_end = prof.availability.iter().filter(|av| av.day_type == scn.day() && av.start_min <= slot_start_min && slot_start_min < av.end_min).map(|av| av.end_min).fold(f64::NAN, f64::max);
                // 도착은 슬롯과 가용 창이 겹치는 [슬롯 시작, 끝) 안이다(창 끝이 슬롯 가운데에 걸려도, S-E49).
                let (s0, s1) = (slot_start_min * 60.0, (slot_start_min + p.visit.slot_min).min(if avail_end.is_nan() { f64::INFINITY } else { avail_end }) * 60.0);
                let mut t = s0 + rng.u("ARRIVAL", &ent, 0, 0, 0) * (s1 - s0).max(0.0);
                let portal = a.portal.unwrap_or(0);
                if mode == Mode::Subway {
                    // 다음 열차 + 출구 보행 1~2분. 끝을 넘으면 끝 안에 닿는 마지막 열차, 그것도 슬롯 앞이면 균등 시각 그대로.
                    let hw = space.portals[portal as usize].headway_min.max(1.0) * scn.headway_multiplier * 60.0;
                    let walk = 60.0 + rng.u("ARRIVAL", &ent, 0, 1, 0) * 60.0;
                    let next = (t / hw).ceil() * hw + walk;
                    let last = ((s1 - walk) / hw).ceil() * hw - hw + walk;
                    t = if next < s1 {
                        next
                    } else if last >= s0 {
                        last
                    } else {
                        t
                    };
                }
                if t < w0 || t >= w1 {
                    replicate += 1;
                    continue;
                }
                let tick = ((t - w0) / dt).floor() as u64;
                let stay = (p.visit.stay_base_min + p.visit.stay_per_purpose_min * prof.purposes.len() as f64).min(p.visit.stay_max_min);
                let end_s = (t + stay * 60.0).min(if avail_end.is_nan() { f64::MAX } else { avail_end * 60.0 });
                let deadline = ((end_s - w0) / dt).ceil().max(tick as f64 + 1.0) as u64;
                out.push(Spawn { tick, kind: SpawnKind::Domestic { profile: d.profile, replicate }, portal, mode, deadline_tick: deadline, entity: ent });
                replicate += 1;
            }
        }
    }
    out
}

/// 배경 보행자 계획(S-F18): 시간대·출입구 종류별 외생 인원 × #11.
pub fn plan_background(space: &Space, b: &Bundle, scn: &Scenario, p: &RtParams, rng: &KeyRng) -> Vec<Spawn> {
    let (w0, w1) = scn.window_s();
    let dt = scn.dt_s;
    let stops_pool: Vec<u32> = space.places.iter().enumerate().filter(|(_, pl)| matches!(pl.kind, PlaceKind::Shop | PlaceKind::Visit | PlaceKind::Stall)).map(|(i, _)| i as u32).collect();
    let mut out = Vec::new();
    if stops_pool.is_empty() {
        return out;
    }
    let weights: Vec<f64> = stops_pool.iter().map(|&i| space.places[i as usize].visibility.max(0.01)).collect();
    let wsum: f64 = weights.iter().sum();
    let pick = |u: f64| -> u32 {
        let mut acc = 0.0;
        for (k, w) in weights.iter().enumerate() {
            acc += w / wsum;
            if u < acc {
                return stops_pool[k];
            }
        }
        *stops_pool.last().unwrap()
    };
    for row in &b.background {
        let (h0, h1) = (row.hour as f64 * 3600.0, (row.hour + 1) as f64 * 3600.0);
        let (a, z) = (h0.max(w0), h1.min(w1));
        if z <= a {
            continue;
        }
        let frac = (z - a) / 3600.0;
        let flow_ent = format!("BGFLOW:{}:{}", row.hour * 60, row.portal_kind.as_str());
        let n = poisson(row.count * frac * scn.foreign_inflow_multiplier, rng.u("BACKGROUND", &flow_ent, 0, 0, 0));
        let portals: Vec<u32> = {
            let v: Vec<u32> = space.portals.iter().enumerate().filter(|(_, po)| po.kind == row.portal_kind).map(|(i, _)| i as u32).collect();
            if v.is_empty() { (0..space.portals.len() as u32).collect() } else { v }
        };
        if portals.is_empty() {
            continue;
        }
        for j in 0..n {
            let ent = format!("BG:{}:{}:{j}", row.hour * 60, row.portal_kind.as_str());
            let t = a + rng.u("BACKGROUND", &ent, 0, 0, 0) * (z - a);
            let portal = portals[((rng.u("BACKGROUND", &ent, 0, 1, 0) * portals.len() as f64).floor() as usize).min(portals.len() - 1)];
            let k = p.bg.stops_min + ((rng.u("BACKGROUND", &ent, 0, 2, 0) * (p.bg.stops_max - p.bg.stops_min + 1) as f64).floor() as usize).min(p.bg.stops_max - p.bg.stops_min);
            let mut stops: Vec<u32> = Vec::new();
            let mut dwell = Vec::new();
            for s in 0..k {
                let mut cand = pick(rng.u("BACKGROUND", &ent, 0, 10 + s as u64, 0));
                if stops.last() == Some(&cand) {
                    cand = pick(rng.u("BACKGROUND", &ent, 0, 10 + s as u64, 1));
                }
                stops.push(cand);
                dwell.push(p.bg.dwell_min_s + rng.u("BACKGROUND", &ent, 0, 30 + s as u64, 0) * (p.bg.dwell_max_s - p.bg.dwell_min_s));
            }
            let tick = ((t - w0) / dt).floor() as u64;
            let deadline = ((w1 - w0) / dt).ceil() as u64;
            out.push(Spawn { tick, kind: SpawnKind::Background { itinerary: BgItinerary { stops, dwell_s: dwell, next: 0, exit_portal: portal } }, portal, mode: Mode::Walk, deadline_tick: deadline, entity: ent });
        }
    }
    out
}
