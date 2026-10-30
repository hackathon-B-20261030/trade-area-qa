//! `parameters.yaml`의 계수를 모듈별 구조체로 한 번 읽는다(02d §2.4).

use mdsim_core::defs::Params;
use mdsim_core::Result;

#[derive(Clone, Debug)]
pub struct ReactParams {
    pub time_weight: f64,
    pub deadline_k: f64,
    pub deadline_horizon_min: f64,
    pub switch_cost: f64,
    /// 세운 계획을 버릴 때의 현상 유지 상수(util). ROUTE의 ABANDON에만 쓴다(S-E23).
    pub status_quo: f64,
}

#[derive(Clone, Debug)]
pub struct NoticeParams {
    pub b0: f64,
    pub bv: f64,
    pub ba: f64,
    pub bt: f64,
    pub brho: f64,
    pub t_ref_s: f64,
    pub w_peripheral: f64,
    pub trigger_min_interval_s: f64,
    pub trigger_min_offer: f64,
}

#[derive(Clone, Debug)]
pub struct LookParams {
    pub k_max: usize,
    pub unattended_mult: f64,
    pub footing_relief: f64,
    pub lookahead_s: f64,
    pub novelty_seen: f64,
    pub interrupt_m: f64,
}

#[derive(Clone, Debug)]
pub struct PerceptionParams {
    pub crowd_kappa: f64,
    pub night_factor: f64,
    pub night_light: f64,
    pub fresh_s: f64,
}

