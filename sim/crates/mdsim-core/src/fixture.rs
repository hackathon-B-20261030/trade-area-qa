//! 계약 형식의 합성 고정 자료 (02d I-228-b, S-E19).
//!
//! Python 번들(02c §2)이 나오기 전까지 모든 단위를 검증하려고 만든 작은 가상 상권이다. 실제 명동이 아니다.
//! 가로·세로 3블록(블록 약 68m), 보행자 도로·생활도로·골목·계단, 동쪽 간선도로(보도 3m), 경사 10% 언덕,
//! 장소 50여 곳(벤치·노점·행사·숙박 포함), 출입구 4곳, 내국인 400명·관광객 60명, 경쟁 상권 2곳을 담는다.
//!
//! 쓰는 형식은 02c §2(번들 manifest, 입력 표, 시나리오 파일)와 같고, 정의 파일은 02c I-201-a·I-203-a·I-207-a의
//! v1 내용을 Rust가 읽는 필드만 옮긴 것이다. 폴더 구성은 저장소와 같다:
//! `ontology/v1/*.yaml`, `data/reaction-inputs/fixture/*.parquet`, `data/reaction-bundles/<bundle_id>/`.

// 고정 자료 표는 튜플 목록으로 적는다(읽기 쉬운 자료 표).
#![allow(clippy::type_complexity)]

use crate::bundle::{BundleManifest, FileEntry, ScenarioEntry, SCHEMA_VERSION};
use crate::defs::{defs_sha256, hex, REQUIRED_PARAMS};
use crate::dims::{Dim, Interest};
use crate::error::{Result, SimError};
use crate::geom::{dist_point_segment, V2};
use crate::pq::{ColData, FieldKind, Scalar, Table};
use crate::rng::{rep_seed, KeyRng};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DIMENSIONS_YAML: &str = r#"version: "1.0.0"
dimensions:
  - {id: D-CLIMB, name: 오르막 부하, unit: "%", aggregate: max, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-FALL, name: 균형·낙상 위험, unit: "1", aggregate: max, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-STEP, name: 단차, unit: cm, aggregate: max, limit: hard_for_wheels, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-WALK, name: 보행 거리, unit: m, aggregate: sum, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-TIME, name: 시간, unit: 분, aggregate: sum, limit: hard, cost_curve: linear, cost: {exponent: 1.0, overflow_slope: 0.0, overflow_penalty: 0.0}, label: assumed}
  - {id: D-MONEY, name: 금전, unit: 원, aggregate: sum, limit: hard, cost_curve: linear, cost: {exponent: 1.0, overflow_slope: 0.0, overflow_penalty: 0.0}, label: assumed}
  - {id: D-WAY, name: 길찾기·언어 부담, unit: "1", aggregate: max, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-SENSE, name: 감각 자극 부하, unit: "1", aggregate: time_weighted_mean, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-CROWD, name: 혼잡, unit: "인/㎡", aggregate: time_weighted_mean, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
  - {id: D-SAFE, name: 차량 혼재 위험, unit: "1", aggregate: max, limit: soft, cost_curve: phi, cost: {exponent: 2.0, overflow_slope: 4.0, overflow_penalty: 1.0}, label: assumed}
store_dimensions:
  - {id: D-COST, name: 비용 부담, aggregate: max, limit: soft}
  - {id: D-DEMAND, name: 수요 부족, aggregate: max, limit: soft}
"#;

pub const INTERESTS_YAML: &str = r#"version: "1.0.0"
interests:
  - {id: A-FOOD, name: 음식}
  - {id: A-BEAUTY, name: 뷰티}
  - {id: A-FASHION, name: 패션}
  - {id: A-SOUVENIR, name: 기념품}
  - {id: A-CULTURE, name: 문화}
  - {id: A-FAITH, name: 신앙}
  - {id: A-NATURE, name: 자연}
"#;

/// 02c I-203-a의 v1 클래스(Rust가 읽는 필드). inputs·actors·variables는 Python 전용이라 뺐다.
pub const CLASSES_YAML: &str = r#"version: "1.0.0"
classes:
  - id: walk_segment
    name: 보행 구간
    attributes:
      grade_pct: {type: continuous, unit: "%", range: [-60, 60], role: u, default: 0, directional: true}
      length_m: {type: continuous, unit: m, range: [0.1, 3000], role: w}
      width_m: {type: continuous, unit: m, range: [0.5, 80], role: u}
      walk_mode: {type: categorical, values: [full, sidewalks], role: w, default: full}
      sidewalk_width_m: {type: continuous, unit: m, range: [0, 15], role: u, default: 0}
      road_type: {type: categorical, values: [pedestrian, footway, steps, path, corridor, residential, service, living_street, primary, secondary, tertiary, primary_link, secondary_link], role: w, default: footway}
      vehicle_mixed: {type: boolean, role: w, default: 0}
      ped_zone: {type: boolean, role: w, default: 0}
      is_crossing: {type: boolean, role: w, default: 0}
      carriage_share: {type: continuous, unit: "1", range: [0, 0.95], role: w, default: 0}
      ad_intensity: {type: continuous, unit: "1", range: [0, 1], role: u, default: 0}
      way_difficulty: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0.4}
      safe_demand: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      surface: {type: categorical, values: [dry, wet, ice], role: w, default: dry}
      layer: {type: integer, unit: "1", range: [-5, 5], role: w, default: 0}
    press:
      D-CLIMB: "max(grade_pct, 0)"
      D-FALL: "clamp(max(-grade_pct, 0) / $fall.ref_down_pct * pick(surface, $fall.surface_factor.dry, $fall.surface_factor.wet, $fall.surface_factor.ice), 0, 1)"
      D-WALK: "length_m"
      D-SAFE: "vehicle_active * safe_demand"
      D-SENSE: "ad_intensity"
      D-WAY: "way_difficulty"
    affords: [PROCEED, PACE, DETOUR]
  - id: steps
    name: 계단
    parent: walk_segment
    attributes:
      riser_cm: {type: continuous, unit: cm, range: [1, 40], role: w, default: 15}
      steps_count: {type: integer, unit: "1", range: [1, 500], role: w, default: 1}
      conveying: {type: boolean, role: w, default: 0}
    press:
      D-STEP: "riser_cm * (1 - conveying)"
      D-CLIMB: "max(grade_pct, 0) * (1 - conveying)"
      D-FALL: "clamp(($fall.stairs_down_base * min(max(-grade_pct, 0), 1) + max(-grade_pct, 0) / $fall.ref_down_pct * pick(surface, $fall.surface_factor.dry, $fall.surface_factor.wet, $fall.surface_factor.ice)) * (1 - conveying), 0, 1)"
  - id: rest_facility
    name: 휴식 시설
    attributes:
      seats: {type: integer, unit: 석, range: [0, 500], role: u, default: 3}
      kind: {type: categorical, values: [bench, plaza], role: w, default: bench}
    affords: [REST_SEAT]
  - id: place
    name: 장소
    abstract: true
    attributes:
      category: {type: categorical, values: [restaurant, cafe, fast_food, bar, bakery, convenience, clothes, department_store, cosmetics, beauty_service, souvenir, electronics, culture_venue, attraction, viewpoint, worship, park, lodging, bench, stall, event], role: w}
      open_min: {type: integer, unit: 분, range: [0, 2880], role: u, default: 0}
      close_min: {type: integer, unit: 분, range: [0, 2880], role: u, default: 1440}
      day_types: {type: categorical_set, values: [weekday, weekend, event_day], role: w}
      visibility: {type: continuous, unit: "1", range: [0, 1], role: u, default: 0.5}
      fame: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0.2}
      foreign_label: {type: boolean, role: w, default: 0}
      floor: {type: integer, unit: 층, range: [-5, 100], role: w, default: 1}
      layer: {type: integer, unit: "1", range: [-5, 5], role: w, default: 0}
      offer_food: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      offer_beauty: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      offer_fashion: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      offer_souvenir: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      offer_culture: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      offer_faith: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
      offer_nature: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0}
    press:
      D-WAY: "(1 - foreign_label) * $way.no_foreign_label"
    offers:
      A-FOOD: "offer_food"
      A-BEAUTY: "offer_beauty"
      A-FASHION: "offer_fashion"
      A-SOUVENIR: "offer_souvenir"
      A-CULTURE: "offer_culture"
      A-FAITH: "offer_faith"
      A-NATURE: "offer_nature"
    affords: [GO_TO]
  - id: store
    name: 상점
    parent: place
    attributes:
      price_level_krw: {type: continuous, unit: 원, range: [0, 10000000], role: u, default: 0}
      discount_pct: {type: continuous, unit: "%", range: [0, 90], role: u, default: 0}
      service_rate_per_min: {type: continuous, unit: 인/분, range: [0, 100], role: w, default: 0}
      capacity: {type: integer, unit: 인, range: [0, 100000], role: w, default: 20}
    press:
      D-MONEY: "price_level_krw * (1 - discount_pct / 100)"
    affords: [GO_TO, BUY]
  - {id: cafe, name: 카페, parent: store, affords: [GO_TO, BUY, REST_PAID]}
  - id: stall
    name: 노점
    parent: store
    attributes:
      footprint_m2: {type: continuous, unit: ㎡, range: [0.5, 20], role: w, default: 3}
    affords: [GO_TO, BUY]
  - id: event
    name: 행사
    parent: place
    attributes:
      capacity: {type: integer, unit: 인, range: [0, 100000], role: w, default: 100}
      sense: {type: continuous, unit: "1", range: [0, 1], role: w, default: 0.6}
    press:
      D-SENSE: "sense * event_active"
    offers:
      A-FOOD: "offer_food * event_active"
      A-BEAUTY: "offer_beauty * event_active"
      A-FASHION: "offer_fashion * event_active"
      A-SOUVENIR: "offer_souvenir * event_active"
      A-CULTURE: "offer_culture * event_active"
      A-FAITH: "offer_faith * event_active"
      A-NATURE: "offer_nature * event_active"
    affords: [GO_TO, WATCH]
  - {id: landmark, name: 명소, parent: place, affords: [GO_TO]}
  - {id: lodging, name: 숙박, parent: place, affords: [LEAVE]}
