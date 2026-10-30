//! 지각 — 시야, 알아차림, 시선 (C-221·C-214, 02d I-221-r, DR-221).
//!
//! ① 가시 집합은 기하로 결정적이다(거리·각·건물 가림·인파 감쇠, S-F6).
//! ② 알아차림은 C-214의 통과 확률 p를 틱 위험률로 바꿔 NOTICE 키로 판정한다(S-F19).
//! ③ 시선은 여섯 번째 결정 맥락 LOOK이고 같은 반응 함수를 쓴다(S-F4).

use crate::collective::Collective;
use crate::env::EnvState;
use crate::params::{NoticeParams, RtParams};
use crate::react::{react, Alt, Caps, Person, Reaction};
use crate::situation::{aggregate, Action, AltTarget, Choice, Exposure};
use crate::space::{PlaceKind, Space};
use crate::state::{Body, GazeTarget};
use mdsim_core::bundle::Profile;
use mdsim_core::defs::DimDef;
use mdsim_core::dims::{Dim, DimVec, ND, NI};
use mdsim_core::geom::{angle_between, V2};
use mdsim_core::rng::KeyRng;

/// 실효 시야 (초점 거리, 초점 각, 주변 거리, 주변 각).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fov {
    pub focal_range: f64,
    pub focal_angle_deg: f64,
    pub peri_range: f64,
    pub peri_angle_deg: f64,
}

pub fn effective_fov(prof: &Profile, b: &Body, env: &EnvState, local_density: f64, p: &RtParams, tick: u64, dt: f64) -> Fov {
    let pp = &p.perception;
    let light_t = ((env.light - pp.night_light) / (1.0 - pp.night_light).max(1e-9)).clamp(0.0, 1.0);
    let light_factor = pp.night_factor + (1.0 - pp.night_factor) * light_t;
    let crowd = (-pp.crowd_kappa * local_density.max(0.0)).exp();
    let hurry = (1.0 - b.minutes_left(tick, dt) / p.react.deadline_horizon_min.max(1.0)).clamp(0.0, 1.0);
    Fov {
        focal_range: prof.perc.focal_range_m * light_factor * crowd,
        focal_angle_deg: prof.perc.focal_angle_deg * (1.0 - 0.3 * b.fatigue.clamp(0.0, 1.0)) * (1.0 - 0.2 * hurry),
        peri_range: prof.perc.peripheral_range_m * crowd,
        peri_angle_deg: prof.perc.peripheral_angle_deg,
    }
}

/// C-214의 알아차림 함수 f(02d §1.5): 통과 한 번의 확률.
pub fn notice_pass_prob(np: &NoticeParams, visibility: f64, ad: f64, attention: f64, density: f64) -> f64 {
    let z = np.b0 + np.bv * visibility + np.ba * ad + np.bt * attention - np.brho * density;
    1.0 / (1.0 + (-z).exp())
}

/// 틱 확률 q = 1 − (1 − p)^(g·Δt / T_ref) (DR-221 ①).
pub fn notice_tick_prob(np: &NoticeParams, p_pass: f64, gaze_weight: f64, dt: f64) -> f64 {
    if gaze_weight <= 0.0 || p_pass <= 0.0 {
        return 0.0;
    }
    1.0 - (1.0 - p_pass.min(1.0 - 1e-12)).powf(gaze_weight * dt / np.t_ref_s.max(1e-9))
}

#[derive(Clone, Debug, Default)]
pub struct PerceptOut {
    /// (장소, 초점 안)
    pub visible: Vec<(u32, bool)>,
    pub notices: Vec<u32>,
    /// (장소, 영업 여부, 줄 길이)
    pub seen: Vec<(u32, bool, u32)>,
    pub seen_edges: Vec<(u32, f64)>,
    pub fov: Option<Fov>,
}

