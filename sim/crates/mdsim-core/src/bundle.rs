//! 입력 번들 읽기와 계약 검사 (02d §2.2, I-CORE-b).
//!
//! 번들 = manifest.json + Parquet 파일 + 시나리오 파일들(02c §2.2). Python(02c)이 만들고, Rust는
//! 정의 해시·파일 해시·행 수를 대조한 뒤 실행에 필요한 형식(열·타입·범위·참조)만 확인한다.
//! manifest의 경로는 저장소 루트 기준 POSIX 상대 경로다.

use crate::defs::Defs;
use crate::dims::{Dim, DimVec, IntVec, Interest, ND, NI};
use crate::error::{Result, SimError};
use crate::geom::V2;
use crate::pq::{self, col_bool, col_f64, col_f64_opt, col_i64, col_points, col_str, col_struct_list, get, has_column, read_table, Scalar};
use arrow_array::RecordBatch;
use crate::scenario::DayType;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 번들·실행 기록의 schema_version(02c §2.9).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalkMode {
    Full,
    Sidewalks,
    NoWalk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Junction,
    Crossing,
    Anchor,
    Portal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PortalKind {
    Subway,
    Bus,
    Hotel,
    Boundary,
    TourBus,
}

impl PortalKind {
    pub fn parse(s: &str) -> Option<PortalKind> {
        Some(match s {
            "subway" => PortalKind::Subway,
            "bus" => PortalKind::Bus,
            "hotel" => PortalKind::Hotel,
            "boundary" => PortalKind::Boundary,
            "tour_bus" => PortalKind::TourBus,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PortalKind::Subway => "subway",
            PortalKind::Bus => "bus",
            PortalKind::Hotel => "hotel",
            PortalKind::Boundary => "boundary",
            PortalKind::TourBus => "tour_bus",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Mode {
    Subway,
    Bus,
    Walk,
    Taxi,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Subway, Mode::Bus, Mode::Walk, Mode::Taxi];

    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "subway" => Mode::Subway,
            "bus" => Mode::Bus,
            "walk" => Mode::Walk,
            "taxi" => Mode::Taxi,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Subway => "subway",
            Mode::Bus => "bus",
            Mode::Walk => "walk",
            Mode::Taxi => "taxi",
        }
    }

    pub fn ix(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Group {
    Domestic,
    Tourist,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeRow {
    pub id: String,
    pub pos: V2,
    pub elev_m: f64,
    pub kind: NodeKind,
    pub signal: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeRow {
    pub id: String,
    pub a: String,
    pub b: String,
    pub class: String,
    pub polyline: Vec<V2>,
    pub length_m: f64,
    pub road_type: String,
    pub width_m: f64,
    pub walk_mode: WalkMode,
    pub sidewalk_width_m: f64,
    pub vehicle_mixed: bool,
    /// 변수 #1(보행 전용 시간)의 대상 간선.
    pub ped_zone: bool,
    /// 횡단 간선. vehicle_active가 항상 1이다.
    pub is_crossing: bool,
    /// 차량 통행 중 잃는 보행 면적 비율 [0, 1).
    pub carriage_share: f64,
    /// a→b 진행 기준 경사(%). 쓰는 값(S-I22).
    pub grade_pct: f64,
    pub grade_pct_orig: Option<f64>,
    pub grade_pct_smooth: f64,
    pub conveying: bool,
    pub layer: f64,
    pub is_steps: bool,
    pub riser_cm: f64,
    pub steps_count: f64,
    pub ad_intensity: f64,
    pub label_width: String,
    /// 클래스 속성과 같은 이름의 숫자 열(02c B6).
    pub attrs: BTreeMap<String, f64>,
    /// 클래스 속성과 같은 이름의 범주 열.
    pub cats: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildingRow {
    pub id: String,
    pub footprint: Vec<V2>,
    pub height_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceRow {
    pub id: String,
    pub class: String,
    pub category: String,
    pub name: String,
    pub pos: V2,
    pub anchor: V2,
    pub edge_id: String,
    /// 간선 a 끝에서 앵커 투영까지 거리(m). 없으면 앵커를 투영해 구한다(02c B5).
    pub offset_m: Option<f64>,
    /// 간선 a→b 기준 앵커 쪽(left·right).
    pub side: Option<String>,
    /// 판매 장소가 아니면 0(널).
    pub price_level_krw: f64,
    pub discount_pct: f64,
    pub open_min: f64,
    pub close_min: f64,
    /// 존재·영업하는 하루 유형.
    pub day_types: Vec<DayType>,
    pub visibility: f64,
    pub fame: f64,
    /// 줄을 만들지 않는 장소는 0(널).
    pub service_rate_per_min: f64,
    /// 동시 수용. 널이면 제한 없음.
    pub capacity: Option<f64>,
    pub seats: Option<f64>,
    pub footprint_m2: Option<f64>,
    pub sense: Option<f64>,
    pub floor: f64,
    pub layer: f64,
    pub foreign_label: bool,
    /// 상점 행위자 재무 값(C-211). 널이면 shop.* 가정값.
    pub rent_krw_month: Option<f64>,
    pub staff_count: Option<f64>,
    pub wage_krw_month: Option<f64>,
    pub revenue_krw_month: Option<f64>,
    pub source: String,
    pub label: String,
    pub attrs: BTreeMap<String, f64>,
    pub cats: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortalRow {
    pub id: String,
    pub kind: PortalKind,
    pub pos: V2,
    pub node_id: String,
    pub name: String,
    /// 지하철 출입구의 기준 배차(분), 그 밖 0.
    pub headway_min: f64,
    /// 이 출입구를 쓰는 집단. 열이 없으면 둘 다.
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Perception {
    pub focal_range_m: f64,
    pub focal_angle_deg: f64,
    pub peripheral_range_m: f64,
    pub peripheral_angle_deg: f64,
    pub head_turn_deg: f64,
    pub min_dwell_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Purpose {
    /// shopping·food·culture·faith·work·meet·tour·other.
    pub purpose: String,
    /// 관련 관심 차원. 일·만남처럼 업종과 무관하면 None.
    pub interest: Option<Interest>,
    pub value: f64,
    pub target_place_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Avail {
    pub day_type: DayType,
    pub start_min: f64,
    pub end_min: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub target: String,
    pub field: String,
    pub start: i64,
    pub end: i64,
    pub rule_id: String,
    pub label: String,
}

/// 반응에 쓰는 인물 프로필. 나이·성별·국적·소득 필드는 두지 않는다(DR-214, C-205 ②).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub persona_id: String,
    /// 몸 수 배율(#10·#11)에만 쓴다. 반응 함수·상황 구성은 읽지 않는다.
    pub group: Group,
    pub origin_id: String,
    pub comfort: DimVec,
    pub limit: DimVec,
    pub uncertainty: DimVec,
    pub cap_label: Vec<String>,
    pub sens: DimVec,
    pub interest: IntVec,
    pub risk: f64,
    pub punctuality: f64,
    pub exploration: f64,
    pub attention: f64,
    pub tau: f64,
    pub base_speed_mps: f64,
    pub child_prob: f64,
    pub stroller_prob: f64,
    pub luggage_prob: f64,
    pub perc: Perception,
    pub purposes: Vec<Purpose>,
    pub availability: Vec<Avail>,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccessRow {
    pub origin_id: String,
    pub destination: String,
    pub mode: Mode,
    pub base_min: f64,
    pub fare_krw: f64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OriginRow {
    pub id: String,
    pub pos: V2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompetitorRow {
    pub id: String,
    pub name: String,
    pub interest: IntVec,
    pub price_level: f64,
    pub crowd_level: f64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BgFlowRow {
    pub hour: u32,
    pub portal_kind: PortalKind,
    pub count: f64,
}

/// plot 요약에서만 쓰는 인물 속성(선택 입력).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileAttrs {
    pub persona_id: String,
    pub age: Option<f64>,
    pub age_band: Option<String>,
    pub sex: Option<String>,
    pub nationality: Option<String>,
    pub income_krw: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub rows: Option<u64>,
}

/// 하루 유형 × 반복마다 한 시나리오 파일(02c §2.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenarioEntry {
    pub day_type: String,
    pub rep: u32,
    pub seed: u64,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    #[serde(default)]
    pub bundle_id: String,
    pub ontology_sha256: String,
    #[serde(default)]
    pub ontology_validation: serde_json::Value,
    #[serde(default)]
    pub space_id: String,
    pub files: BTreeMap<String, Vec<FileEntry>>,
    #[serde(default)]
    pub scenarios: Vec<ScenarioEntry>,
    #[serde(default)]
    pub pending_inputs: Vec<String>,
    #[serde(default)]
    pub assumption_flags: Vec<String>,
    #[serde(default)]
    pub producer: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct Bundle {
    pub dir: PathBuf,
    /// manifest 경로의 기준(저장소 루트).
    pub root: PathBuf,
    pub manifest: BundleManifest,
    pub nodes: Vec<NodeRow>,
    pub edges: Vec<EdgeRow>,
    pub buildings: Vec<BuildingRow>,
    pub places: Vec<PlaceRow>,
    pub portals: Vec<PortalRow>,
    pub profiles: Vec<Profile>,
    pub access: Vec<AccessRow>,
    pub origins: Vec<OriginRow>,
    pub competitors: Vec<CompetitorRow>,
    pub background: Vec<BgFlowRow>,
    pub prior_crowd: Option<Vec<f64>>,
    pub attrs: Option<Vec<ProfileAttrs>>,
}

/// Rust 필수 역할(정확히 1개, profiles만 1개 이상)과 선택 역할(0~1개). 그 밖 역할(areas·evidence)은 Python 전용이라 무시한다.
pub const REQUIRED_FILES: [&str; 8] = ["profiles", "nodes", "edges", "buildings", "places", "portals", "access", "competitors"];
pub const OPTIONAL_FILES: [&str; 4] = ["background_flows", "profile_attrs", "origins", "prior_crowd"];

/// manifest의 경로를 푼다. 절대 경로는 그대로, 상대 경로는 저장소 루트 기준.
pub fn resolve_path(root: &Path, p: &str) -> PathBuf {
    let pb = Path::new(p);
    if pb.is_absolute() {
        pb.to_path_buf()
    } else {
        root.join(pb)
    }
}

/// 선택 열(bool). 없으면 false.
fn opt_bool(b: &RecordBatch, name: &str, f: &str) -> Result<Vec<bool>> {
    if has_column(b, name) {
        col_bool(b, name, f)
    } else {
        Ok(vec![false; b.num_rows()])
    }
}

/// 선택 열(float, 널 허용). 없으면 모두 None.
fn opt_f64(b: &RecordBatch, name: &str, f: &str) -> Result<Vec<Option<f64>>> {
    if has_column(b, name) {
        col_f64_opt(b, name, f)
    } else {
        Ok(vec![None; b.num_rows()])
    }
}

/// list<string> 열을 행마다 문자열 목록으로.
fn col_str_list(b: &RecordBatch, name: &str, f: &str) -> Result<Vec<Vec<String>>> {
    use arrow_array::{Array, ListArray, StringArray};
    let a = b.column_by_name(name).ok_or_else(|| bad(f, format!("필수 열 '{name}' 없음")))?;
    let la = a.as_any().downcast_ref::<ListArray>().ok_or_else(|| bad(f, format!("열 '{name}'은 list<string>이어야 한다")))?;
    let mut out = Vec::with_capacity(la.len());
    for i in 0..la.len() {
        if la.is_null(i) {
            out.push(Vec::new());
            continue;
        }
        let v = la.value(i);
        let c = arrow_cast::cast(&v, &arrow_schema::DataType::Utf8).map_err(|e| bad(f, format!("열 '{name}': {e}")))?;
        let sa = c.as_any().downcast_ref::<StringArray>().unwrap();
        out.push((0..sa.len()).filter(|&j| !sa.is_null(j)).map(|j| sa.value(j).to_string()).collect());
    }
    Ok(out)
}

fn bad(file: &str, msg: impl Into<String>) -> SimError {
    SimError::contract(file, msg)
}

fn check_range(file: &str, col: &str, i: usize, v: f64, lo: f64, hi: f64) -> Result<()> {
    if v < lo || v > hi || !v.is_finite() {
        return Err(bad(file, format!("열 '{col}' {i}행: 값 {v}이 범위 [{lo}, {hi}] 밖")));
    }
    Ok(())
}

impl Bundle {
    /// 번들 폴더(`dir/manifest.json`)를 읽는다. 경로는 `root` 기준이다. 정의 해시가 다르면 거부한다(DR-215).
    pub fn load(dir: &Path, root: &Path, defs: &Defs) -> Result<Bundle> {
        let mpath = dir.join("manifest.json");
        let text = std::fs::read_to_string(&mpath).map_err(|e| bad("manifest.json", format!("{}: {e}", mpath.display())))?;
        let manifest: BundleManifest = serde_json::from_str(&text).map_err(|e| bad("manifest.json", e.to_string()))?;
        if manifest.schema_version != SCHEMA_VERSION {
            return Err(bad("manifest.json", format!("schema_version {}은 이 엔진이 읽는 {SCHEMA_VERSION}과 다르다", manifest.schema_version)));
        }
        if manifest.ontology_sha256 != defs.sha256 {
            return Err(SimError::Defs(format!("정의 해시 불일치: manifest {} ≠ 정의 폴더 {}", manifest.ontology_sha256, defs.sha256)));
        }
        let mut paths: BTreeMap<String, Vec<(PathBuf, Option<u64>)>> = BTreeMap::new();
        for key in REQUIRED_FILES.iter().chain(OPTIONAL_FILES.iter()) {
            let entries = manifest.files.get(*key).map(|v| v.as_slice()).unwrap_or(&[]);
            let required = REQUIRED_FILES.contains(key);
            let ok = match (*key, entries.len()) {
                ("profiles", n) => n >= 1,
                (_, 1) => true,
                (_, 0) => !required,
                _ => false,
            };
            if !ok {
                return Err(bad("manifest.json", format!("역할 '{key}'의 파일 수 {}가 계약과 다르다(필수는 1개, profiles는 1개 이상, 선택은 0~1개)", entries.len())));
            }
            for entry in entries {
                let p = resolve_path(root, &entry.path);
                let sha = pq::file_sha256(&p).map_err(|e| bad(key, format!("{}: {e}", p.display())))?;
                if sha != entry.sha256 {
                    return Err(bad(key, format!("파일 해시 불일치({})", entry.path)));
                }
                paths.entry(key.to_string()).or_default().push((p, entry.rows));
            }
        }
        let one = |k: &str| -> (PathBuf, Option<u64>) { paths[k][0].clone() };
        let rows_ok = |k: &str, want: Option<u64>, got: usize| -> Result<()> {
            match want {
                Some(w) if w != got as u64 => Err(bad(k, format!("행 수 {got}이 manifest의 {w}와 다르다"))),
                _ => Ok(()),
            }
        };
        let (p, r) = one("nodes");
        let nodes = read_nodes(&p)?;
        rows_ok("nodes", r, nodes.len())?;
        let (p, r) = one("edges");
        let edges = read_edges(&p, defs)?;
        rows_ok("edges", r, edges.len())?;
        let (p, r) = one("buildings");
        let buildings = read_buildings(&p)?;
        rows_ok("buildings", r, buildings.len())?;
        let (p, r) = one("places");
        let places = read_places(&p, defs)?;
        rows_ok("places", r, places.len())?;
        let (p, r) = one("portals");
        let portals = read_portals(&p)?;
        rows_ok("portals", r, portals.len())?;
        let mut profiles = Vec::new();
        for (p, r) in &paths["profiles"] {
            let part = read_profiles(p)?;
            rows_ok("profiles", *r, part.len())?;
            profiles.extend(part);
        }
        let mut ids = BTreeSet::new();
        for pr in &profiles {
            if !ids.insert(pr.persona_id.as_str()) {
                return Err(bad("profiles", format!("persona_id '{}' 중복(파일 사이 포함)", pr.persona_id)));
            }
        }
        let (p, r) = one("access");
        let access = read_access(&p)?;
        rows_ok("access", r, access.len())?;
        let (p, r) = one("competitors");
        let competitors = read_competitors(&p)?;
        rows_ok("competitors", r, competitors.len())?;
        let origins = match paths.get("origins") {
            Some(v) => read_origins(&v[0].0)?,
            None => Vec::new(),
        };
        let background = match paths.get("background_flows") {
            Some(v) => {
                let b = read_background(&v[0].0)?;
                rows_ok("background_flows", v[0].1, b.len())?;
                b
            }
            None => Vec::new(),
        };
        let prior_crowd = match paths.get("prior_crowd") {
            Some(v) => Some(read_prior(&v[0].0)?),
            None => None,
        };
        let attrs = match paths.get("profile_attrs") {
            Some(v) => Some(read_attrs(&v[0].0)?),
            None => None,
        };
        let b = Bundle { dir: dir.to_path_buf(), root: root.to_path_buf(), manifest, nodes, edges, buildings, places, portals, profiles, access, origins, competitors, background, prior_crowd, attrs };
        b.check_references()?;
        Ok(b)
    }

    /// 실행할 시나리오 파일이 manifest의 `scenarios`에 있는지(해시로) 확인하고 그 항목을 준다.
    pub fn scenario_entry(&self, scenario_path: &Path) -> Result<&ScenarioEntry> {
        let sha = pq::file_sha256(scenario_path).map_err(|e| bad("scenario", format!("{}: {e}", scenario_path.display())))?;
        self.manifest
            .scenarios
            .iter()
            .find(|e| e.sha256 == sha)
            .ok_or_else(|| bad("scenario", format!("시나리오 파일 {}의 해시가 manifest의 scenarios에 없다", scenario_path.display())))
    }

    fn check_references(&self) -> Result<()> {
        let node_ids: BTreeSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        if node_ids.len() != self.nodes.len() {
            return Err(bad("nodes", "node_id 중복"));
        }
        let mut edge_ids = BTreeSet::new();
        for e in &self.edges {
            if !edge_ids.insert(e.id.as_str()) {
                return Err(bad("edges", format!("edge_id {} 중복", e.id)));
            }
            if !node_ids.contains(e.a.as_str()) || !node_ids.contains(e.b.as_str()) {
                return Err(bad("edges", format!("간선 {}의 a·b 노드가 nodes에 없음", e.id)));
            }
        }
        let mut place_ids = BTreeSet::new();
        for p in &self.places {
            if !place_ids.insert(p.id.as_str()) {
                return Err(bad("places", format!("place_id {} 중복", p.id)));
            }
            if !edge_ids.contains(p.edge_id.as_str()) {
                return Err(bad("places", format!("장소 {}의 edge_id {}가 edges에 없음", p.id, p.edge_id)));
            }
        }
        for p in &self.portals {
            if !node_ids.contains(p.node_id.as_str()) {
                return Err(bad("portals", format!("출입구 {}의 node_id {}가 nodes에 없음", p.id, p.node_id)));
            }
        }
        // origins는 선택 입력이다(02c B8). 없으면 출발지 목록은 접근 시간 표에서 얻는다.
        let origin_ids: BTreeSet<&str> = if self.origins.is_empty() { self.access.iter().map(|a| a.origin_id.as_str()).collect() } else { self.origins.iter().map(|o| o.id.as_str()).collect() };
        let competitor_ids: BTreeSet<&str> = self.competitors.iter().map(|c| c.id.as_str()).collect();
        for a in &self.access {
            if !origin_ids.contains(a.origin_id.as_str()) {
                return Err(bad("access", format!("origin_id {}가 origins에 없음", a.origin_id)));
            }
            if a.destination != "myeongdong" && !competitor_ids.contains(a.destination.as_str()) {
                return Err(bad("access", format!("destination '{}'는 myeongdong이나 경쟁 상권 id여야 한다", a.destination)));
            }
        }
        for p in &self.profiles {
            if !origin_ids.contains(p.origin_id.as_str()) {
                return Err(bad("profiles", format!("인물 {}의 origin_id {}가 origins에 없음", p.persona_id, p.origin_id)));
            }
            for pu in &p.purposes {
                if let Some(t) = &pu.target_place_id {
                    if !place_ids.contains(t.as_str()) {
                        return Err(bad("profiles", format!("인물 {}의 목적 장소 {}가 places에 없음", p.persona_id, t)));
                    }
                }
            }
        }
        // 출발지마다 명동과 모든 경쟁 상권의 접근 시간이 있어야 한다(C-216 ③).
        for o in &origin_ids {
            for dest in std::iter::once("myeongdong").chain(competitor_ids.iter().copied()) {
                if !self.access.iter().any(|a| a.origin_id == *o && a.destination == dest) {
                    return Err(bad("access", format!("출발지 {o}에서 '{dest}'까지의 접근 시간이 없음")));
                }
            }
        }
        Ok(())
    }

    pub fn place_ix_of(&self) -> BTreeMap<String, usize> {
        self.places.iter().enumerate().map(|(i, p)| (p.id.clone(), i)).collect()
    }
}

fn read_nodes(path: &Path) -> Result<Vec<NodeRow>> {
    let f = "nodes";
    let b = read_table(path, f)?;
    let (id, x, z, elev, kind, signal) = (col_str(&b, "node_id", f)?, col_f64(&b, "x", f)?, col_f64(&b, "z", f)?, col_f64(&b, "elev_m", f)?, col_str(&b, "kind", f)?, col_bool(&b, "signal", f)?);
    (0..b.num_rows())
        .map(|i| {
            let kind = match kind[i].as_str() {
                "junction" => NodeKind::Junction,
                "crossing" => NodeKind::Crossing,
                "anchor" => NodeKind::Anchor,
                "portal" => NodeKind::Portal,
                o => return Err(bad(f, format!("{i}행: kind '{o}'"))),
            };
            Ok(NodeRow { id: id[i].clone(), pos: V2::new(x[i], z[i]), elev_m: elev[i], kind, signal: signal[i] })
        })
        .collect()
}

fn read_edges(path: &Path, defs: &Defs) -> Result<Vec<EdgeRow>> {
    let f = "edges";
    let b = read_table(path, f)?;
    let n = b.num_rows();
    let id = col_str(&b, "edge_id", f)?;
    let a = col_str(&b, "a", f)?;
    let bb = col_str(&b, "b", f)?;
    let (attrs, cats) = generic_attrs(&b, defs, f)?;
    let class = col_str(&b, "class", f)?;
    let poly = col_points(&b, "polyline", f)?;
    let length = col_f64(&b, "length_m", f)?;
    let road = col_str(&b, "road_type", f)?;
    let width = col_f64(&b, "width_m", f)?;
    let mode = col_str(&b, "walk_mode", f)?;
    let sw = col_f64(&b, "sidewalk_width_m", f)?;
    let vm = col_bool(&b, "vehicle_mixed", f)?;
    let pz = opt_bool(&b, "ped_zone", f)?;
    let cr = opt_bool(&b, "is_crossing", f)?;
    let cs = opt_f64(&b, "carriage_share", f)?;
    let conv = opt_bool(&b, "conveying", f)?;
    let layer = opt_f64(&b, "layer", f)?;
    let g = col_f64(&b, "grade_pct", f)?;
    let go = opt_f64(&b, "grade_pct_orig", f)?;
    let gs = col_f64(&b, "grade_pct_smooth", f)?;
    let steps = col_bool(&b, "is_steps", f)?;
    let riser = col_f64(&b, "riser_cm", f)?;
    let sc = col_f64(&b, "steps_count", f)?;
    let ad = col_f64(&b, "ad_intensity", f)?;
    let lw = col_str(&b, "label_width", f)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if defs.class(&class[i]).is_none() {
            return Err(bad(f, format!("{i}행: class '{}'가 정의에 없음", class[i])));
        }
        if poly[i].len() < 2 {
            return Err(bad(f, format!("{i}행: polyline 점이 2개 미만")));
        }
        check_range(f, "length_m", i, length[i], 1e-6, 1e5)?;
        check_range(f, "width_m", i, width[i], 0.1, 200.0)?;
        check_range(f, "ad_intensity", i, ad[i], 0.0, 1.0)?;
        check_range(f, "grade_pct", i, g[i], -100.0, 100.0)?;
        check_range(f, "grade_pct_smooth", i, gs[i], -100.0, 100.0)?;
        let carriage_share = cs[i].unwrap_or(0.0);
        check_range(f, "carriage_share", i, carriage_share, 0.0, 0.999)?;
        let walk_mode = match mode[i].as_str() {
            "full" => WalkMode::Full,
            "sidewalks" => WalkMode::Sidewalks,
            "none" => WalkMode::NoWalk,
            o => return Err(bad(f, format!("{i}행: walk_mode '{o}'"))),
        };
        if walk_mode == WalkMode::Sidewalks && !(sw[i] > 0.0 && 2.0 * sw[i] <= width[i]) {
            return Err(bad(f, format!("{i}행: 보도 폭 {}이 차도 폭 {}과 맞지 않음", sw[i], width[i])));
        }
        out.push(EdgeRow {
            id: id[i].clone(),
            a: a[i].clone(),
            b: bb[i].clone(),
            class: class[i].clone(),
            polyline: poly[i].clone(),
            length_m: length[i],
            road_type: road[i].clone(),
            width_m: width[i],
            walk_mode,
            sidewalk_width_m: sw[i],
            vehicle_mixed: vm[i],
            ped_zone: pz[i],
            is_crossing: cr[i],
            carriage_share,
            grade_pct: g[i],
            grade_pct_orig: go[i],
            grade_pct_smooth: gs[i],
            conveying: conv[i],
            layer: layer[i].unwrap_or(0.0),
            is_steps: steps[i],
            riser_cm: riser[i],
            steps_count: sc[i],
            ad_intensity: ad[i],
            label_width: lw[i].clone(),
            attrs: attrs[i].clone(),
            cats: cats[i].clone(),
        });
    }
    Ok(out)
}

fn read_buildings(path: &Path) -> Result<Vec<BuildingRow>> {
    let f = "buildings";
    let b = read_table(path, f)?;
    let (id, fp, h) = (col_str(&b, "building_id", f)?, col_points(&b, "footprint", f)?, col_f64(&b, "height_m", f)?);
    (0..b.num_rows())
        .map(|i| {
            if fp[i].len() < 3 {
                return Err(bad(f, format!("{i}행: footprint 점이 3개 미만")));
            }
            Ok(BuildingRow { id: id[i].clone(), footprint: fp[i].clone(), height_m: h[i] })
        })
        .collect()
}

fn read_places(path: &Path, defs: &Defs) -> Result<Vec<PlaceRow>> {
    let f = "places";
    let b = read_table(path, f)?;
    let n = b.num_rows();
    let id = col_str(&b, "place_id", f)?;
    let class = col_str(&b, "class", f)?;
    let (attrs, cats) = generic_attrs(&b, defs, f)?;
    let offset = if has_column(&b, "offset_m") { col_f64_opt(&b, "offset_m", f)? } else { vec![None; b.num_rows()] };
    let side = if has_column(&b, "side") { col_str(&b, "side", f)?.into_iter().map(|s| if s.is_empty() { None } else { Some(s) }).collect() } else { vec![None; b.num_rows()] };
    let category = col_str(&b, "category", f)?;
    let name = col_str(&b, "name", f)?;
    let (x, z, ax, az) = (col_f64(&b, "x", f)?, col_f64(&b, "z", f)?, col_f64(&b, "anchor_x", f)?, col_f64(&b, "anchor_z", f)?);
    let edge = col_str(&b, "edge_id", f)?;
    let price = col_f64_opt(&b, "price_level_krw", f)?;
    let disc = col_f64(&b, "discount_pct", f)?;
    let (open, close) = (col_f64(&b, "open_min", f)?, col_f64(&b, "close_min", f)?);
    let day_types = col_str_list(&b, "day_types", f)?;
    let vis = col_f64(&b, "visibility", f)?;
    let fame = opt_f64(&b, "fame", f)?;
    let rate = col_f64_opt(&b, "service_rate_per_min", f)?;
    let cap = col_f64_opt(&b, "capacity", f)?;
    let seats = opt_f64(&b, "seats", f)?;
    let foot = opt_f64(&b, "footprint_m2", f)?;
    let sense = opt_f64(&b, "sense", f)?;
    let layer = opt_f64(&b, "layer", f)?;
    let (rent, staff, wage, revenue) = (opt_f64(&b, "rent_krw_month", f)?, opt_f64(&b, "staff_count", f)?, opt_f64(&b, "wage_krw_month", f)?, opt_f64(&b, "revenue_krw_month", f)?);
    let floor = opt_f64(&b, "floor", f)?;
    let foreign = if has_column(&b, "foreign_label") { col_bool(&b, "foreign_label", f)? } else { vec![false; n] };
    let source = col_str(&b, "source", f)?;
    let label = col_str(&b, "label", f)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if defs.class(&class[i]).is_none() {
            return Err(bad(f, format!("{i}행: class '{}'가 정의에 없음", class[i])));
        }
        check_range(f, "visibility", i, vis[i], 0.0, 1.0)?;
        check_range(f, "discount_pct", i, disc[i], 0.0, 90.0)?;
        check_range(f, "open_min", i, open[i], 0.0, 2880.0)?;
        check_range(f, "close_min", i, close[i], 0.0, 2880.0)?;
        if open[i] >= close[i] {
            return Err(bad(f, format!("{i}행: open_min {} ≥ close_min {}", open[i], close[i])));
        }
        let price_i = price[i].unwrap_or(0.0);
        let rate_i = rate[i].unwrap_or(0.0);
        check_range(f, "price_level_krw", i, price_i, 0.0, 1e7)?;
        check_range(f, "service_rate_per_min", i, rate_i, 0.0, 1000.0)?;
        if let Some(c) = cap[i] {
            check_range(f, "capacity", i, c, 0.0, 1e5)?;
        }
        let dts = day_types[i].iter().map(|d| DayType::parse(d).ok_or_else(|| bad(f, format!("{i}행: day_types의 '{d}'")))).collect::<Result<Vec<_>>>()?;
        out.push(PlaceRow {
            id: id[i].clone(),
            class: class[i].clone(),
            category: category[i].clone(),
            name: name[i].clone(),
            pos: V2::new(x[i], z[i]),
            anchor: V2::new(ax[i], az[i]),
            edge_id: edge[i].clone(),
            offset_m: offset[i],
            side: side[i].clone(),
            price_level_krw: price_i,
            discount_pct: disc[i],
            open_min: open[i],
            close_min: close[i],
            day_types: dts,
            visibility: vis[i],
            fame: fame[i].unwrap_or(0.0),
            service_rate_per_min: rate_i,
            capacity: cap[i],
            seats: seats[i],
            footprint_m2: foot[i],
            sense: sense[i],
            floor: floor[i].unwrap_or(0.0),
            layer: layer[i].unwrap_or(0.0),
            foreign_label: foreign[i],
            rent_krw_month: rent[i],
            staff_count: staff[i],
            wage_krw_month: wage[i],
            revenue_krw_month: revenue[i],
            source: source[i].clone(),
            label: label[i].clone(),
            attrs: attrs[i].clone(),
            cats: cats[i].clone(),
        });
    }
    Ok(out)
}

fn read_portals(path: &Path) -> Result<Vec<PortalRow>> {
    let f = "portals";
    let b = read_table(path, f)?;
    let (id, kind, x, z, node, name, hw) = (col_str(&b, "portal_id", f)?, col_str(&b, "kind", f)?, col_f64(&b, "x", f)?, col_f64(&b, "z", f)?, col_str(&b, "node_id", f)?, col_str(&b, "name", f)?, col_f64_opt(&b, "headway_min", f)?);
    let groups = if has_column(&b, "groups") { Some(col_str_list(&b, "groups", f)?) } else { None };
    (0..b.num_rows())
        .map(|i| {
            let kind = PortalKind::parse(&kind[i]).ok_or_else(|| bad(f, format!("{i}행: kind '{}'", kind[i])))?;
            let headway = hw[i].unwrap_or(0.0);
            if kind == PortalKind::Subway && !(headway > 0.0) {
                return Err(bad(f, format!("{i}행: 지하철 출입구의 headway_min이 없음")));
            }
            let groups = match &groups {
                None => vec![Group::Domestic, Group::Tourist],
                Some(g) => g[i]
                    .iter()
                    .map(|s| match s.as_str() {
                        "domestic" => Ok(Group::Domestic),
                        "tourist" => Ok(Group::Tourist),
                        o => Err(bad(f, format!("{i}행: groups의 '{o}'"))),
                    })
                    .collect::<Result<Vec<_>>>()?,
            };
            Ok(PortalRow { id: id[i].clone(), kind, pos: V2::new(x[i], z[i]), node_id: node[i].clone(), name: name[i].clone(), headway_min: headway, groups })
        })
        .collect()
}

fn read_profiles(path: &Path) -> Result<Vec<Profile>> {
    let f = "profiles";
    let b = read_table(path, f)?;
    for forbidden in ["age", "sex", "nationality", "income_krw"] {
        if has_column(&b, forbidden) {
            return Err(bad(f, format!("반응 입력에 '{forbidden}' 열이 있으면 안 된다(DR-214, C-205 ②) — profile_attrs로 옮길 것")));
        }
    }
    let n = b.num_rows();
    let pid = col_str(&b, "persona_id", f)?;
    let group = col_str(&b, "group", f)?;
    let origin = col_str(&b, "origin_id", f)?;
    let mut comfort = vec![[0.0; ND]; n];
    let mut limit = vec![[0.0; ND]; n];
    let mut unc = vec![[0.0; ND]; n];
    let mut labels = vec![vec![String::new(); ND]; n];
    let mut sens = vec![[0.0; ND]; n];
    for d in Dim::ALL {
        let k = d.key();
        let s = col_f64(&b, &format!("sens_{k}"), f)?;
        if d == Dim::Time {
            // D-TIME은 능력 열이 없다. 한계는 런타임이 남은 시간으로 준다(02c §2.4 (1)).
            for i in 0..n {
                if s[i] < 0.0 {
                    return Err(bad(f, format!("{i}행: sens_{k}가 음수")));
                }
                sens[i][d.ix()] = s[i];
                labels[i][d.ix()] = "runtime".into();
            }
            continue;
        }
        let c = col_f64(&b, &format!("cap_{k}_comfort"), f)?;
        let l = col_f64(&b, &format!("cap_{k}_limit"), f)?;
        let u = col_f64(&b, &format!("cap_{k}_uncertainty"), f)?;
        let lab = col_str(&b, &format!("cap_{k}_label"), f)?;
        for i in 0..n {
            if !(0.0 <= c[i] && c[i] < l[i]) {
                return Err(bad(f, format!("{i}행: cap_{k}_comfort {}와 limit {}가 0 ≤ comfort < limit를 어김", c[i], l[i])));
            }
            if s[i] < 0.0 {
                return Err(bad(f, format!("{i}행: sens_{k}가 음수")));
            }
            comfort[i][d.ix()] = c[i];
            limit[i][d.ix()] = l[i];
            unc[i][d.ix()] = u[i];
            labels[i][d.ix()] = lab[i].clone();
            sens[i][d.ix()] = s[i];
        }
    }
    let mut interest = vec![[0.0; NI]; n];
    for it in Interest::ALL {
        let v = col_f64(&b, &format!("interest_{}", it.key()), f)?;
        for i in 0..n {
            if v[i] < 0.0 {
                return Err(bad(f, format!("{i}행: interest_{}가 음수", it.key())));
            }
            interest[i][it.ix()] = v[i];
        }
    }
    let get_col = |name: &str| col_f64(&b, name, f);
    let (risk, punct, expl, att, tau, speed) = (get_col("risk")?, get_col("punctuality")?, get_col("exploration")?, get_col("attention")?, get_col("tau")?, get_col("base_speed_mps")?);
    let (child, stroller, luggage) = (get_col("child_prob")?, get_col("stroller_prob")?, get_col("luggage_prob")?);
    let (fr, fa, pr, pa, ht, md) = (get_col("focal_range_m")?, get_col("focal_angle_deg")?, get_col("peripheral_range_m")?, get_col("peripheral_angle_deg")?, get_col("head_turn_deg")?, get_col("min_dwell_s")?);
    let purposes = col_struct_list(&b, "purposes", f)?;
    let avail = col_struct_list(&b, "availability", f)?;
    let evidence = if has_column(&b, "evidence") { col_struct_list(&b, "evidence", f)? } else { vec![Vec::new(); n] };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        for (name, v, lo, hi) in [
            ("risk", risk[i], 0.0, 1.0),
            ("punctuality", punct[i], 0.0, 1.0),
            ("exploration", expl[i], 0.0, 1.0),
            ("attention", att[i], 0.0, 1.0),
            ("tau", tau[i], 1e-3, 100.0),
            ("base_speed_mps", speed[i], 0.1, 3.0),
            ("child_prob", child[i], 0.0, 1.0),
            ("stroller_prob", stroller[i], 0.0, 1.0),
            ("luggage_prob", luggage[i], 0.0, 1.0),
            ("focal_range_m", fr[i], 0.5, 200.0),
            ("focal_angle_deg", fa[i], 1.0, 360.0),
            ("peripheral_range_m", pr[i], 0.5, 100.0),
            ("peripheral_angle_deg", pa[i], 1.0, 360.0),
            ("head_turn_deg", ht[i], 0.0, 180.0),
            ("min_dwell_s", md[i], 0.0, 60.0),
        ] {
            check_range(f, name, i, v, lo, hi)?;
        }
        let group = match group[i].as_str() {
            "domestic" => Group::Domestic,
            "tourist" => Group::Tourist,
            o => return Err(bad(f, format!("{i}행: group '{o}'"))),
        };
        let purposes = purposes[i]
            .iter()
            .map(|r| {
                let purpose = get(r, "purpose").and_then(|v| v.s()).unwrap_or("other").to_string();
                let it = match get(r, "interest").and_then(|v| v.s()) {
                    None | Some("") => None,
                    Some(s) => Some(Interest::parse(s).ok_or_else(|| bad(f, format!("{i}행: 목적의 interest '{s}'가 관심 차원이 아님")))?),
                };
                let value = get(r, "value").and_then(|v| v.f()).ok_or_else(|| bad(f, format!("{i}행: 목적의 value 없음")))?;
                let target = match get(r, "target_place_id") {
                    Some(Scalar::S(s)) if !s.is_empty() => Some(s.clone()),
                    Some(Scalar::F(v)) => Some(format!("{}", *v as i64)),
                    _ => None,
                };
                Ok(Purpose { purpose, interest: it, value, target_place_id: target })
            })
            .collect::<Result<Vec<_>>>()?;
        let availability = avail[i]
            .iter()
            .map(|r| {
                let dt = get(r, "day_type").and_then(|v| v.s()).and_then(DayType::parse).ok_or_else(|| bad(f, format!("{i}행: availability의 day_type")))?;
                let s = get(r, "start_min").and_then(|v| v.f()).ok_or_else(|| bad(f, format!("{i}행: availability의 start_min")))?;
                let e = get(r, "end_min").and_then(|v| v.f()).ok_or_else(|| bad(f, format!("{i}행: availability의 end_min")))?;
                if !(0.0 <= s && s < e && e <= 1440.0) {
                    return Err(bad(f, format!("{i}행: availability 구간 [{s}, {e})가 잘못됨")));
                }
                Ok(Avail { day_type: dt, start_min: s, end_min: e })
            })
            .collect::<Result<Vec<_>>>()?;
        let evidence = evidence[i]
            .iter()
            .map(|r| EvidenceRef {
                target: get(r, "target").and_then(|v| v.s()).unwrap_or("").to_string(),
                field: get(r, "field").and_then(|v| v.s()).unwrap_or("").to_string(),
                start: get(r, "start").and_then(|v| v.f()).unwrap_or(-1.0) as i64,
                end: get(r, "end").and_then(|v| v.f()).unwrap_or(-1.0) as i64,
                rule_id: get(r, "rule_id").and_then(|v| v.s()).unwrap_or("").to_string(),
                label: get(r, "label").and_then(|v| v.s()).unwrap_or("").to_string(),
            })
            .collect();
        out.push(Profile {
            persona_id: pid[i].clone(),
            group,
            origin_id: origin[i].clone(),
            comfort: comfort[i],
            limit: limit[i],
            uncertainty: unc[i],
            cap_label: labels[i].clone(),
            sens: sens[i],
            interest: interest[i],
            risk: risk[i],
            punctuality: punct[i],
            exploration: expl[i],
            attention: att[i],
            tau: tau[i],
            base_speed_mps: speed[i],
            child_prob: child[i],
            stroller_prob: stroller[i],
            luggage_prob: luggage[i],
            perc: Perception { focal_range_m: fr[i], focal_angle_deg: fa[i], peripheral_range_m: pr[i], peripheral_angle_deg: pa[i], head_turn_deg: ht[i], min_dwell_s: md[i] },
            purposes,
            availability,
            evidence,
        });
    }
    let mut ids = BTreeSet::new();
    for p in &out {
        if !ids.insert(p.persona_id.clone()) {
            return Err(bad(f, format!("persona_id '{}' 중복", p.persona_id)));
        }
    }
    Ok(out)
}

fn read_access(path: &Path) -> Result<Vec<AccessRow>> {
    let f = "access";
    let b = read_table(path, f)?;
    let (o, d, m, base, fare, lab) = (col_str(&b, "origin_id", f)?, col_str(&b, "destination", f)?, col_str(&b, "mode", f)?, col_f64(&b, "base_min", f)?, col_f64(&b, "fare_krw", f)?, col_str(&b, "label", f)?);
    (0..b.num_rows())
        .map(|i| {
            let mode = Mode::parse(&m[i]).ok_or_else(|| bad(f, format!("{i}행: mode '{}'", m[i])))?;
            check_range(f, "base_min", i, base[i], 0.0, 600.0)?;
            check_range(f, "fare_krw", i, fare[i], 0.0, 1e6)?;
            Ok(AccessRow { origin_id: o[i].clone(), destination: d[i].clone(), mode, base_min: base[i], fare_krw: fare[i], label: lab[i].clone() })
        })
        .collect()
}

fn read_origins(path: &Path) -> Result<Vec<OriginRow>> {
    let f = "origins";
    let b = read_table(path, f)?;
    let (id, x, z) = (col_str(&b, "origin_id", f)?, col_f64(&b, "x", f)?, col_f64(&b, "z", f)?);
    Ok((0..b.num_rows()).map(|i| OriginRow { id: id[i].clone(), pos: V2::new(x[i], z[i]) }).collect())
}

fn read_competitors(path: &Path) -> Result<Vec<CompetitorRow>> {
    let f = "competitors";
    let b = read_table(path, f)?;
    let n = b.num_rows();
    let (id, name, price, crowd, lab) = (col_str(&b, "id", f)?, col_str(&b, "name", f)?, col_f64(&b, "price_level", f)?, col_f64(&b, "crowd_level", f)?, col_str(&b, "label", f)?);
    let mut interest = vec![[0.0; NI]; n];
    for it in Interest::ALL {
        let v = col_f64(&b, &format!("interest_{}", it.key()), f)?;
        for i in 0..n {
            interest[i][it.ix()] = v[i];
        }
    }
    (0..n)
        .map(|i| {
            if id[i] == "myeongdong" {
                return Err(bad(f, "경쟁 상권 id로 'myeongdong'은 쓸 수 없다"));
            }
            if lab[i].is_empty() {
                return Err(bad(f, format!("{i}행: 출처 라벨 없음(C-216 ④)")));
            }
            Ok(CompetitorRow { id: id[i].clone(), name: name[i].clone(), interest: interest[i], price_level: price[i], crowd_level: crowd[i], label: lab[i].clone() })
        })
        .collect()
}

fn read_background(path: &Path) -> Result<Vec<BgFlowRow>> {
    let f = "background_flows";
    let b = read_table(path, f)?;
    let (h, k, c) = (col_i64(&b, "hour", f)?, col_str(&b, "portal_kind", f)?, col_f64(&b, "count", f)?);
    (0..b.num_rows())
        .map(|i| {
            if !(0..24).contains(&h[i]) {
                return Err(bad(f, format!("{i}행: hour {}", h[i])));
            }
            let kind = PortalKind::parse(&k[i]).ok_or_else(|| bad(f, format!("{i}행: portal_kind '{}'", k[i])))?;
            check_range(f, "count", i, c[i], 0.0, 1e6)?;
            Ok(BgFlowRow { hour: h[i] as u32, portal_kind: kind, count: c[i] })
        })
        .collect()
}

fn read_prior(path: &Path) -> Result<Vec<f64>> {
    let f = "prior_crowd";
    let b = read_table(path, f)?;
    let (slot, d) = (col_i64(&b, "slot", f)?, col_f64(&b, "density", f)?);
    let mut out = vec![f64::NAN; 144];
    for i in 0..b.num_rows() {
        if !(0..144).contains(&slot[i]) {
            return Err(bad(f, format!("{i}행: slot {}", slot[i])));
        }
        out[slot[i] as usize] = d[i];
    }
    if out.iter().any(|v| v.is_nan()) {
        return Err(bad(f, "144개 슬롯이 모두 있어야 한다"));
    }
    Ok(out)
}

fn read_attrs(path: &Path) -> Result<Vec<ProfileAttrs>> {
    let f = "profile_attrs";
    let b = read_table(path, f)?;
    let pid = col_str(&b, "persona_id", f)?;
    let age = if has_column(&b, "age") { col_f64_opt(&b, "age", f)? } else { vec![None; b.num_rows()] };
    let band: Vec<Option<String>> = if has_column(&b, "age_band") { col_str(&b, "age_band", f)?.into_iter().map(|s| if s.is_empty() { None } else { Some(s) }).collect() } else { vec![None; b.num_rows()] };
    let sex = if has_column(&b, "sex") { col_str(&b, "sex", f)?.into_iter().map(|s| if s.is_empty() { None } else { Some(s) }).collect() } else { vec![None; b.num_rows()] };
    let nat = if has_column(&b, "nationality") { col_str(&b, "nationality", f)?.into_iter().map(|s| if s.is_empty() { None } else { Some(s) }).collect() } else { vec![None; b.num_rows()] };
    let inc = if has_column(&b, "income_krw") { col_f64_opt(&b, "income_krw", f)? } else { vec![None; b.num_rows()] };
    Ok((0..b.num_rows()).map(|i| ProfileAttrs { persona_id: pid[i].clone(), age: age[i], age_band: band[i].clone(), sex: sex[i].clone(), nationality: nat[i].clone(), income_krw: inc[i] }).collect())
}

type AttrMaps = (Vec<BTreeMap<String, f64>>, Vec<BTreeMap<String, String>>);

/// 클래스 속성과 같은 이름의 열을 읽는다(02c B6). 범주형은 문자열, 그 밖은 숫자(널은 건너뜀).
fn generic_attrs(b: &RecordBatch, defs: &Defs, f: &str) -> Result<AttrMaps> {
    let n = b.num_rows();
    let mut nums = vec![BTreeMap::new(); n];
    let mut cats = vec![BTreeMap::new(); n];
    for (name, categorical) in defs.attr_names() {
        if !has_column(b, &name) {
            continue;
        }
        if categorical {
            let v = col_str(b, &name, f)?;
            for i in 0..n {
                if !v[i].is_empty() {
                    cats[i].insert(name.clone(), v[i].clone());
                }
            }
        } else {
            let v = col_f64_opt(b, &name, f)?;
            for i in 0..n {
                if let Some(x) = v[i] {
                    nums[i].insert(name.clone(), x);
                }
            }
        }
    }
    Ok((nums, cats))
}