"#;

/// 02c I-207-a의 v1 결정 맥락.
pub const CONTEXTS_YAML: &str = r#"version: "1.0.0"
contexts:
  - {id: VISIT, actor: person, mechanism: VISIT, actions: [VISIT_NOW, GO_ELSEWHERE, VISIT_LATER, SKIP], always: [SKIP]}
  - {id: NEXT, actor: person, mechanism: NEXT, actions: [GO_TO, WATCH, EXPLORE, LEAVE], always: [LEAVE]}
  - {id: ROUTE, actor: person, mechanism: ROUTE, actions: [PROCEED, DETOUR, PACE, SWITCH_MODE, DEFER, ABANDON, RETARGET], always: [PROCEED, PACE, DEFER, ABANDON]}
  - {id: REST, actor: person, mechanism: REST, actions: [REST_SEAT, REST_PAID, CONTINUE, LEAVE], always: [CONTINUE, LEAVE]}
  - {id: PURCHASE, actor: person, mechanism: PURCHASE, actions: [BUY, NO_BUY], always: [NO_BUY]}
  - {id: LOOK, actor: person, mechanism: LOOK, actions: [PATH, FOOTING, LOOK_AT], always: [PATH]}
  - {id: STORE, actor: store, mechanism: STORE, actions: [KEEP, PRICE_UP, PRICE_DOWN, HOURS_UP, HOURS_DOWN, STAFF_UP, STAFF_DOWN, SWITCH_CATEGORY, CLOSE], always: [KEEP]}
"#;

/// 02d 소유 계수(02d §2.4). 값의 근거는 02d 표에 있다.
pub fn fixture_params() -> Vec<(&'static str, Vec<f64>)> {
    let s = |v: f64| vec![v];
    vec![
        ("react.time_weight", s(0.04)),
        ("react.deadline_k", s(1.0)),
        ("react.deadline_horizon_min", s(60.0)),
        ("react.switch_cost", s(0.3)),
        ("react.status_quo", s(1.0)),
        ("notice.b0", s(-1.0)),
        ("notice.bv", s(2.5)),
        ("notice.ba", s(1.0)),
        ("notice.bt", s(1.5)),
        ("notice.brho", s(0.5)),
        ("notice.t_ref_s", s(8.0)),
        ("notice.w_peripheral", s(0.3)),
        ("notice.trigger_min_interval_s", s(20.0)),
        ("notice.trigger_min_offer", s(0.15)),
        ("look.k_max", s(8.0)),
        ("look.unattended_mult", s(0.5)),
        ("look.footing_relief", s(0.5)),
        ("look.lookahead_s", s(4.0)),
        ("look.novelty_seen", s(0.3)),
        ("look.interrupt_m", s(2.0)),
        ("perception.crowd_kappa", s(0.15)),
        ("perception.night_factor", s(0.6)),
        ("perception.night_light", s(0.6)),
        ("observe.fresh_s", s(120.0)),
        ("crowd.prior_density", s(0.15)),
        ("crowd.local_radius_m", s(2.0)),
        ("crowd.los_thresholds", vec![0.31, 0.43, 0.72, 1.08, 2.17]),
        ("loco.scan_dirs", s(21.0)),
        ("loco.tau_s", s(0.5)),
        ("loco.time_gap_s", s(1.0)),
        ("loco.body_radius_m", s(0.25)),
        ("loco.companion_radius_add_m", s(0.15)),
        ("loco.lookahead_m", s(3.0)),
        ("loco.browse_factor", s(0.55)),
        ("loco.fatigue_factor", s(0.3)),
        ("loco.steps_factor", s(0.6)),
        ("loco.tobler_k", s(3.5)),
        ("loco.tobler_s0", s(0.05)),
        ("loco.tobler_fmax", s(1.15)),
        ("loco.arrive_m", s(1.0)),
        ("loco.queue_spacing_m", s(0.6)),
        ("loco.wall_clearance_m", s(0.2)),
        ("loco.lane_width_m", s(3.0)),
        ("loco.core_strip_m", s(1.2)),
        ("state.climb_coef", s(1.0)),
        ("state.steps_coef", s(0.5)),
        ("state.recover_seated_per_min", s(0.08)),
        ("state.recover_standing_per_min", s(0.02)),
        ("state.rest_trigger", s(0.8)),
        ("purchase.band_mult", vec![0.6, 1.0, 1.8]),
        ("purchase.band_gain", vec![0.6, 1.0, 1.3]),
        ("purchase.dwell_no_buy_min", s(4.0)),
        ("purchase.dwell_buy_min", vec![10.0, 15.0, 25.0]),
        ("route.k", s(3.0)),
        ("route.penalty", s(1.6)),
        ("route.pace_time", s(1.3)),
        ("route.pace_exertion", s(0.7)),
        ("route.replan_margin", s(0.3)),
        ("route.crossing_wait_s", s(30.0)),
        ("route.defer_min", s(2.0)),
        ("rest.seat_min", s(8.0)),
        ("rest.max_bench_m", s(80.0)),
        ("visit.slot_min", s(10.0)),
        ("visit.theta_skip", s(0.0)),
        ("visit.exit_walk_default_min", s(3.0)),
        ("visit.expected_walk_m", s(1500.0)),
        ("visit.expected_spend_share", s(0.5)),
        ("visit.stay_base_min", s(40.0)),
        ("visit.stay_per_purpose_min", s(25.0)),
        ("visit.stay_max_min", s(180.0)),
        ("visit.mean_dwell_min", s(75.0)),
        ("visit.myeongdong_way", s(0.4)),
        ("visit.myeongdong_climb", s(4.0)),
        ("visit.bus_wait_min", s(6.0)),
        ("visit.taxi_wait_min", s(4.0)),
        ("bg.stops_min", s(2.0)),
        ("bg.stops_max", s(5.0)),
        ("bg.dwell_min_s", s(180.0)),
        ("bg.dwell_max_s", s(600.0)),
        ("bg.speed_mps", s(1.2)),
        ("next.max_candidates", s(25.0)),
        ("explore.radius_min_m", s(40.0)),
        ("explore.radius_max_m", s(120.0)),
        ("explore.discovery_share", s(0.5)),
        // 상점 월 반응(02d I-211-r). 월 고리(months ≥ 1)에서만 요구한다.
        ("shop.rent_share", s(0.25)),
        ("shop.labor_share", s(0.30)),
        ("shop.other_share", s(0.45)),
        ("shop.staff_default", s(3.0)),
        ("shop.min_revenue_krw", s(300000.0)),
        ("shop.cost_comfort", s(0.6)),
        ("shop.cost_limit", s(1.0)),
        ("shop.demand_comfort", s(1.0)),
        ("shop.demand_limit", s(1.5)),
        ("shop.price_step", s(0.05)),
        ("shop.price_elasticity", s(1.2)),
        ("shop.hours_step_min", s(60.0)),
        ("shop.hours_revenue_elasticity", s(0.5)),
        ("shop.staff_recover_share", s(0.5)),
        ("shop.staff_down_loss", s(0.05)),
        ("shop.switch_efficiency", s(0.8)),
        ("shop.switch_cost", s(2.5)),
        ("shop.close_cost", s(3.0)),
        ("shop.change_cost", s(1.75)),
        ("shop.profit_weight", s(10.0)),
        ("shop.revenue_memory", s(0.7)),
        ("shop.switch_max_ratio", s(1.5)),
        ("shop.evidence_customers", s(30.0)),
        ("shop.tau", s(0.5)),
    ]
}

/// 02c 소유인데 v1 클래스 식이 쓰는 계수(02c §1.10의 값).
const PARAMS_02C: &str = r#"  fall.ref_down_pct: {value: 20, unit: "%", label: assumed, source: "02c §1.10", owner: 02c}
  fall.surface_factor: {value: {dry: 1.0, wet: 1.6, ice: 3.0}, unit: "1", label: assumed, source: "02c §1.10", owner: 02c}
  fall.stairs_down_base: {value: 0.35, unit: "1", label: assumed, source: "02c §1.10", owner: 02c}
  way.no_foreign_label: {value: 0.5, unit: "1", label: assumed, source: "02c §1.10", owner: 02c}
  competitor.price_ref_krw: {value: 30000, unit: 원, label: assumed, source: "02c §1.10", owner: 02c}
  segment.ped_zone_name_pattern: {value: "^명동(\\d+[가-힣]?)?길$", unit: "-", label: assumed, source: "02c §1.10 (Rust는 읽지 않음)", owner: 02c}
  capacity.climb.comfort_pct: {value: [[19, 8.0], [30, 8.0], [60, 6.2], [75, 5.3], [90, 4.4]], unit: "%", label: assumed, source: "02c §1.10 (Rust는 읽지 않음)", owner: 02c}