#[derive(Clone, Debug)]
pub struct CrowdParams {
    pub prior_density: f64,
    pub local_radius_m: f64,
    pub los_thresholds: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct LocoParams {
    pub scan_dirs: usize,
    pub tau_s: f64,
    pub time_gap_s: f64,
    pub body_radius_m: f64,
    pub companion_radius_add_m: f64,
    pub lookahead_m: f64,
    pub browse_factor: f64,
    pub fatigue_factor: f64,
    pub steps_factor: f64,
    pub tobler_k: f64,
    pub tobler_s0: f64,
    pub tobler_fmax: f64,
    pub arrive_m: f64,
    pub queue_spacing_m: f64,
    pub wall_clearance_m: f64,
    pub lane_width_m: f64,
}

#[derive(Clone, Debug)]
pub struct StateParams {
    pub climb_coef: f64,
    pub steps_coef: f64,
    pub recover_seated_per_min: f64,
    pub recover_standing_per_min: f64,
    pub rest_trigger: f64,
}

#[derive(Clone, Debug)]
pub struct PurchaseParams {
    pub band_mult: Vec<f64>,
    pub band_gain: Vec<f64>,
    pub dwell_no_buy_min: f64,
    pub dwell_buy_min: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct RouteParams {
    pub k: usize,
    pub penalty: f64,
    pub pace_time: f64,
    pub pace_exertion: f64,
    pub replan_margin: f64,
    pub crossing_wait_s: f64,
    /// DEFER로 기다리는 시간(분, S-E36).
    pub defer_min: f64,
}

#[derive(Clone, Debug)]
pub struct RestParams {
    pub seat_min: f64,
    pub max_bench_m: f64,
}

#[derive(Clone, Debug)]
pub struct VisitParams {
    pub slot_min: f64,
    pub theta_skip: f64,
    pub exit_walk_default_min: f64,
    pub expected_walk_m: f64,
    pub expected_spend_share: f64,
    pub stay_base_min: f64,
    pub stay_per_purpose_min: f64,
    pub stay_max_min: f64,
    pub mean_dwell_min: f64,
    pub myeongdong_way: f64,
    pub myeongdong_climb: f64,
    pub bus_wait_min: f64,
    pub taxi_wait_min: f64,
}

#[derive(Clone, Debug)]
pub struct BgParams {
    pub stops_min: usize,
    pub stops_max: usize,
    pub dwell_min_s: f64,
    pub dwell_max_s: f64,
    pub speed_mps: f64,
}

#[derive(Clone, Debug)]
pub struct RtParams {
    pub react: ReactParams,
    pub notice: NoticeParams,
    pub look: LookParams,
    pub perception: PerceptionParams,
    pub crowd: CrowdParams,
    pub loco: LocoParams,
    pub state: StateParams,
    pub purchase: PurchaseParams,
    pub route: RouteParams,
    pub rest: RestParams,
    pub visit: VisitParams,
    pub bg: BgParams,
    pub next_max_candidates: usize,
    pub explore: ExploreParams,
    /// 정의의 NEXT 맥락이 허용하는 행동(contexts.yaml). 없는 행동은 대안으로 만들지 않는다.
    pub next_explore: bool,
    pub next_watch: bool,
    pub next_rest_seat: bool,
    /// 정의의 REST 맥락이 유료 쉬기(REST_PAID)를 허용하는가(S-E53).
    pub rest_paid: bool,
}

/// EXPLORE(둘러보며 찾기, S-E25): 반경 [min, max] 안의 처음 가는 교차점까지 걷는다.
#[derive(Clone, Debug)]
pub struct ExploreParams {
    pub radius_min_m: f64,
    pub radius_max_m: f64,
    pub discovery_share: f64,
}

impl RtParams {
    pub fn load(p: &Params) -> Result<RtParams> {
        let f = |k: &str| p.f(k);
        Ok(RtParams {
            react: ReactParams { time_weight: f("react.time_weight")?, deadline_k: f("react.deadline_k")?, deadline_horizon_min: f("react.deadline_horizon_min")?, switch_cost: f("react.switch_cost")?, status_quo: f("react.status_quo")? },
            notice: NoticeParams {
                b0: f("notice.b0")?,
                bv: f("notice.bv")?,
                ba: f("notice.ba")?,
                bt: f("notice.bt")?,
                brho: f("notice.brho")?,
                t_ref_s: f("notice.t_ref_s")?,
                w_peripheral: f("notice.w_peripheral")?,
                trigger_min_interval_s: f("notice.trigger_min_interval_s")?,
                trigger_min_offer: f("notice.trigger_min_offer")?,
            },
            look: LookParams {
                k_max: f("look.k_max")?.max(0.0) as usize,
                unattended_mult: f("look.unattended_mult")?,
                footing_relief: f("look.footing_relief")?,
                lookahead_s: f("look.lookahead_s")?,
                novelty_seen: f("look.novelty_seen")?,
                interrupt_m: f("look.interrupt_m")?,
            },
            perception: PerceptionParams { crowd_kappa: f("perception.crowd_kappa")?, night_factor: f("perception.night_factor")?, night_light: f("perception.night_light")?, fresh_s: f("observe.fresh_s")? },
            crowd: CrowdParams { prior_density: f("crowd.prior_density")?, local_radius_m: f("crowd.local_radius_m")?, los_thresholds: p.v("crowd.los_thresholds")? },
            loco: LocoParams {
                scan_dirs: (f("loco.scan_dirs")?.max(3.0) as usize) | 1,
                tau_s: f("loco.tau_s")?,
                time_gap_s: f("loco.time_gap_s")?,
                body_radius_m: f("loco.body_radius_m")?,
                companion_radius_add_m: f("loco.companion_radius_add_m")?,
                lookahead_m: f("loco.lookahead_m")?,
                browse_factor: f("loco.browse_factor")?,
                fatigue_factor: f("loco.fatigue_factor")?,
                steps_factor: f("loco.steps_factor")?,
                tobler_k: f("loco.tobler_k")?,
                tobler_s0: f("loco.tobler_s0")?,
                tobler_fmax: f("loco.tobler_fmax")?,
                arrive_m: f("loco.arrive_m")?,
                queue_spacing_m: f("loco.queue_spacing_m")?,
                wall_clearance_m: f("loco.wall_clearance_m")?,
                lane_width_m: f("loco.lane_width_m")?,
            },
            state: StateParams {
                climb_coef: f("state.climb_coef")?,
                steps_coef: f("state.steps_coef")?,
                recover_seated_per_min: f("state.recover_seated_per_min")?,
                recover_standing_per_min: f("state.recover_standing_per_min")?,
                rest_trigger: f("state.rest_trigger")?,
            },
            purchase: PurchaseParams { band_mult: p.v("purchase.band_mult")?, band_gain: p.v("purchase.band_gain")?, dwell_no_buy_min: f("purchase.dwell_no_buy_min")?, dwell_buy_min: p.v("purchase.dwell_buy_min")? },
            route: RouteParams {
                k: f("route.k")?.max(1.0) as usize,
                penalty: f("route.penalty")?,
                pace_time: f("route.pace_time")?,
                pace_exertion: f("route.pace_exertion")?,
                replan_margin: f("route.replan_margin")?,
                crossing_wait_s: f("route.crossing_wait_s")?,
                defer_min: f("route.defer_min")?,
            },
            rest: RestParams { seat_min: f("rest.seat_min")?, max_bench_m: f("rest.max_bench_m")? },
            visit: VisitParams {
                slot_min: f("visit.slot_min")?,
                theta_skip: f("visit.theta_skip")?,
                exit_walk_default_min: f("visit.exit_walk_default_min")?,
                expected_walk_m: f("visit.expected_walk_m")?,
                expected_spend_share: f("visit.expected_spend_share")?,
                stay_base_min: f("visit.stay_base_min")?,
                stay_per_purpose_min: f("visit.stay_per_purpose_min")?,
                stay_max_min: f("visit.stay_max_min")?,
                mean_dwell_min: f("visit.mean_dwell_min")?,
                myeongdong_way: f("visit.myeongdong_way")?,
                myeongdong_climb: f("visit.myeongdong_climb")?,
                bus_wait_min: f("visit.bus_wait_min")?,
                taxi_wait_min: f("visit.taxi_wait_min")?,
            },
            bg: BgParams {
                stops_min: f("bg.stops_min")?.max(1.0) as usize,
                stops_max: f("bg.stops_max")?.max(1.0) as usize,
                dwell_min_s: f("bg.dwell_min_s")?,
                dwell_max_s: f("bg.dwell_max_s")?,
                speed_mps: f("bg.speed_mps")?,
            },
            next_max_candidates: f("next.max_candidates")?.max(1.0) as usize,
            explore: ExploreParams { radius_min_m: f("explore.radius_min_m")?, radius_max_m: f("explore.radius_max_m")?, discovery_share: f("explore.discovery_share")? },
            next_explore: true,
            next_watch: true,
            next_rest_seat: true,
            rest_paid: true,
        })
    }
}

/// Tobler 보정(02d §1.5). s = 경사(상승/거리, 오르막 +).
pub fn tobler(p: &LocoParams, s: f64) -> f64 {
    ((-p.tobler_k * (s + p.tobler_s0).abs()).exp() / (-p.tobler_k * p.tobler_s0).exp()).min(p.tobler_fmax)
}

/// Weidmann 기준식(검증용, 02d §1.5).
pub fn weidmann_speed(rho: f64) -> f64 {
    let (v_free, gamma, rho_max) = (1.34, 1.913, 5.4);
    if rho <= 1e-9 {
        return v_free;
    }
    (v_free * (1.0 - (-gamma * (1.0 / rho - 1.0 / rho_max)).exp())).max(0.0)
}