/// 한 몸의 지각(틱 t의 상태만 읽는다).
pub fn perceive(space: &Space, env: &EnvState, coll: &Collective, queue_len: &[u32], p: &RtParams, prof: &Profile, b: &Body, rng: &KeyRng, tick: u64, dt: f64) -> PerceptOut {
    let rho = coll.local_density.get(b.id as usize).copied().unwrap_or(0.0);
    let fov = effective_fov(prof, b, env, rho, p, tick, dt);
    let mut out = PerceptOut { fov: Some(fov), ..Default::default() };
    let range = fov.focal_range.max(fov.peri_range);
    let mut near = Vec::new();
    space.places_near(b.pos, range, &mut near);
    let prefix = rng.prefix("NOTICE", &b.entity);
    for &i in &near {
        let pl = &space.places[i as usize];
        let to = pl.anchor - b.pos;
        let d = to.len();
        let a = to.angle();
        let in_focal = d <= fov.focal_range && (d < 0.5 || angle_between(a, b.gaze) <= fov.focal_angle_deg.to_radians() / 2.0);
        let in_peri = d <= fov.peri_range && (d < 0.5 || angle_between(a, b.heading) <= fov.peri_angle_deg.to_radians() / 2.0);
        if !(in_focal || in_peri) {
            continue;
        }
        if !space.line_of_sight(b.pos, pl.anchor) {
            continue;
        }
        out.visible.push((i, in_focal));
        out.seen.push((i, env.open[i as usize], queue_len.get(i as usize).copied().unwrap_or(0)));
        if b.aware_of(i).is_none() {
            let pp = notice_pass_prob(&p.notice, pl.visibility, pl.ad, prof.attention, rho);
            let g = if in_focal { 1.0 } else { p.notice.w_peripheral };
            let q = notice_tick_prob(&p.notice, pp, g, dt);
            if prefix.u(tick, i as u64, 0) < q {
                out.notices.push(i);
            }
        }
    }
    // 본 혼잡: 지금 간선과 경로의 다음 간선(초점 거리 안)
    if let Some(e) = b.cur_edge {
        out.seen_edges.push((e, coll.edge_density[e as usize]));
    }
    if let Some(r) = &b.route {
        if let Some((ne, nfwd)) = r.next() {
            let es = &space.edges[ne as usize];
            let start = if nfwd { es.poly[0] } else { *es.poly.last().unwrap() };
            if start.dist(b.pos) <= fov.focal_range && space.line_of_sight(b.pos, start) {
                out.seen_edges.push((ne, coll.edge_density[ne as usize]));
            }
        }
    }
    out
}

/// 앞 lookahead_s초 경로의 요구(지금 간선 진행 방향 + 다음 간선).
pub fn ahead_demand(space: &Space, env: &EnvState, b: &Body, local_density: f64, dims: &[DimDef; ND], lookahead_s: f64) -> DimVec {
    let mut ex = Vec::new();
    let reach = b.base_speed.max(0.5) * lookahead_s;
    if let Some(r) = &b.route {
        let mut left = reach;
        for j in r.idx..r.edges.len() {
            if left <= 0.0 {
                break;
            }
            let (e, fwd) = r.edges[j];
            let es = &space.edges[e as usize];
            let mut d = *es.demand_of(fwd, env);
            d[Dim::Walk.ix()] = 0.0;
            d[Dim::Crowd.ix()] = local_density;
            let take = left.min(es.len);
            ex.push(Exposure { demand: d, amount: 1.0, weight: take });
            left -= take;
        }
    }
    if ex.is_empty() {
        let mut d = [0.0; ND];
        d[Dim::Crowd.ix()] = local_density;
        ex.push(Exposure { demand: d, amount: 1.0, weight: 1.0 });
    }
    aggregate(&ex, dims)
}