"#;

pub fn parameters_yaml() -> String {
    let mut out = String::from("version: \"1.0.0\"\nparameters:\n");
    for (k, v) in fixture_params() {
        let list = matches!(k, "crowd.los_thresholds" | "purchase.band_mult" | "purchase.band_gain" | "purchase.dwell_buy_min");
        let value = if v.len() == 1 && !list { format!("{}", v[0]) } else { format!("[{}]", v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(", ")) };
        let (label, source) = if k == "crowd.los_thresholds" { ("measured", "Fruin 1971 보행 서비스 수준(근사 경계)") } else { ("assumed", "02d §2.4 기본값(가정)") };
        out.push_str(&format!("  {k}: {{value: {value}, unit: \"-\", label: {label}, source: \"{source}\", owner: 02d}}\n"));
    }
    out.push_str(PARAMS_02C);
    out
}

/// 시나리오 파일(02c §2.4 (13)). `seed`는 seed_r(base_seed, rep).
#[allow(clippy::too_many_arguments)]
fn scenario_yaml(id: &str, day_type: &str, window: (&str, &str), seed: u64, target: f64, ped_only: &str, events: &str, headway_mult: f64, trace: u32, months: u32, configuration: &str) -> String {
    format!(
        "id: {id}\nschema_version: 1\ndate: 2026-11-07\nday_type: {day_type}\nwindow: {{start: \"{}\", end: \"{}\"}}\ndt_s: 0.5\nrender_every_ticks: 2\nseed: {seed}\ntarget_peak_concurrency: {target}\nrepresentation_ratio: null\npedestrian_only: {ped_only}\nrent_support_pct: 0\nevents: {events}\nheadway_multiplier: {headway_mult}\ndemand_multiplier: 1.0\nforeign_inflow_multiplier: 1.0\nlabor_cost_index: 1.0\ninteractions: true\ntrace_bodies: {trace}\nconfiguration: {configuration}\nmonths: {months}\nevent_days_per_month: 0\nstatic_applied: {{preset: null, v02_width_delta_m: 0.0, v03_stall_scale: 1.0, v04_added_benches: 0, v05_discount_pct: null, v06_hours_extension_h: 0, v12_income_multiplier: 1.0, v13_competitor_access_scale: 1.0, v15_price_scale: 1.0, v16_visibility_level: base}}\n",
        window.0, window.1
    )
}

pub const FIXTURE_BASE_SEED: u64 = 20261031;

// ---------- 공간 ----------

struct Street {
    pts: Vec<(f64, f64)>,
    road_type: &'static str,
    class: &'static str,
    width: f64,
    walk_mode: &'static str,
    sw: f64,
    vehicle_mixed: bool,
    ad: f64,
}

fn elev(x: f64, z: f64) -> f64 {
    let _ = x;
    20.0 + 0.10 * (z - 100.0).max(0.0)
}

fn streets() -> Vec<Street> {
    let st = |pts: Vec<(f64, f64)>, road_type, class, width, walk_mode, sw, vehicle_mixed, ad| Street { pts, road_type, class, width, walk_mode, sw, vehicle_mixed, ad };
    vec![
        st(vec![(0.0, 0.0), (68.0, 0.0), (136.0, 0.0), (210.0, 0.0)], "pedestrian", "walk_segment", 8.0, "full", 0.0, false, 0.7),
        st(vec![(-40.0, 68.0), (0.0, 68.0), (68.0, 68.0), (102.0, 68.0), (136.0, 68.0), (210.0, 68.0)], "residential", "walk_segment", 6.5, "full", 0.0, true, 0.3),
        st(vec![(0.0, 136.0), (68.0, 136.0), (102.0, 136.0), (136.0, 136.0), (170.0, 136.0), (210.0, 136.0)], "pedestrian", "walk_segment", 8.0, "full", 0.0, false, 0.5),
        st(vec![(0.0, 204.0), (68.0, 204.0), (136.0, 204.0), (210.0, 204.0)], "residential", "walk_segment", 6.5, "full", 0.0, true, 0.1),
        st(vec![(0.0, 0.0), (0.0, 68.0), (0.0, 136.0), (0.0, 204.0)], "residential", "walk_segment", 6.5, "full", 0.0, true, 0.2),
        st(vec![(68.0, 0.0), (68.0, 68.0), (68.0, 136.0), (68.0, 204.0)], "pedestrian", "walk_segment", 8.0, "full", 0.0, false, 0.6),
        st(vec![(136.0, 0.0), (136.0, 68.0), (136.0, 136.0), (136.0, 204.0)], "residential", "walk_segment", 6.5, "full", 0.0, true, 0.3),
        st(vec![(210.0, 0.0), (210.0, 34.0), (210.0, 68.0), (210.0, 102.0), (210.0, 136.0), (210.0, 170.0), (210.0, 204.0)], "primary", "walk_segment", 26.0, "sidewalks", 3.0, false, 0.4),
        st(vec![(102.0, 68.0), (102.0, 102.0)], "footway", "walk_segment", 3.5, "full", 0.0, false, 0.2),
        st(vec![(102.0, 102.0), (102.0, 136.0)], "steps", "steps", 3.5, "full", 0.0, false, 0.1),
    ]
}

pub struct FixtureNode {
    pub id: i64,
    pub pos: V2,
    pub kind: &'static str,
    pub signal: bool,
}

pub struct FixtureEdge {
    pub id: String,
    pub a: i64,
    pub b: i64,
    pub street: usize,
    pub poly: Vec<V2>,
}

fn key_of(p: (f64, f64)) -> (i64, i64) {
    ((p.0 * 10.0).round() as i64, (p.1 * 10.0).round() as i64)
}

fn node_id(id: i64) -> String {
    format!("N:{id}")
}

fn build_graph() -> (Vec<FixtureNode>, Vec<FixtureEdge>, Vec<Street>) {
    let streets = streets();
    let mut ids: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for (si, s) in streets.iter().enumerate() {
        let mut prev: Option<i64> = None;
        for &p in &s.pts {
            let k = key_of(p);
            let id = *ids.entry(k).or_insert_with(|| {
                let id = 1 + nodes.len() as i64;
                nodes.push(FixtureNode { id, pos: V2::new(p.0, p.1), kind: "junction", signal: false });
                id
            });
            if let Some(a) = prev {
                let pa = nodes[(a - 1) as usize].pos;
                let pb = nodes[(id - 1) as usize].pos;
                let seq = edges.iter().filter(|e: &&FixtureEdge| e.street == si).count();
                edges.push(FixtureEdge { id: format!("E:w{}:{seq}", 7000 + si), a, b: id, street: si, poly: vec![pa, pb] });
            }
            prev = Some(id);
        }
    }
    for n in &mut nodes {
        let p = (n.pos.x, n.pos.z);
        if p == (210.0, 102.0) {
            n.kind = "crossing";
            n.signal = true;
        } else if [(210.0, 34.0), (210.0, 170.0), (-40.0, 68.0), (170.0, 136.0)].contains(&p) {
            n.kind = "portal";
        }
    }
    (nodes, edges, streets)
}

struct Building {
    id: i64,
    rect: (f64, f64, f64, f64),
    height: f64,
}

fn buildings() -> Vec<Building> {
    // (x0, x1, z0, z1) 블록 경계와 경계 도로의 반폭
    let hw = |kind: &str| match kind {
        "ped" => 4.0,
        "res" => 3.25,
        "art" => 13.0,
        "alley" => 1.75,
        _ => 3.25,
    };
    let mut out = Vec::new();
    let xs = [(0.0, 68.0, "res", "ped"), (68.0, 136.0, "ped", "res"), (136.0, 210.0, "res", "art")];
    let zs = [(0.0, 68.0, "ped", "res"), (68.0, 136.0, "res", "ped"), (136.0, 204.0, "ped", "res")];
    let mut id = 500;
    for (zi, &(z0, z1, zt, zb)) in zs.iter().enumerate() {
        for (xi, &(x0, x1, xl, xr)) in xs.iter().enumerate() {
            let rect = (x0 + hw(xl) + 1.0, x1 - hw(xr) - 1.0, z0 + hw(zt) + 1.0, z1 - hw(zb) - 1.0);
            let height = 12.0 + 6.0 * ((zi * 3 + xi) % 5) as f64;
            if zi == 1 && xi == 1 {
                // 골목(x=102)이 가르는 가운데 블록
                out.push(Building { id, rect: (rect.0, 102.0 - hw("alley") - 1.0, rect.2, rect.3), height });
                id += 1;
                out.push(Building { id, rect: (102.0 + hw("alley") + 1.0, rect.1, rect.2, rect.3), height: height + 8.0 });
                id += 1;
            } else {
                out.push(Building { id, rect, height });
                id += 1;
            }
        }
    }
    out
}

fn rect_poly(r: (f64, f64, f64, f64)) -> Vec<V2> {
    vec![V2::new(r.0, r.2), V2::new(r.1, r.2), V2::new(r.1, r.3), V2::new(r.0, r.3)]
}

