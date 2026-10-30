//! `ontology/v1/` 정의 읽기 (02d I-CORE-a).
//!
//! 정의의 정본 검증은 Python(02c)이 맡고(DR-214·DR-215), Rust는 실행에 필요한 형식만 확인한다.
//! 해시는 폴더 바로 아래 `*.yaml`을 파일 이름순으로 이어 붙인 바이트의 SHA-256이다.

use crate::dims::{Dim, DimVec, IntVec, Interest, ND, NI};
use crate::error::{Result, SimError};
use crate::expr::Expr;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 식에서 쓸 수 있는 런타임 변수(02d §2.1).
pub const RUNTIME_VARS: [&str; 6] = ["vehicle_active", "stall_active", "event_active", "open", "light", "ad_multiplier"];

/// Rust 런타임이 요구하는 계수 키(02d §2.4). 하나라도 없으면 정의 적재 오류다.
pub const REQUIRED_PARAMS: &[&str] = &[
    "react.time_weight",
    "react.deadline_k",
    "react.deadline_horizon_min",
    "react.switch_cost",
    "react.status_quo",
    "notice.b0",
    "notice.bv",
    "notice.ba",
    "notice.bt",
    "notice.brho",
    "notice.t_ref_s",
    "notice.w_peripheral",
    "notice.trigger_min_interval_s",
    "notice.trigger_min_offer",
    "look.k_max",
    "look.unattended_mult",
    "look.footing_relief",
    "look.lookahead_s",
    "look.novelty_seen",
    "look.interrupt_m",
    "perception.crowd_kappa",
    "perception.night_factor",
    "perception.night_light",
    "observe.fresh_s",
    "crowd.prior_density",
    "crowd.local_radius_m",
    "crowd.los_thresholds",
    "loco.scan_dirs",
    "loco.tau_s",
    "loco.time_gap_s",
    "loco.body_radius_m",
    "loco.companion_radius_add_m",
    "loco.lookahead_m",
    "loco.browse_factor",
    "loco.fatigue_factor",
    "loco.steps_factor",
    "loco.tobler_k",
    "loco.tobler_s0",
    "loco.tobler_fmax",
    "loco.arrive_m",
    "loco.queue_spacing_m",
    "loco.wall_clearance_m",
    "loco.lane_width_m",
    "loco.core_strip_m",
    "state.climb_coef",
    "state.steps_coef",
    "state.recover_seated_per_min",
    "state.recover_standing_per_min",
    "state.rest_trigger",
    "purchase.band_mult",
    "purchase.band_gain",
    "purchase.dwell_no_buy_min",
    "purchase.dwell_buy_min",
    "route.k",
    "route.penalty",
    "route.pace_time",
    "route.pace_exertion",
    "route.replan_margin",
    "route.crossing_wait_s",
    "route.defer_min",
    "rest.seat_min",
    "rest.max_bench_m",
    "visit.slot_min",
    "visit.theta_skip",
    "visit.exit_walk_default_min",
    "visit.expected_walk_m",
    "visit.expected_spend_share",
    "visit.stay_base_min",
    "visit.stay_per_purpose_min",
    "visit.stay_max_min",
    "visit.mean_dwell_min",
    "visit.myeongdong_way",
    "visit.myeongdong_climb",
    "visit.bus_wait_min",
    "visit.taxi_wait_min",
    "bg.stops_min",
    "bg.stops_max",
    "bg.dwell_min_s",
    "bg.dwell_max_s",
    "bg.speed_mps",
    "next.max_candidates",
    "explore.radius_min_m",
    "explore.radius_max_m",
    "explore.discovery_share",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Sum,
    Max,
    TimeWeightedMean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    Soft,
    Hard,
    HardForWheels,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DimDef {
    pub dim: Dim,
    pub aggregate: Aggregate,
    pub limit: LimitKind,
    pub gamma: f64,
    pub beta: f64,
    pub penalty: f64,
    /// `cost_curve: linear`: φ 비용 없이 선형 항(λ)과 경성 한계만 쓴다(D-TIME·D-MONEY, 02c B4).
    pub linear: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttrType {
    Continuous,
    Integer,
    /// 0/1로 식에 들어간다.
    Boolean,
    /// 식에서는 values 안의 인덱스.
    Categorical(Vec<String>),
    /// 범주 집합(day_types 등). 식에 쓸 수 없다.
    CategoricalSet(Vec<String>),
    /// 일정. 식에 쓸 수 없고 런타임 변수로 바뀐다.
    Schedule,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttrDef {
    pub name: String,
    pub ty: AttrType,
    pub range: Option<(f64, f64)>,
    pub role: String,
    /// 숫자 기본값(범주형이면 values 안의 인덱스).
    pub default: Option<f64>,
    /// 진행 방향 값으로 넣는 속성(grade_pct). b→a로 지나면 부호를 바꾼다(02c §2.3).
    pub directional: bool,
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub attrs: Vec<AttrDef>,
    pub press: Vec<(Dim, Expr)>,
    /// press처럼 키 단위로 부모 것을 덮는다(02c I-203-a 상속 규칙).
    pub offers: Option<Vec<(Interest, Expr)>>,
    pub derived: Vec<(String, Expr)>,
    /// 정의하면 부모 것을 덮는다.
    pub affords: Option<Vec<String>>,
    pub blocks: Option<Vec<String>>,
}

/// 상속을 풀어낸 클래스.
#[derive(Clone, Debug)]
pub struct ResolvedClass {
    pub id: String,
    pub name: String,
    pub chain: Vec<String>,
    pub attrs: BTreeMap<String, AttrDef>,
    pub press: Vec<(Dim, Expr)>,
    pub offers: Vec<(Interest, Expr)>,
    pub derived: Vec<(String, Expr)>,
    pub affords: BTreeSet<String>,
    pub blocks: BTreeSet<String>,
    /// 런타임 변수에 기대는 요구 차원.
    pub dynamic: [bool; ND],
}

impl ResolvedClass {
    pub fn affords(&self, action: &str) -> bool {
        self.affords.contains(action) && !self.blocks.contains(action)
    }
}

#[derive(Clone, Debug)]
pub struct ContextDef {
    pub id: String,
    pub actions: Vec<String>,
    pub always: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamValue {
    pub value: Vec<f64>,
    pub scalar: bool,
    pub unit: String,
    pub label: String,
    pub source: String,
}

#[derive(Clone, Debug, Default)]
pub struct Params(pub BTreeMap<String, ParamValue>);

impl Params {
    pub fn f(&self, key: &str) -> Result<f64> {
        match self.0.get(key) {
            Some(p) if p.scalar => Ok(p.value[0]),
            Some(_) => Err(SimError::Defs(format!("계수 '{key}'는 목록이다(스칼라가 필요함)"))),
            None => Err(SimError::Defs(format!("계수 '{key}'가 parameters.yaml에 없음"))),
        }
    }

    pub fn v(&self, key: &str) -> Result<Vec<f64>> {
        self.0.get(key).map(|p| p.value.clone()).ok_or_else(|| SimError::Defs(format!("계수 '{key}'가 parameters.yaml에 없음")))
    }

    /// 없어도 되는 계수(상점 월 반응 등).
    pub fn f_or(&self, key: &str, default: f64) -> f64 {
        self.0.get(key).filter(|p| p.scalar).map(|p| p.value[0]).unwrap_or(default)
    }
}

#[derive(Clone, Debug)]
pub struct Defs {
    pub dims: [DimDef; ND],
    /// dimensions.yaml의 version(실행 manifest의 dimensions_version).
    pub dims_version: String,
    pub classes: Vec<ResolvedClass>,
    pub class_ix: BTreeMap<String, usize>,
    pub contexts: BTreeMap<String, ContextDef>,
    pub params: Params,
    pub sha256: String,
}

// ---------- YAML 형식 ----------

#[derive(Deserialize)]
struct DimFile {
    #[serde(default)]
    version: Option<serde_norway::Value>,
    dimensions: Vec<DimYaml>,
}

#[derive(Deserialize)]
struct CostYaml {
    exponent: f64,
    overflow_slope: f64,
    overflow_penalty: f64,
}

#[derive(Deserialize)]
struct DimYaml {
    id: String,
    aggregate: String,
    limit: String,
    cost: CostYaml,
    #[serde(default)]
    cost_curve: Option<String>,
}

#[derive(Deserialize)]
struct InterestFile {
    interests: Vec<InterestYaml>,
}

#[derive(Deserialize)]
struct InterestYaml {
    id: String,
}

#[derive(Deserialize)]
struct ClassFile {
    classes: Vec<ClassYaml>,
}

#[derive(Deserialize)]
struct ClassYaml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    attributes: BTreeMap<String, AttrYaml>,
    #[serde(default)]
    press: BTreeMap<String, String>,
    #[serde(default)]
    offers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    derived: Vec<DerivedYaml>,
    #[serde(default)]
    affords: Option<Vec<String>>,
    #[serde(default)]
    blocks: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct DerivedYaml {
    name: String,
    expr: String,
}

#[derive(Deserialize)]
struct AttrYaml {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    range: Option<Vec<f64>>,
    #[serde(default)]
    values: Option<Vec<String>>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    default: Option<serde_norway::Value>,
    #[serde(default)]
    directional: Option<bool>,
}

#[derive(Deserialize)]
struct ContextFile {
    contexts: Vec<ContextYaml>,
}

#[derive(Deserialize)]
struct ContextYaml {
    id: String,
    actions: Vec<String>,
    #[serde(default)]
    always: Vec<String>,
}

#[derive(Deserialize)]
struct ParamFile {
    parameters: BTreeMap<String, ParamYaml>,
}

#[derive(Deserialize)]
struct ParamYaml {
    value: serde_norway::Value,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

fn read_yaml<T: for<'de> Deserialize<'de>>(dir: &Path, name: &str) -> Result<T> {
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path).map_err(|e| SimError::Defs(format!("{}: {e}", path.display())))?;
    serde_norway::from_str(&text).map_err(|e| SimError::Defs(format!("{name}: {e}")))
}

/// 폴더 바로 아래 `*.yaml`의 SHA-256(파일 이름순으로 바이트를 이어 붙임).
pub fn defs_sha256(dir: &Path) -> Result<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| SimError::Defs(format!("{}: {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && e.path().extension().map(|x| x == "yaml").unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    let mut h = Sha256::new();
    for n in &names {
        h.update(std::fs::read(dir.join(n))?);
    }
    Ok(hex(&h.finalize()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn num_value(v: &serde_norway::Value) -> Option<f64> {
    match v {
        serde_norway::Value::Number(n) => n.as_f64(),
        serde_norway::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

impl Defs {
    pub fn load(dir: &Path) -> Result<Defs> {
        let sha256 = defs_sha256(dir)?;
        let dim_file: DimFile = read_yaml(dir, "dimensions.yaml")?;
        let int_file: InterestFile = read_yaml(dir, "interests.yaml")?;
        let class_file: ClassFile = read_yaml(dir, "classes.yaml")?;
        let ctx_file: ContextFile = read_yaml(dir, "contexts.yaml")?;
        let par_file: ParamFile = read_yaml(dir, "parameters.yaml")?;

        // 차원
        let mut dims: Vec<Option<DimDef>> = vec![None; ND];
        for d in &dim_file.dimensions {
            let dim = Dim::from_id(&d.id).ok_or_else(|| SimError::Defs(format!("dimensions.yaml: 엔진이 모르는 차원 '{}' (새 차원은 코드 변경이 필요하다)", d.id)))?;
            let aggregate = match d.aggregate.as_str() {
                "sum" => Aggregate::Sum,
                "max" => Aggregate::Max,
                "time_weighted_mean" => Aggregate::TimeWeightedMean,
                o => return Err(SimError::Defs(format!("dimensions.yaml: {}: 알 수 없는 집계 '{o}'", d.id))),
            };
            let limit = match d.limit.as_str() {
                "soft" => LimitKind::Soft,
                "hard" => LimitKind::Hard,
                "hard_for_wheels" => LimitKind::HardForWheels,
                o => return Err(SimError::Defs(format!("dimensions.yaml: {}: 알 수 없는 한계 '{o}'", d.id))),
            };
            if d.cost.exponent < 1.0 || d.cost.overflow_slope < 0.0 || d.cost.overflow_penalty < 0.0 {
                return Err(SimError::Defs(format!("dimensions.yaml: {}: 비용 곡선 계수가 범위 밖", d.id)));
            }
            if dims[dim.ix()].is_some() {
                return Err(SimError::Defs(format!("dimensions.yaml: 차원 '{}' 중복", d.id)));
            }
            let linear = match d.cost_curve.as_deref() {
                None | Some("phi") | Some("convex") => false,
                Some("linear") => true,
                Some(o) => return Err(SimError::Defs(format!("dimensions.yaml: {}: 알 수 없는 cost_curve '{o}'", d.id))),
            };
            dims[dim.ix()] = Some(DimDef { dim, aggregate, limit, gamma: d.cost.exponent, beta: d.cost.overflow_slope, penalty: d.cost.overflow_penalty, linear });
        }
        let missing: Vec<&str> = Dim::ALL.iter().filter(|d| dims[d.ix()].is_none()).map(|d| d.id()).collect();
        if !missing.is_empty() {
            return Err(SimError::Defs(format!("dimensions.yaml: 차원 누락 {missing:?}")));
        }
        let dims: [DimDef; ND] = std::array::from_fn(|i| dims[i].clone().unwrap());

        // 관심 차원
        let mut seen = [false; NI];
        for i in &int_file.interests {
            let it = Interest::from_id(&i.id).ok_or_else(|| SimError::Defs(format!("interests.yaml: 엔진이 모르는 관심 차원 '{}'", i.id)))?;
            seen[it.ix()] = true;
        }
        if seen.iter().any(|s| !s) {
            return Err(SimError::Defs("interests.yaml: 관심 차원 누락".into()));
        }

        // 클래스
        let mut raw: BTreeMap<String, ClassDef> = BTreeMap::new();
        for c in class_file.classes {
            if raw.contains_key(&c.id) {
                return Err(SimError::Defs(format!("classes.yaml: 클래스 '{}' 중복", c.id)));
            }
            let mut attrs = Vec::new();
            for (name, a) in &c.attributes {
                let ty = match a.ty.as_str() {
                    "continuous" => AttrType::Continuous,
                    "integer" => AttrType::Integer,
                    "boolean" => AttrType::Boolean,
                    "categorical" => AttrType::Categorical(a.values.clone().unwrap_or_default()),
                    "categorical_set" => AttrType::CategoricalSet(a.values.clone().unwrap_or_default()),
                    "schedule" => AttrType::Schedule,
                    o => return Err(SimError::Defs(format!("classes.yaml: {}.{name}: 알 수 없는 형식 '{o}'", c.id))),
                };
                let range = match &a.range {
                    Some(r) if r.len() == 2 && r[0] <= r[1] => Some((r[0], r[1])),
                    Some(_) => return Err(SimError::Defs(format!("classes.yaml: {}.{name}: range는 [하한, 상한]", c.id))),
                    None => None,
                };
                let default = match (&ty, &a.default) {
                    (_, None) => None,
                    (AttrType::Categorical(values), Some(serde_norway::Value::String(s))) => {
                        Some(values.iter().position(|v| v == s).ok_or_else(|| SimError::Defs(format!("classes.yaml: {}.{name}: 기본값 '{s}'가 values에 없음", c.id)))? as f64)
                    }
                    (_, Some(v)) => Some(num_value(v).ok_or_else(|| SimError::Defs(format!("classes.yaml: {}.{name}: 기본값이 숫자가 아님", c.id)))?),
                };
                if let (Some(d), Some((lo, hi))) = (default, range) {
                    if d < lo || d > hi {
                        return Err(SimError::Defs(format!("classes.yaml: {}.{name}: 기본값 {d}이 범위 [{lo}, {hi}] 밖", c.id)));
                    }
                }
                attrs.push(AttrDef { name: name.clone(), ty, range, role: a.role.clone().unwrap_or_default(), default, directional: a.directional.unwrap_or(false) });
            }
            let parse = |src: &str, what: &str| Expr::parse(src).map_err(|e| SimError::Defs(format!("classes.yaml: {}.{what}: {e}", c.id)));
            let mut press = Vec::new();
            for (k, src) in &c.press {
                let d = Dim::from_id(k).ok_or_else(|| SimError::Defs(format!("classes.yaml: {}: press의 '{k}'는 차원이 아니다(금지 간선)", c.id)))?;
                press.push((d, parse(src, k)?));
            }
            let offers = match &c.offers {
                None => None,
                Some(m) => {
                    let mut v = Vec::new();
                    for (k, src) in m {
                        let it = Interest::from_id(k).ok_or_else(|| SimError::Defs(format!("classes.yaml: {}: offers의 '{k}'는 관심 차원이 아니다(금지 간선)", c.id)))?;
                        v.push((it, parse(src, k)?));
                    }
                    Some(v)
                }
            };
            let mut derived = Vec::new();
            for d in &c.derived {
                derived.push((d.name.clone(), parse(&d.expr, &d.name)?));
            }
            raw.insert(
                c.id.clone(),
                ClassDef { id: c.id.clone(), name: c.name.clone().unwrap_or_else(|| c.id.clone()), parent: c.parent.clone(), attrs, press, offers, derived, affords: c.affords.clone(), blocks: c.blocks.clone() },
            );
        }
        let mut classes = Vec::new();
        let mut class_ix = BTreeMap::new();
        for id in raw.keys() {
            let rc = resolve(&raw, id)?;
            class_ix.insert(id.clone(), classes.len());
            classes.push(rc);
        }

        // 맥락
        let mut contexts = BTreeMap::new();
        for c in ctx_file.contexts {
            for a in &c.always {
                if !c.actions.contains(a) {
                    return Err(SimError::Defs(format!("contexts.yaml: {}: always의 '{a}'가 actions에 없음", c.id)));
                }
            }
            contexts.insert(c.id.clone(), ContextDef { id: c.id, actions: c.actions, always: c.always });
        }
        for need in ["VISIT", "NEXT", "ROUTE", "REST", "PURCHASE", "LOOK"] {
            if !contexts.contains_key(need) {
                return Err(SimError::Defs(format!("contexts.yaml: 결정 맥락 '{need}' 없음")));
            }
        }

        // 계수(사전 값은 점 경로로 펼친다: fall.surface_factor.wet, 02c B2)
        let mut params = Params::default();
        fn put(params: &mut Params, k: &str, v: &serde_norway::Value, unit: &str, label: &str, source: &str) -> Result<()> {
            let (value, scalar) = match v {
                serde_norway::Value::Mapping(m) => {
                    for (sk, sv) in m {
                        let sk = match sk {
                            serde_norway::Value::String(x) => x.clone(),
                            other => serde_norway::to_string(other).unwrap_or_default().trim().to_string(),
                        };
                        put(params, &format!("{k}.{sk}"), sv, unit, label, source)?;
                    }
                    return Ok(());
                }
                // 숫자만 든 목록만 읽는다. 표·문자열 목록(Python 전용 키)은 건너뛴다(02c §2.3: 모르는 값은 무시).
                serde_norway::Value::Sequence(seq) => {
                    // 숫자 표([[a, b], …])는 행 순서로 펼친다(모양은 읽는 쪽이 안다).
                    let flat: Option<Vec<f64>> = seq
                        .iter()
                        .map(|x| match x {
                            serde_norway::Value::Sequence(row) => row.iter().map(num_value).collect::<Option<Vec<f64>>>(),
                            other => num_value(other).map(|v| vec![v]),
                        })
                        .collect::<Option<Vec<Vec<f64>>>>()
                        .map(|rows| rows.concat());
                    match flat {
                        Some(v) => (v, false),
                        None => return Ok(()),
                    }
                }
                other => match num_value(other) {
                    Some(v) => (vec![v], true),
                    None => return Ok(()),
                },
            };
            if value.iter().any(|v| !v.is_finite()) {
                return Err(SimError::Defs(format!("parameters.yaml: '{k}' 값이 유한하지 않음")));
            }
            params.0.insert(k.to_string(), ParamValue { value, scalar, unit: unit.to_string(), label: label.to_string(), source: source.to_string() });
            Ok(())
        }
        for (k, p) in par_file.parameters {
            put(&mut params, &k, &p.value, p.unit.as_deref().unwrap_or(""), p.label.as_deref().unwrap_or(""), p.source.as_deref().unwrap_or(""))?;
        }
        // 식의 $매개변수 참조가 모두 스칼라로 있는지
        for c in &classes {
            for e in c.press.iter().map(|(_, e)| e).chain(c.offers.iter().map(|(_, e)| e)).chain(c.derived.iter().map(|(_, e)| e)) {
                for name in e.idents().into_iter().filter(|n| n.starts_with('$')) {
                    match params.0.get(&name[1..]) {
                        Some(pv) if pv.scalar => {}
                        _ => return Err(SimError::Defs(format!("classes.yaml: {}: 식 '{}'의 매개변수 '{name}'가 parameters.yaml에 스칼라로 없음", c.id, e.source()))),
                    }
                }
            }
        }
        let missing: Vec<&&str> = REQUIRED_PARAMS.iter().filter(|k| !params.0.contains_key(**k)).collect();
        if !missing.is_empty() {
            return Err(SimError::Defs(format!("parameters.yaml: Rust 런타임에 필요한 계수 누락 {missing:?}")));
        }

        let dims_version = match &dim_file.version {
            Some(serde_norway::Value::String(v)) => v.clone(),
            Some(serde_norway::Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        Ok(Defs { dims, dims_version, classes, class_ix, contexts, params, sha256 })
    }

    pub fn class(&self, id: &str) -> Option<&ResolvedClass> {
        self.class_ix.get(id).map(|&i| &self.classes[i])
    }

    /// 인스턴스의 요구·제공을 계산한다. `attr`는 인스턴스 값, `rt`는 런타임 변수.
    /// 이름은 derived → 인스턴스 값 → 클래스 기본값 → 런타임 변수 순으로 찾는다.
    pub fn eval_instance(&self, class_ix: usize, attr: &dyn Fn(&str) -> Option<f64>, rt: &dyn Fn(&str) -> Option<f64>) -> std::result::Result<(DimVec, IntVec), String> {
        let c = &self.classes[class_ix];
        let mut derived: Vec<(String, f64)> = Vec::new();
        let lookup = |name: &str, derived: &[(String, f64)]| -> Option<f64> {
            if let Some(key) = name.strip_prefix('$') {
                return self.params.0.get(key).filter(|p| p.scalar).map(|p| p.value[0]);
            }
            if let Some((_, v)) = derived.iter().find(|(n, _)| n == name) {
                return Some(*v);
            }
            if let Some(v) = attr(name) {
                return Some(v);
            }
            if let Some(a) = c.attrs.get(name) {
                if let Some(d) = a.default {
                    return Some(d);
                }
            }
            rt(name)
        };
        for (name, e) in &c.derived {
            let v = e.eval(&|n| lookup(n, &derived)).map_err(|m| format!("{}.{name}: {m}", c.id))?;
            derived.push((name.clone(), v));
        }
        let mut press = [0.0; ND];
        for (d, e) in &c.press {
            press[d.ix()] = e.eval(&|n| lookup(n, &derived)).map_err(|m| format!("{}.{}: {m}", c.id, d.id()))?;
        }
        let mut offers = [0.0; NI];
        for (i, e) in &c.offers {
            offers[i.ix()] = e.eval(&|n| lookup(n, &derived)).map_err(|m| format!("{}.{}: {m}", c.id, i.id()))?;
        }
        Ok((press, offers))
    }

    /// 모든 클래스가 선언한 속성 이름과 범주형 여부(번들의 같은 이름 열을 읽는 일반 규칙, 02c B6).
    pub fn attr_names(&self) -> BTreeMap<String, bool> {
        let mut out = BTreeMap::new();
        for c in &self.classes {
            for (n, a) in &c.attrs {
                // 범주 집합·일정은 식에 쓰지 않으므로 같은 이름 열 규칙에서 뺀다(번들이 따로 읽는다).
                if matches!(a.ty, AttrType::CategoricalSet(_) | AttrType::Schedule) {
                    continue;
                }
                let cat = matches!(a.ty, AttrType::Categorical(_));
                let e = out.entry(n.clone()).or_insert(cat);
                *e = *e || cat;
            }
        }
        out
    }

    /// 범주형 속성 문자열의 인덱스.
    pub fn categorical_index(&self, class_ix: usize, attr: &str, value: &str) -> Option<f64> {
        match &self.classes[class_ix].attrs.get(attr)?.ty {
            AttrType::Categorical(values) => values.iter().position(|v| v == value).map(|i| i as f64),
            _ => None,
        }
    }
}

fn resolve(raw: &BTreeMap<String, ClassDef>, id: &str) -> Result<ResolvedClass> {
    // 조상부터 자손 순으로 쌓는다.
    let mut chain = Vec::new();
    let mut cur = Some(id.to_string());
    while let Some(c) = cur {
        if chain.contains(&c) {
            return Err(SimError::Defs(format!("classes.yaml: '{id}'의 parent가 순환한다")));
        }
        let def = raw.get(&c).ok_or_else(|| SimError::Defs(format!("classes.yaml: '{id}'의 조상 '{c}'가 없다")))?;
        chain.push(c.clone());
        cur = def.parent.clone();
    }
    chain.reverse();
    let mut attrs: BTreeMap<String, AttrDef> = BTreeMap::new();
    let mut press: BTreeMap<Dim, Expr> = BTreeMap::new();
    let mut offers: BTreeMap<Interest, Expr> = BTreeMap::new();
    let mut derived: Vec<(String, Expr)> = Vec::new();
    let mut affords = BTreeSet::new();
    let mut blocks = BTreeSet::new();
    for c in &chain {
        let def = &raw[c];
        for a in &def.attrs {
            attrs.insert(a.name.clone(), a.clone());
        }
        for (d, e) in &def.press {
            press.insert(*d, e.clone());
        }
        if let Some(o) = &def.offers {
            for (i, e) in o {
                offers.insert(*i, e.clone());
            }
        }
        for (n, e) in &def.derived {
            if let Some(slot) = derived.iter_mut().find(|(m, _)| m == n) {
                slot.1 = e.clone();
            } else {
                derived.push((n.clone(), e.clone()));
            }
        }
        if let Some(a) = &def.affords {
            affords = a.iter().cloned().collect();
        }
        if let Some(b) = &def.blocks {
            blocks = b.iter().cloned().collect();
        }
    }
    // 이름 해석 검사와 동적 차원 표시
    let derived_names: Vec<String> = derived.iter().map(|(n, _)| n.clone()).collect();
    let known = |n: &str| n.starts_with('$') || attrs.contains_key(n) || derived_names.iter().any(|d| d == n) || RUNTIME_VARS.contains(&n);
    let mut dynamic_derived: BTreeSet<String> = BTreeSet::new();
    for (n, e) in &derived {
        for v in e.idents() {
            if !known(&v) {
                return Err(SimError::Defs(format!("classes.yaml: {id}.{n}: 정의되지 않은 이름 '{v}'")));
            }
            if RUNTIME_VARS.contains(&v.as_str()) || dynamic_derived.contains(&v) {
                dynamic_derived.insert(n.clone());
            }
        }
    }
    let mut dynamic = [false; ND];
    for (d, e) in &press {
        for v in e.idents() {
            if !known(&v) {
                return Err(SimError::Defs(format!("classes.yaml: {id}.{}: 정의되지 않은 이름 '{v}'", d.id())));
            }
            if RUNTIME_VARS.contains(&v.as_str()) || dynamic_derived.contains(&v) {
                dynamic[d.ix()] = true;
            }
        }
    }
    for (i, e) in &offers {
        for v in e.idents() {
            if !known(&v) {
                return Err(SimError::Defs(format!("classes.yaml: {id}.{}: 정의되지 않은 이름 '{v}'", i.id())));
            }
        }
    }
    let def = &raw[id];
    Ok(ResolvedClass {
        id: id.to_string(),
        name: def.name.clone(),
        chain,
        attrs,
        press: press.into_iter().collect(),
        offers: offers.into_iter().collect(),
        derived,
        affords,
        blocks,
        dynamic,
    })
}