/// LOOK 상황: 앞길, 발밑, 알아차렸고 보이는 대상(시계성 ÷ 거리 상위 K, S-F4).
pub fn look_situation(space: &Space, env: &EnvState, p: &RtParams, dims: &[DimDef; ND], b: &Body, visible: &[(u32, bool)], local_density: f64, tick: u64, dt: f64, head_turn_deg: f64) -> Vec<Choice> {
    let lp = &p.look;
    let ahead = ahead_demand(space, env, b, local_density, dims, lp.lookahead_s);
    let dt_min = lp.lookahead_s / 60.0;
    let mk = |action: Action, target: AltTarget, demand: DimVec, offer: [f64; NI], blocked: bool| Choice { action, target, alt: Alt { demand, offer, purpose: 0.0, achieve: 1.0, dt_min, cost_krw: 0.0, blocked }, path: None, dwell_min: 0.0 };
    let mut out = vec![mk(Action::Path, AltTarget::None, ahead, [0.0; NI], false)];
    // 주의가 길을 떠나면 커지는 차원: 사람·차량과 부딪힐 위험, 발밑 위험(S-F4)
    const WATCH_DIMS: [Dim; 4] = [Dim::Crowd, Dim::Safe, Dim::Step, Dim::Fall];
    let mut foot = ahead;
    for d in [Dim::Step, Dim::Fall] {
        foot[d.ix()] *= lp.footing_relief;
    }
    for d in [Dim::Crowd, Dim::Safe] {
        foot[d.ix()] *= 1.0 + lp.unattended_mult;
    }
    out.push(mk(Action::Footing, AltTarget::None, foot, [0.0; NI], false));
    let mut cands: Vec<(f64, u32)> = visible
        .iter()
        .filter(|(i, _)| b.aware_of(*i).is_some() && !matches!(space.places[*i as usize].kind, PlaceKind::Bench))
        .map(|&(i, _)| {
            let pl = &space.places[i as usize];
            (pl.visibility / pl.anchor.dist(b.pos).max(1.0), i)
        })
        .collect();
    cands.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
    cands.truncate(lp.k_max);
    let recent = (10.0 / dt).round() as u64;
    for (_, i) in cands {
        let pl = &space.places[i as usize];
        let mut d = ahead;
        for dim in WATCH_DIMS {
            d[dim.ix()] *= 1.0 + lp.unattended_mult;
        }
        // 그 가게 정면을 보면 그 구간 광고 자극에 그대로 노출된다(S-R25 ③)
        d[Dim::Sense.ix()] = d[Dim::Sense.ix()].max(pl.ad);
        let fresh = b.aware_of(i).map(|a| tick.saturating_sub(a.first_tick) <= recent && !a.known_before).unwrap_or(false);
        let novelty = if fresh { 1.0 } else { lp.novelty_seen };
        let offer: [f64; NI] = std::array::from_fn(|k| pl.offer[k] * novelty);
        let blocked = angle_between((pl.anchor - b.pos).angle(), b.heading) > head_turn_deg.to_radians();
        out.push(mk(Action::LookAt, AltTarget::Place(i), d, offer, blocked));
    }
    out
}

pub fn look_decide(choices: &[Choice], prof: &Profile, b: &Body, dims: &[DimDef; ND], p: &RtParams, rng: &KeyRng, tick: u64, caps: &Caps) -> Reaction {
    let alts: Vec<Alt> = choices.iter().map(|c| c.alt.clone()).collect();
    let person = Person { sens: &prof.sens, interest: &prof.interest, risk: prof.risk, punctuality: prof.punctuality, tau: prof.tau };
    let u = rng.u("LOOK", &b.entity, tick, 0, 0);
    react(&person, caps, &b.sat, 1e6, &alts, dims, &p.react, u)
}

/// 시선 대상의 방향.
pub fn gaze_dir(space: &Space, b: &Body, t: GazeTarget) -> f64 {
    match t {
        GazeTarget::Path | GazeTarget::Footing => b.heading,
        GazeTarget::Place(i) => (space.places[i as usize].anchor - b.pos).angle(),
    }
}

/// 진행 방향 앞 interrupt_m 안에 사람이 있는가(시선 예외, S-F4).
pub fn collision_risk(coll: &Collective, b: &Body, interrupt_m: f64) -> bool {
    let mut buf = Vec::new();
    coll.neighbors(b.pos, interrupt_m, &mut buf);
    buf.iter().any(|&j| {
        if j == b.id || !coll.on_street[j as usize] {
            return false;
        }
        let to: V2 = coll.pos[j as usize] - b.pos;
        to.len() > 1e-6 && angle_between(to.angle(), b.heading) < std::f64::consts::FRAC_PI_3
    })
}