pub struct FixturePlace {
    pub id: String,
    pub class: &'static str,
    pub category: &'static str,
    pub name: String,
    pub pos: V2,
    pub anchor: V2,
    pub edge: String,
    pub price: Option<f64>,
    pub discount: f64,
    pub open: f64,
    pub close: f64,
    pub vis: f64,
    pub rate: Option<f64>,
    pub cap: Option<f64>,
    pub offer: [f64; 7],
}

fn nearest_edge(edges: &[FixtureEdge], p: V2) -> (String, f64) {
    let mut best = (edges[0].id.clone(), f64::INFINITY);
    for e in edges {
        let (d, _) = dist_point_segment(p, e.poly[0], e.poly[1]);
        if d < best.1 {
            best = (e.id.clone(), d);
        }
    }
    best
}

/// 업종 어휘(02c I-204-b)의 클래스·제공 강도·기본값.
fn category_spec(category: &str) -> (&'static str, [f64; 7]) {
    match category {
        "restaurant" => ("store", [0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        "cafe" => ("cafe", [0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        "clothes" => ("store", [0.0, 0.0, 0.8, 0.1, 0.0, 0.0, 0.0]),
        "cosmetics" => ("store", [0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0]),
        "souvenir" => ("store", [0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0]),
        "culture_venue" => ("store", [0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0]),
        "worship" => ("landmark", [0.0, 0.0, 0.0, 0.0, 0.3, 0.8, 0.0]),
        "park" => ("landmark", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5]),
        "stall" => ("stall", [0.6, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0]),
        "event" => ("event", [0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0]),
        "lodging" => ("lodging", [0.0; 7]),
        "bench" => ("rest_facility", [0.0; 7]),
        _ => ("store", [0.0; 7]),
    }
}

fn places(edges: &[FixtureEdge], streets: &[Street], blds: &[Building]) -> Vec<FixturePlace> {
    // category, price, open, close, rate, cap
    let cycle: [(&str, f64, f64, f64, f64, f64); 6] = [
        ("restaurant", 12_000.0, 660.0, 1320.0, 0.5, 20.0),
        ("clothes", 45_000.0, 630.0, 1320.0, 2.0, 25.0),
        ("cafe", 6_000.0, 540.0, 1320.0, 1.0, 15.0),
        ("cosmetics", 25_000.0, 630.0, 1320.0, 2.0, 25.0),
        ("souvenir", 10_000.0, 630.0, 1320.0, 2.0, 20.0),
        ("restaurant", 9_000.0, 660.0, 1320.0, 0.6, 18.0),
    ];
    let edge_of = |id: &str| edges.iter().find(|e| e.id == id).unwrap();
    let mut out = Vec::new();
    let mut k = 0usize;
    let mut id = 1000;
    for b in blds {
        let (x0, x1, z0, z1) = b.rect;
        // 네 면의 가운데 두 곳씩(길에 면한 면만)
        let faces = [
            (V2::new(x0, z0), V2::new(x1, z0), V2::new(0.0, -1.0)),
            (V2::new(x0, z1), V2::new(x1, z1), V2::new(0.0, 1.0)),
            (V2::new(x0, z0), V2::new(x0, z1), V2::new(-1.0, 0.0)),
            (V2::new(x1, z0), V2::new(x1, z1), V2::new(1.0, 0.0)),
        ];
        for (fa, fb, out_n) in faces {
            for t in [0.33, 0.67] {
                let pos = fa.lerp(fb, t);
                let anchor = pos + out_n * 1.5;
                let (edge, d) = nearest_edge(edges, anchor);
                let s = &streets[edge_of(&edge).street];
                if d > s.width / 2.0 + 0.01 || s.road_type == "steps" {
                    continue; // 길에 면하지 않은 면
                }
                let (category, price, open, close, rate, cap) = cycle[k % cycle.len()];
                let vis = (0.35 + 0.5 * s.ad + if k.is_multiple_of(4) { 0.1 } else { 0.0 }).min(1.0);
                let discount = if k.is_multiple_of(5) { 10.0 } else { 0.0 };
                // 한 곳은 15시에 닫아 영업 종료 사건을 만든다.
                let close = if k == 7 { 900.0 } else { close };
                let (class, offer) = category_spec(category);
                out.push(FixturePlace { id: format!("O:n{id}"), class, category, name: format!("{category} {id}"), pos, anchor, edge, price: Some(price), discount, open, close, vis, rate: Some(rate), cap: Some(cap), offer });
                id += 1;
                k += 1;
            }
        }
    }
    // 특별 장소
    let special: Vec<(&str, &str, &str, V2, V2, Option<f64>, f64, f64, f64, Option<f64>, Option<f64>)> = vec![
        ("O:w2001", "culture_venue", "합성 극장", V2::new(30.0, 141.0), V2::new(30.0, 139.0), Some(30_000.0), 720.0, 1260.0, 0.85, Some(5.0), Some(100.0)),
        ("O:w2002", "worship", "합성 성당", V2::new(175.0, 199.0), V2::new(175.0, 201.0), None, 360.0, 1260.0, 0.9, None, None),
        ("O:w2003", "park", "합성 쉼터", V2::new(-25.0, 64.0), V2::new(-25.0, 66.0), None, 0.0, 1440.0, 0.6, None, None),
        ("O:w2004", "lodging", "합성 호텔", V2::new(170.0, 131.0), V2::new(170.0, 132.5), None, 0.0, 1440.0, 0.7, None, None),
    ];
    for (id, category, name, pos, anchor, price, open, close, vis, rate, cap) in special {
        let (edge, _) = nearest_edge(edges, anchor);
        let (class, offer) = category_spec(category);
        out.push(FixturePlace { id: id.to_string(), class, category, name: name.to_string(), pos, anchor, edge, price, discount: 0.0, open, close, vis, rate, cap, offer });
    }
    // 벤치 4
    for (i, p) in [V2::new(34.0, 2.5), V2::new(102.0, 2.5), V2::new(68.0 + 2.5, 100.0), V2::new(34.0, 138.5)].iter().enumerate() {
        let (edge, _) = nearest_edge(edges, *p);
        out.push(FixturePlace { id: format!("R:{:04}", i + 1), class: "rest_facility", category: "bench", name: format!("벤치 {}", i + 1), pos: *p, anchor: *p, edge, price: None, discount: 0.0, open: 0.0, close: 1440.0, vis: 0.4, rate: None, cap: None, offer: [0.0; 7] });
    }
    // 노점 6 (16:00~23:00)
    for (i, p) in [V2::new(20.0, -2.5), V2::new(50.0, 2.5), V2::new(90.0, -2.5), V2::new(120.0, 2.5), V2::new(70.5, 30.0), V2::new(65.5, 160.0)].iter().enumerate() {
        let (edge, _) = nearest_edge(edges, *p);
        let (class, offer) = category_spec("stall");
        out.push(FixturePlace { id: format!("L:{:04}", i + 1), class, category: "stall", name: format!("노점 {}", i + 1), pos: *p, anchor: *p, edge, price: Some(5_000.0), discount: 0.0, open: 960.0, close: 1380.0, vis: 0.85, rate: Some(1.0), cap: Some(2.0), offer });
    }
    // 행사 1 (시간은 시나리오 events)
    let ev = V2::new(80.0, 133.0);
    let (edge, _) = nearest_edge(edges, ev);
    let (class, offer) = category_spec("event");
    out.push(FixturePlace { id: "V:0001".into(), class, category: "event", name: "거리 공연".into(), pos: ev, anchor: ev, edge, price: None, discount: 0.0, open: 0.0, close: 1440.0, vis: 0.95, rate: None, cap: Some(60.0), offer });
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn place_offset(edges: &[FixtureEdge], p: &FixturePlace) -> f64 {
    let e = edges.iter().find(|e| e.id == p.edge).unwrap();
    let (_, t) = dist_point_segment(p.anchor, e.poly[0], e.poly[1]);
    t * e.poly[0].dist(e.poly[1])
}

fn place_side(edges: &[FixtureEdge], p: &FixturePlace) -> String {
    let e = edges.iter().find(|e| e.id == p.edge).unwrap();
    let dir = e.poly[1] - e.poly[0];
    if dir.cross(p.anchor - e.poly[0]) >= 0.0 {
        "left".into()
    } else {
        "right".into()
    }
}

// ---------- 쓰기 ----------

pub struct FixturePaths {
    /// 저장소 루트처럼 쓰는 폴더(manifest 경로의 기준).
    pub root: PathBuf,
    pub defs: PathBuf,
    /// 14:00~17:00 주말(반복 0·1).
    pub bundle: PathBuf,
    pub scenario: PathBuf,
    pub scenario_r1: PathBuf,
    /// 14:00~15:00 주말, 목표 300명.
    pub bundle_quick: PathBuf,
    pub scenario_quick: PathBuf,
    /// 15:30~16:30 행사일: 노점 개장(16시), 보행 전용 시작(16시), 행사(15:45~16:15).
    pub bundle_variant: PathBuf,
    pub scenario_variant: PathBuf,
    /// 월 고리: 평일 본, 3개월, store_adaptation, 창 14:00~15:00, 목표 600명.
    pub bundle_monthly: PathBuf,
    pub scenario_monthly: PathBuf,
}

fn io_err(e: impl std::fmt::Display) -> SimError {
    SimError::Io(e.to_string())
}

/// 02c §2.2의 bundle_id: 정규 JSON {ontology_sha256, files: 역할 → sha256 목록, scenario_sha256: 목록}의 SHA-256 앞 16자.
pub fn compute_bundle_id(ontology_sha256: &str, files: &BTreeMap<String, Vec<FileEntry>>, scenario_sha256: &[String]) -> String {
    let files_sha: BTreeMap<&str, Vec<&str>> = files.iter().map(|(k, v)| (k.as_str(), v.iter().map(|e| e.sha256.as_str()).collect())).collect();
    let canon = serde_json::json!({"ontology_sha256": ontology_sha256, "files": files_sha, "scenario_sha256": scenario_sha256});
    let text = serde_json::to_string(&canon).expect("json");
    hex(&Sha256::digest(text.as_bytes()))[..16].to_string()
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// `root/ontology/v1/`, `root/data/reaction-inputs/fixture/`, `root/data/reaction-bundles/<id>/`를 만든다.
/// 같은 입력은 바이트가 같은 파일을 낸다.
pub fn write_fixture(root: &Path) -> Result<FixturePaths> {
    let defs_dir = root.join("ontology").join("v1");
    let idir = root.join("data").join("reaction-inputs").join("fixture");
    std::fs::create_dir_all(&defs_dir)?;
    std::fs::create_dir_all(&idir)?;
    std::fs::write(defs_dir.join("dimensions.yaml"), DIMENSIONS_YAML)?;
    std::fs::write(defs_dir.join("interests.yaml"), INTERESTS_YAML)?;
    std::fs::write(defs_dir.join("classes.yaml"), CLASSES_YAML)?;
    std::fs::write(defs_dir.join("contexts.yaml"), CONTEXTS_YAML)?;
    std::fs::write(defs_dir.join("parameters.yaml"), parameters_yaml())?;
    debug_assert!(REQUIRED_PARAMS.iter().all(|k| fixture_params().iter().any(|(p, _)| p == k)));
    let ontology_sha256 = defs_sha256(&defs_dir)?;
    let meta: Vec<(&str, &str)> = vec![("schema_version", "1"), ("producer", "mdsim fixture")];
    let mut files: BTreeMap<String, Vec<FileEntry>> = BTreeMap::new();
    let mut put = |key: &str, t: Table| -> Result<()> {
        let path = idir.join(format!("{key}.parquet"));
        let rows = t.len() as u64;
        let sha = t.write_parquet(&path, &meta)?;
        files.entry(key.to_string()).or_default().push(FileEntry { path: rel(root, &path), sha256: sha, rows: Some(rows) });
        Ok(())
    };

    let (nodes, edges, streets) = build_graph();
    put(
        "nodes",
        Table::new()
            .col("node_id", ColData::Str(nodes.iter().map(|n| node_id(n.id)).collect()))
            .col("x", ColData::F64(nodes.iter().map(|n| n.pos.x).collect()))
            .col("z", ColData::F64(nodes.iter().map(|n| n.pos.z).collect()))
            .col("elev_m", ColData::F64(nodes.iter().map(|n| elev(n.pos.x, n.pos.z)).collect()))
            .col("elev_source", ColData::Str(vec!["orig".into(); nodes.len()]))
            .col("layer", ColData::I32(vec![0; nodes.len()]))
            .col("kind", ColData::Str(nodes.iter().map(|n| n.kind.to_string()).collect()))
            .col("signal", ColData::Bool(nodes.iter().map(|n| n.signal).collect()))
            .col("cross_safe_demand", ColData::F64(nodes.iter().map(|n| if n.kind == "crossing" { 0.1 } else { 0.0 }).collect()))
            .col("inside_boundary", ColData::Bool(vec![true; nodes.len()])),
    )?;
    let node_pos = |id: i64| nodes[(id - 1) as usize].pos;
    let grade = |e: &FixtureEdge| {
        let (pa, pb) = (node_pos(e.a), node_pos(e.b));
        (elev(pb.x, pb.z) - elev(pa.x, pa.z)) / pa.dist(pb) * 100.0
    };
    let st = |e: &FixtureEdge| &streets[e.street];
    let is_steps = |e: &FixtureEdge| st(e).road_type == "steps";
    put(
        "edges",
        Table::new()
            .col("edge_id", ColData::Str(edges.iter().map(|e| e.id.clone()).collect()))
            .col("a", ColData::Str(edges.iter().map(|e| node_id(e.a)).collect()))
            .col("b", ColData::Str(edges.iter().map(|e| node_id(e.b)).collect()))
            .col("class", ColData::Str(edges.iter().map(|e| st(e).class.to_string()).collect()))
            .col("polyline", ColData::Points(edges.iter().map(|e| e.poly.clone()).collect()))
            .col("length_m", ColData::F64(edges.iter().map(|e| e.poly[0].dist(e.poly[1])).collect()))
            .col("road_type", ColData::Str(edges.iter().map(|e| st(e).road_type.to_string()).collect()))
            .col("name", ColData::StrOpt(edges.iter().map(|e| if st(e).road_type == "pedestrian" { Some(format!("합성{}길", e.street)) } else { None }).collect()))
            .col("osm_way_id", ColData::I64(edges.iter().map(|e| 7000 + e.street as i64).collect()))
            .col("layer", ColData::I32(vec![0; edges.len()]))
            .col("width_m", ColData::F64(edges.iter().map(|e| st(e).width).collect()))
            .col("label_width", ColData::Str(vec!["assumption".into(); edges.len()]))
            .col("walk_mode", ColData::Str(edges.iter().map(|e| st(e).walk_mode.to_string()).collect()))
            .col("sidewalk_width_m", ColData::F64(edges.iter().map(|e| st(e).sw).collect()))
            .col("vehicle_mixed", ColData::Bool(edges.iter().map(|e| st(e).vehicle_mixed).collect()))
            .col("ped_zone", ColData::Bool(edges.iter().map(|e| st(e).vehicle_mixed && e.poly.iter().all(|p| p.z <= 140.0)).collect()))
            .col("is_crossing", ColData::Bool(vec![false; edges.len()]))
            .col("carriage_share", ColData::F64(edges.iter().map(|e| if st(e).vehicle_mixed { 0.5 } else { 0.0 }).collect()))
            .col("grade_pct", ColData::F64(edges.iter().map(&grade).collect()))
            .col("grade_pct_orig", ColData::F64Opt(edges.iter().map(|e| Some(grade(e))).collect()))
            .col("grade_pct_smooth", ColData::F64(edges.iter().map(|e| grade(e) * 0.8).collect()))
            .col("grade_source", ColData::Str(edges.iter().map(|e| if is_steps(e) { "steps".to_string() } else { "orig".to_string() }).collect()))
            .col("grade_baseline_m", ColData::F64(edges.iter().map(|e| e.poly[0].dist(e.poly[1]).max(20.0)).collect()))
            .col("is_steps", ColData::Bool(edges.iter().map(&is_steps).collect()))
            .col("riser_cm", ColData::F64(edges.iter().map(|e| if is_steps(e) { 15.0 } else { 0.0 }).collect()))
            .col("steps_count", ColData::I32(edges.iter().map(|e| if is_steps(e) { (grade(e).abs() / 100.0 * e.poly[0].dist(e.poly[1]) / 0.15).round().max(1.0) as i32 } else { 0 }).collect()))
            .col("conveying", ColData::Bool(vec![false; edges.len()]))
            .col("surface", ColData::Str(vec!["dry".into(); edges.len()]))
            .col("ad_intensity", ColData::F64(edges.iter().map(|e| st(e).ad).collect()))
            .col("way_difficulty", ColData::F64(edges.iter().map(|e| match st(e).road_type { "pedestrian" => 0.2, "primary" => 0.3, "residential" => 0.5, _ => 0.4 }).collect()))
            .col("safe_demand", ColData::F64(edges.iter().map(|e| match st(e).road_type { "residential" => 0.4, "primary" => 0.2, _ => 0.0 }).collect()))
            .col("component", ColData::I32(vec![0; edges.len()]))
            .col("inside_boundary", ColData::Bool(vec![true; edges.len()]))
            .col("assumed", ColData::StrList(edges.iter().map(|_| vec!["width_m".to_string(), "ad_intensity".to_string()]).collect())),
    )?;
    let blds = buildings();
    put(
        "buildings",
        Table::new()
            .col("building_id", ColData::Str(blds.iter().map(|b| format!("B:w{}", b.id)).collect()))
            .col("footprint", ColData::Points(blds.iter().map(|b| rect_poly(b.rect)).collect()))
            .col("height_m", ColData::F64(blds.iter().map(|b| b.height).collect()))
            .col("base_m", ColData::F64(blds.iter().map(|b| elev((b.rect.0 + b.rect.1) / 2.0, (b.rect.2 + b.rect.3) / 2.0)).collect()))
            .col("min_height_m", ColData::F64(vec![0.0; blds.len()]))
            .col("name", ColData::StrOpt(vec![None; blds.len()]))
            .col("source_ids", ColData::StrList(blds.iter().map(|b| vec![format!("way/{}", b.id)]).collect())),
    )?;
    let pl = places(&edges, &streets, &blds);
    let all_days = || vec!["weekday".to_string(), "weekend".to_string(), "event_day".to_string()];
    let mut pt = Table::new()
        .col("place_id", ColData::Str(pl.iter().map(|p| p.id.clone()).collect()))
        .col("class", ColData::Str(pl.iter().map(|p| p.class.to_string()).collect()))
        .col("category", ColData::Str(pl.iter().map(|p| p.category.to_string()).collect()))
        .col("name", ColData::StrOpt(pl.iter().map(|p| Some(p.name.clone())).collect()))
        .col("x", ColData::F64(pl.iter().map(|p| p.pos.x).collect()))
        .col("z", ColData::F64(pl.iter().map(|p| p.pos.z).collect()))
        .col("edge_id", ColData::Str(pl.iter().map(|p| p.edge.clone()).collect()))
        .col("offset_m", ColData::F64(pl.iter().map(|p| place_offset(&edges, p)).collect()))
        .col("anchor_x", ColData::F64(pl.iter().map(|p| p.anchor.x).collect()))
        .col("anchor_z", ColData::F64(pl.iter().map(|p| p.anchor.z).collect()))
        .col("side", ColData::Str(pl.iter().map(|p| place_side(&edges, p)).collect()))
        .col("layer", ColData::I32(vec![0; pl.len()]))
        .col("floor", ColData::I64Opt(pl.iter().map(|p| if p.class == "rest_facility" || p.class == "stall" { None } else { Some(1) }).collect()));
    for it in Interest::ALL {
        pt = pt.col(&format!("offer_{}", it.key()), ColData::F64(pl.iter().map(|p| p.offer[it.ix()]).collect()));
    }
    let pt = pt
        .col("price_level_krw", ColData::F64Opt(pl.iter().map(|p| p.price).collect()))
        .col("discount_pct", ColData::F64(pl.iter().map(|p| p.discount).collect()))
        .col("open_min", ColData::I32(pl.iter().map(|p| p.open as i32).collect()))
        .col("close_min", ColData::I32(pl.iter().map(|p| p.close as i32).collect()))
        .col("day_types", ColData::StrList(pl.iter().map(|_| all_days()).collect()))
        .col("visibility", ColData::F64(pl.iter().map(|p| p.vis).collect()))
        .col("fame", ColData::F64(pl.iter().enumerate().map(|(i, _)| if i.is_multiple_of(9) { 1.0 } else { 0.2 }).collect()))
        .col("foreign_label", ColData::Bool(pl.iter().enumerate().map(|(i, _)| i.is_multiple_of(3)).collect()))
        .col("service_rate_per_min", ColData::F64Opt(pl.iter().map(|p| p.rate).collect()))
        .col("capacity", ColData::I64Opt(pl.iter().map(|p| p.cap.map(|c| c as i64)).collect()))
        .col("seats", ColData::I64Opt(pl.iter().map(|p| if p.class == "rest_facility" { Some(3) } else { None }).collect()))
        .col("footprint_m2", ColData::F64Opt(pl.iter().map(|p| if p.class == "stall" { Some(3.0) } else { None }).collect()))
        .col("sense", ColData::F64Opt(pl.iter().map(|p| if p.class == "event" { Some(0.6) } else { None }).collect()))
        .col("rent_krw_month", ColData::F64Opt(vec![None; pl.len()]))
        .col("staff_count", ColData::F64Opt(vec![None; pl.len()]))
        .col("wage_krw_month", ColData::F64Opt(vec![None; pl.len()]))
        .col("revenue_krw_month", ColData::F64Opt(vec![None; pl.len()]))
        .col("source", ColData::Str(vec!["osm".into(); pl.len()]))
        .col("label", ColData::Str(vec!["assumption".into(); pl.len()]))
        .col("assumed", ColData::StrList(pl.iter().map(|_| vec!["price_level_krw".to_string()]).collect()))
        .col("store_ref", ColData::StrOpt(vec![None; pl.len()]));
    put("places", pt)?;
    let portals: Vec<(&str, &str, V2, (f64, f64), &str, f64, Vec<&str>)> = vec![
        ("Q:b0001", "boundary", V2::new(-40.0, 68.0), (-40.0, 68.0), "서쪽 경계", 0.0, vec!["domestic", "tourist"]),
        ("Q:n9001", "subway", V2::new(198.5, 34.0), (210.0, 34.0), "합성역 1번 출구", 4.0, vec!["domestic", "tourist"]),
        ("Q:n9002", "subway", V2::new(198.5, 170.0), (210.0, 170.0), "합성역 2번 출구", 4.0, vec!["domestic", "tourist"]),
        ("Q:w2004", "hotel", V2::new(170.0, 132.5), (170.0, 136.0), "합성 호텔", 0.0, vec!["tourist"]),
    ];
    let node_id_at = |p: (f64, f64)| nodes.iter().find(|n| (n.pos.x, n.pos.z) == p).map(|n| n.id).unwrap();
    put(
        "portals",
        Table::new()
            .col("portal_id", ColData::Str(portals.iter().map(|p| p.0.to_string()).collect()))
            .col("kind", ColData::Str(portals.iter().map(|p| p.1.to_string()).collect()))
            .col("node_id", ColData::Str(portals.iter().map(|p| node_id(node_id_at(p.3))).collect()))
            .col("x", ColData::F64(portals.iter().map(|p| p.2.x).collect()))
            .col("z", ColData::F64(portals.iter().map(|p| p.2.z).collect()))
            .col("name", ColData::StrOpt(portals.iter().map(|p| Some(p.4.to_string())).collect()))
            .col("headway_min", ColData::F64(portals.iter().map(|p| p.5).collect()))
            .col("groups", ColData::StrList(portals.iter().map(|p| p.6.iter().map(|g| g.to_string()).collect()).collect())),
    )?;

    // 출발지(구)와 숙소 출발지의 접근 시간
    let districts: Vec<(&str, V2)> = vec![("G:11110", V2::new(-1200.0, -900.0)), ("G:11140", V2::new(300.0, 400.0)), ("G:11170", V2::new(500.0, 4000.0)), ("G:11440", V2::new(-5000.0, 500.0)), ("G:11680", V2::new(7000.0, 6000.0))];
    let competitors: Vec<(&str, &str, &str, V2, [f64; 7], f64, f64)> = vec![
        ("C:alpha", "가상 상권 A", "11110", V2::new(-4000.0, 1000.0), [0.7, 0.4, 0.6, 0.2, 0.5, 0.0, 0.2], 0.9, 0.9),
        ("C:beta", "가상 상권 B", "11680", V2::new(5000.0, 6000.0), [0.6, 0.6, 0.7, 0.1, 0.2, 0.0, 0.1], 1.2, 0.7),
    ];
    let hotel = ("Q:w2004", V2::new(170.0, 132.5));
    let mut acc: Vec<(String, String, &str, f64, f64)> = Vec::new();
    for (oid, opos) in districts.iter().chain(std::iter::once(&hotel)) {
        let dests: Vec<(String, V2)> = std::iter::once(("myeongdong".to_string(), V2::new(100.0, 100.0))).chain(competitors.iter().map(|c| (c.0.to_string(), c.3))).collect();
        for (dest, dpos) in dests {
            let km = opos.dist(dpos) / 1000.0;
            if km >= 0.8 {
                acc.push((oid.to_string(), dest.clone(), "bus", 15.0 + 4.0 * km, 1500.0));
                acc.push((oid.to_string(), dest.clone(), "subway", 12.0 + 3.0 * km, 1550.0));
                acc.push((oid.to_string(), dest.clone(), "taxi", 6.0 + 2.5 * km, 4800.0 + 1000.0 * km));
            }
            if km < 2.0 {
                acc.push((oid.to_string(), dest.clone(), "walk", (12.0 * km).max(1.0), 0.0));
            }
        }
    }
    acc.sort_by(|a, b| (a.0.as_str(), a.1.as_str(), a.2).cmp(&(b.0.as_str(), b.1.as_str(), b.2)));
    put(
        "access",
        Table::new()
            .col("origin_id", ColData::Str(acc.iter().map(|a| a.0.clone()).collect()))
            .col("destination", ColData::Str(acc.iter().map(|a| a.1.clone()).collect()))
            .col("mode", ColData::Str(acc.iter().map(|a| a.2.to_string()).collect()))
            .col("base_min", ColData::F64(acc.iter().map(|a| (a.3 * 10.0f64).round() / 10.0).collect()))
            .col("fare_krw", ColData::F64(acc.iter().map(|a| a.4.round()).collect()))
            .col("hops", ColData::I64Opt(vec![None; acc.len()]))
            .col("access_mode", ColData::Str(vec!["uniform".into(); acc.len()]))
            .col("label", ColData::Str(vec!["assumption".into(); acc.len()])),
    )?;
    let mut ct = Table::new()
        .col("id", ColData::Str(competitors.iter().map(|c| c.0.to_string()).collect()))
        .col("name", ColData::Str(competitors.iter().map(|c| c.1.to_string()).collect()))
        .col("district_code", ColData::Str(competitors.iter().map(|c| c.2.to_string()).collect()));
    for it in Interest::ALL {
        ct = ct.col(&format!("interest_{}", it.key()), ColData::F64(competitors.iter().map(|c| c.4[it.ix()]).collect()));
    }
    let ct = ct
        .col("price_level", ColData::F64(competitors.iter().map(|c| c.5).collect()))
        .col("crowd_level", ColData::F64(competitors.iter().map(|c| c.6).collect()))
        .col("label", ColData::Str(vec!["assumption".into(); competitors.len()]))
        .col("source", ColData::Str(vec!["fixture".into(); competitors.len()]));
    put("competitors", ct)?;
    let bg: Vec<(i32, &str, f64)> = vec![(14, "hotel", 40.0), (14, "subway", 30.0), (15, "hotel", 40.0), (15, "subway", 30.0), (16, "hotel", 45.0), (16, "subway", 35.0)];
    put(
        "background_flows",
        Table::new()
            .col("hour", ColData::I32(bg.iter().map(|b| b.0).collect()))
            .col("portal_kind", ColData::Str(bg.iter().map(|b| b.1.to_string()).collect()))
            .col("count", ColData::F64(bg.iter().map(|b| b.2).collect()))
            .col("label", ColData::Str(vec!["assumption".into(); bg.len()])),
    )?;

    // 인물
    let district_ids: Vec<&str> = districts.iter().map(|d| d.0).collect();
    let (profiles, attrs) = profiles_table(&pl, &district_ids, hotel.0);
    put("profiles", profiles)?;
    put("profile_attrs", attrs)?;

    // 번들 셋: 기본(주말 반복 0·1), 짧은 것(주말 1시간), 변형(행사일)
    let ped = "[{start: \"12:00\", end: \"20:00\"}]";
    let make = |name: &str, specs: &[(&str, u32, (&str, &str), f64, &str, &str, f64, u32)], months: u32, configuration: &str| -> Result<(PathBuf, Vec<PathBuf>)> {
        let mut texts = Vec::new();
        let mut entries = Vec::new();
        for (day_type, rep, window, target, ped_only, events, hw, trace) in specs {
            let seed = rep_seed(FIXTURE_BASE_SEED, *rep);
            let text = scenario_yaml(&format!("{name}_{day_type}_r{rep}"), day_type, *window, seed, *target, ped_only, events, *hw, *trace, months, configuration);
            let sha = hex(&Sha256::digest(text.as_bytes()));
            texts.push((format!("{day_type}_r{rep}.yaml"), text));
            entries.push((day_type.to_string(), *rep, seed, sha));
        }
        let scen_sha: Vec<String> = entries.iter().map(|e| e.3.clone()).collect();
        let bundle_id = compute_bundle_id(&ontology_sha256, &files, &scen_sha);
        let bdir = root.join("data").join("reaction-bundles").join(&bundle_id);
        let sdir = bdir.join("scenarios");
        std::fs::create_dir_all(&sdir)?;
        let mut paths = Vec::new();
        let mut scenarios = Vec::new();
        for ((fname, text), (day_type, rep, seed, sha)) in texts.iter().zip(entries) {
            let p = sdir.join(fname);
            std::fs::write(&p, text)?;
            scenarios.push(ScenarioEntry { day_type, rep, seed, path: rel(root, &p), sha256: sha });
            paths.push(p);
        }
        let manifest = BundleManifest {
            schema_version: SCHEMA_VERSION,
            bundle_id: bundle_id.clone(),
            ontology_sha256: ontology_sha256.clone(),
            ontology_validation: serde_json::json!({"status": "synthetic", "validator": "mdsim fixture (Python 검증 아님)", "dimensions_version": "1.0.0", "checks": 0}),
            space_id: "fixture0000000000".into(),
            files: files.clone(),
            scenarios,
            pending_inputs: vec!["stores:DR-212".into(), "tourist_stats:DR-208".into()],
            assumption_flags: vec!["synthetic_fixture".into(), format!("fixture_{name}")],
            producer: serde_json::json!({"package": "mdsim", "version": env!("CARGO_PKG_VERSION"), "role": "fixture"}),
        };
        std::fs::write(bdir.join("manifest.json"), serde_json::to_string_pretty(&manifest).map_err(io_err)? + "\n")?;
        Ok((bdir, paths))
    };
    let (bundle, base) = make("base", &[("weekend", 0, ("14:00", "17:00"), 2000.0, ped, "[]", 1.0, 50), ("weekend", 1, ("14:00", "17:00"), 2000.0, ped, "[]", 1.0, 50)], 0, "static_stores")?;
    let (bundle_quick, quick) = make("quick", &[("weekend", 0, ("14:00", "15:00"), 300.0, ped, "[]", 1.0, 30)], 0, "static_stores")?;
    let (bundle_variant, variant) = make("variant", &[("event_day", 0, ("15:30", "16:30"), 400.0, "[{start: \"16:00\", end: \"20:00\"}]", "[{place_id: \"V:0001\", start: \"15:45\", end: \"16:15\"}]", 1.2, 30)], 0, "static_stores")?;
    // 월 고리(C-211): 상점 적응 구성, 3개월, 하루 창 1시간(상점당 매출이 잡음에 묻히지 않을 만큼)
    let (bundle_monthly, monthly) = make("monthly", &[("weekday", 0, ("14:00", "15:00"), 600.0, ped, "[]", 1.0, 10)], 3, "store_adaptation")?;
    Ok(FixturePaths {
        root: root.to_path_buf(),
        defs: defs_dir,
        bundle,
        scenario: base[0].clone(),
        scenario_r1: base[1].clone(),
        bundle_quick,
        scenario_quick: quick[0].clone(),
        bundle_variant,
        scenario_variant: variant[0].clone(),
        bundle_monthly,
        scenario_monthly: monthly[0].clone(),
    })
}

fn profiles_table(places: &[FixturePlace], districts: &[&str], hotel: &str) -> (Table, Table) {
    let n_dom = 400usize;
    let n_tour = 60usize;
    let n = n_dom + n_tour;
    let r = KeyRng::new(42);
    let u = |i: usize, k: u64| r.u("FIXTURE", "profile", i as u64, k, 0);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let tourist = |i: usize| i >= n_dom;
    let mut t = Table::new();
    let ids: Vec<String> = (0..n).map(|i| if tourist(i) { format!("T:{:06}", i - n_dom + 1) } else { format!("P:fx-{:04}", i + 1) }).collect();
    t = t.col("persona_id", ColData::Str(ids.clone()));
    t = t.col("group", ColData::Str((0..n).map(|i| if tourist(i) { "tourist".to_string() } else { "domestic".to_string() }).collect()));
    t = t.col("origin_id", ColData::Str((0..n).map(|i| if tourist(i) { hotel.to_string() } else { districts[((u(i, 1) * districts.len() as f64).floor() as usize).min(districts.len() - 1)].to_string() }).collect()));
    // 능력: 0~1의 신체 여력 f로 차원을 함께 움직인다(나이는 넣지 않는다). D-TIME은 능력 열이 없다.
    let fit: Vec<f64> = (0..n).map(|i| u(i, 2)).collect();
    let caps: [(Dim, f64, f64, f64, f64); 9] = [
        // dim, comfort lo, comfort hi, limit gap lo, limit gap hi
        (Dim::Climb, 3.0, 12.0, 4.0, 10.0),
        (Dim::Fall, 0.15, 0.45, 0.2, 0.4),
        (Dim::Step, 15.0, 30.0, 10.0, 20.0),
        (Dim::Walk, 1500.0, 5000.0, 1000.0, 4000.0),
        (Dim::Money, 20_000.0, 120_000.0, 30_000.0, 200_000.0),
        (Dim::Way, 0.3, 0.6, 0.3, 0.4),
        (Dim::Sense, 0.3, 0.6, 0.3, 0.4),
        (Dim::Crowd, 0.4, 1.0, 0.8, 1.5),
        (Dim::Safe, 0.15, 0.35, 0.3, 0.4),
    ];
    for (k, (d, clo, chi, glo, ghi)) in caps.iter().enumerate() {
        let body = matches!(d, Dim::Climb | Dim::Fall | Dim::Step | Dim::Walk);
        let comfort: Vec<f64> = (0..n)
            .map(|i| {
                let c = lerp(*clo, *chi, if body { fit[i] } else { u(i, 10 + k as u64) });
                // 관광객은 한국어 표기 길찾기 부담에 약하다(능력값으로만 들어간다).
                if *d == Dim::Way && tourist(i) {
                    c * 0.5
                } else {
                    c
                }
            })
            .collect();
        let limit: Vec<f64> = (0..n).map(|i| comfort[i] + lerp(*glo, *ghi, u(i, 30 + k as u64))).collect();
        t = t.col(&format!("cap_{}_comfort", d.key()), ColData::F64(comfort));
        t = t.col(&format!("cap_{}_limit", d.key()), ColData::F64(limit));
        t = t.col(&format!("cap_{}_uncertainty", d.key()), ColData::F64(vec![0.25; n]));
        t = t.col(&format!("cap_{}_label", d.key()), ColData::Str(vec!["assumption".to_string(); n]));
    }
    for (k, d) in Dim::ALL.iter().enumerate() {
        t = t.col(&format!("sens_{}", d.key()), ColData::F64((0..n).map(|i| lerp(0.5, 1.5, u(i, 50 + k as u64))).collect()));
    }
    let interests: Vec<[f64; 7]> = (0..n)
        .map(|i| {
            std::array::from_fn(|j| {
                let base = (u(i, 70 + j as u64) * 1.2 - 0.2).max(0.0);
                // 관광객은 뷰티·기념품·음식 쪽이 크다.
                if tourist(i) && matches!(j, 0 | 1 | 3) {
                    (base + 0.3).min(1.0)
                } else {
                    base
                }
            })
        })
        .collect();
    for it in Interest::ALL {
        t = t.col(&format!("interest_{}", it.key()), ColData::F64(interests.iter().map(|v| v[it.ix()]).collect()));
    }
    for (k, name) in ["risk", "punctuality", "exploration", "attention"].iter().enumerate() {
        t = t.col(name, ColData::F64((0..n).map(|i| u(i, 90 + k as u64)).collect()));
    }
    t = t.col("tau", ColData::F64((0..n).map(|i| lerp(0.7, 1.3, u(i, 95))).collect()));
    t = t.col("base_speed_mps", ColData::F64((0..n).map(|i| lerp(1.0, 1.45, fit[i])).collect()));
    t = t.col("child_prob", ColData::F64((0..n).map(|i| if u(i, 96) < 0.1 { 0.5 } else { 0.0 }).collect()));
    t = t.col("stroller_prob", ColData::F64((0..n).map(|i| if u(i, 97) < 0.05 { 1.0 } else { 0.0 }).collect()));
    t = t.col("luggage_prob", ColData::F64((0..n).map(|i| if tourist(i) && u(i, 98) < 0.2 { 1.0 } else { 0.0 }).collect()));
    t = t.col("focal_range_m", ColData::F64((0..n).map(|i| lerp(15.0, 30.0, fit[i] * 0.6 + u(i, 99) * 0.4)).collect()));
    t = t.col("focal_angle_deg", ColData::F64((0..n).map(|i| lerp(40.0, 70.0, fit[i] * 0.6 + u(i, 100) * 0.4)).collect()));
    t = t.col("peripheral_range_m", ColData::F64(vec![10.0; n]));
    t = t.col("peripheral_angle_deg", ColData::F64(vec![150.0; n]));
    t = t.col("head_turn_deg", ColData::F64(vec![70.0; n]));
    t = t.col("min_dwell_s", ColData::F64((0..n).map(|i| lerp(1.0, 1.5, u(i, 101))).collect()));
    // 목적
    let shops: Vec<&FixturePlace> = places.iter().filter(|p| matches!(p.class, "store" | "cafe" | "landmark")).collect();
    let purpose_of = |it: Interest| match it {
        Interest::Food => "food",
        Interest::Beauty | Interest::Fashion | Interest::Souvenir => "shopping",
        Interest::Culture => "culture",
        Interest::Faith => "faith",
        Interest::Nature => "tour",
    };
    let purpose_rows: Vec<Vec<Vec<Scalar>>> = (0..n)
        .map(|i| {
            let k = 1 + (u(i, 110) * 2.0).floor() as usize;
            let mut rows: Vec<Vec<Scalar>> = (0..k)
                .map(|j| {
                    let it = Interest::ALL[(u(i, 111 + j as u64) * 5.0).floor() as usize];
                    let target = if u(i, 120 + j as u64) < 0.3 {
                        let cands: Vec<&&FixturePlace> = shops.iter().filter(|p| p.offer[it.ix()] >= 0.5).collect();
                        if cands.is_empty() {
                            None
                        } else {
                            Some(cands[(u(i, 130 + j as u64) * cands.len() as f64).floor() as usize].id.clone())
                        }
                    } else {
                        None
                    };
                    vec![Scalar::S(purpose_of(it).into()), Scalar::S(it.id().to_string()), Scalar::F(lerp(0.5, 1.5, u(i, 140 + j as u64))), target.map(Scalar::S).unwrap_or(Scalar::Null)]
                })
                .collect();
            // 몇 명은 업종과 무관한 약속(만남)이 있다.
            if !tourist(i) && u(i, 145) < 0.08 {
                rows.push(vec![Scalar::S("meet".into()), Scalar::S(String::new()), Scalar::F(2.5), Scalar::Null]);
            }
            rows
        })
        .collect();
    t = t.col("purposes", ColData::StructList { fields: vec![("purpose".into(), FieldKind::Str), ("interest".into(), FieldKind::Str), ("value".into(), FieldKind::F64), ("target_place_id".into(), FieldKind::Str)], rows: purpose_rows });
    let avail_rows: Vec<Vec<Vec<Scalar>>> = (0..n)
        .map(|i| {
            let wk = if tourist(i) { (600.0, 1320.0) } else if u(i, 150) < 0.6 { (1110.0, 1320.0) } else { (600.0, 1260.0) };
            let we = if u(i, 151) < 0.8 { (600.0, 1320.0) } else { (780.0, 1080.0) };
            vec![
                vec![Scalar::S("weekday".into()), Scalar::F(wk.0), Scalar::F(wk.1)],
                vec![Scalar::S("weekend".into()), Scalar::F(we.0), Scalar::F(we.1)],
                vec![Scalar::S("event_day".into()), Scalar::F(we.0), Scalar::F(we.1)],
            ]
        })
        .collect();
    t = t.col("availability", ColData::StructList { fields: vec![("day_type".into(), FieldKind::Str), ("start_min".into(), FieldKind::I64), ("end_min".into(), FieldKind::I64)], rows: avail_rows });
    let ev_rows: Vec<Vec<Vec<Scalar>>> = (0..n)
        .map(|i| {
            let mut v = vec![vec![Scalar::S("cap_limit.climb".into()), Scalar::S("sports_persona".into()), Scalar::F(0.0), Scalar::F(12.0), Scalar::S("LX-CLIMB-FX".into()), Scalar::S(if fit[i] < 0.3 { "text".into() } else { "assumption".into() })]];
            if interests[i][0] > 0.6 {
                v.push(vec![Scalar::S("interest.food".into()), Scalar::S("culinary_persona".into()), Scalar::F(3.0), Scalar::F(9.0), Scalar::S("LX-FOOD-FX".into()), Scalar::S("text".into())]);
            }
            v
        })
        .collect();
    t = t.col(
        "evidence",
        ColData::StructList {
            fields: vec![("target".into(), FieldKind::Str), ("field".into(), FieldKind::Str), ("start".into(), FieldKind::I64), ("end".into(), FieldKind::I64), ("rule_id".into(), FieldKind::Str), ("label".into(), FieldKind::Str)],
            rows: ev_rows,
        },
    );
    t = t.col("profile_version", ColData::Str(vec!["fixture-1".into(); n]));
    let ages: Vec<i64> = (0..n).map(|i| (80.0 - 58.0 * fit[i] + 6.0 * (u(i, 160) - 0.5)).round().clamp(18.0, 90.0) as i64).collect();
    let band = |a: i64| if a >= 60 { "60대이상".to_string() } else { format!("{}대", a / 10 * 10) };
    let attrs = Table::new()
        .col("persona_id", ColData::Str(ids))
        .col("group", ColData::Str((0..n).map(|i| if tourist(i) { "tourist".to_string() } else { "domestic".to_string() }).collect()))
        .col("age", ColData::I64(ages.clone()))
        .col("age_band", ColData::Str(ages.iter().map(|a| band(*a)).collect()))
        .col("sex", ColData::Str((0..n).map(|i| if u(i, 161) < 0.5 { "여자".to_string() } else { "남자".to_string() }).collect()))
        .col("nationality", ColData::Str((0..n).map(|i| if tourist(i) { ["CN", "JP", "US", "TW"][(u(i, 163) * 4.0).floor() as usize % 4].to_string() } else { "KR".to_string() }).collect()))
        .col("income_krw", ColData::I64((0..n).map(|i| ((lerp(1.2e6, 6.0e6, u(i, 162)) / 1000.0).round() * 1000.0) as i64).collect()))
        .col("income_label", ColData::Str(vec!["assumption".into(); n]))
        .col("source_ref", ColData::Str(vec!["fixture".into(); n]));
    (t, attrs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Bundle;
    use crate::defs::Defs;
    use crate::scenario::Scenario;

    pub fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mdsim-{name}-{}", std::process::id()))
    }

    #[test]
    fn fixture_loads_and_is_deterministic() {
        let a = write_fixture(&tmp("fxa")).unwrap();
        let b = write_fixture(&tmp("fxb")).unwrap();
        let defs = Defs::load(&a.defs).unwrap();
        let bundle = Bundle::load(&a.bundle, &a.root, &defs).unwrap();
        assert_eq!(bundle.profiles.len(), 460);
        assert!(bundle.places.len() >= 40, "장소 {}", bundle.places.len());
        assert_eq!(bundle.portals.len(), 4);
        assert!(bundle.attrs.is_some());
        assert!(bundle.origins.is_empty(), "origins는 계약에 없는 선택 입력");
        let e = bundle.scenario_entry(&a.scenario).unwrap();
        assert_eq!((e.day_type.as_str(), e.rep, e.seed), ("weekend", 0, rep_seed(FIXTURE_BASE_SEED, 0)));
        let scn = Scenario::load(&a.scenario).unwrap();
        assert_eq!(scn.seed, e.seed);
        assert!(bundle.scenario_entry(&a.scenario_quick).is_err(), "다른 번들의 시나리오는 거부");
        // 내용 주소: 같은 입력이면 같은 번들 ID와 같은 manifest 바이트
        assert_eq!(a.bundle.file_name(), b.bundle.file_name());
        let ma = std::fs::read_to_string(a.bundle.join("manifest.json")).unwrap();
        let mb = std::fs::read_to_string(b.bundle.join("manifest.json")).unwrap();
        assert_eq!(ma, mb, "같은 고정 자료는 같은 해시");
        let _ = std::fs::remove_dir_all(&a.root);
        let _ = std::fs::remove_dir_all(&b.root);
    }
}
