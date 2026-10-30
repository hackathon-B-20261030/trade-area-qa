//! 실행 폴더 쓰기 (02c §2.6, 02d I-226-r).
//!
//! Python이 읽는 표(agents·events·decisions·decision_alts·metrics_10min)와 manifest.json을 쓴다.
//! ID 열은 문자열이고 없으면 null이다. 실행 시각 같은 비결정 값은 넣지 않는다(같은 입력 → 같은 바이트).

use crate::record::{AltRow, DecisionRow, EventRow, IdMaps, RowsPart};
use crate::situation::Context;
use crate::step::{SimState, World};
use crate::visit::{VisitDecision, VisitKind};
use mdsim_core::dims::{Dim, ND};
use mdsim_core::pq::{ColData, StreamWriter, Table};
use mdsim_core::rng::canonical_key;
use mdsim_core::scenario::fmt_hhmm;
use mdsim_core::{Result, SimError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const RUN_SCHEMA_VERSION: u32 = 1;
pub const ENGINE_NAME: &str = "mdsim";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileRec {
    pub path: String,
    pub sha256: String,
    pub rows: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngineRec {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowRec {
    pub start: String,
    pub end: String,
    pub start_min: f64,
    pub end_min: f64,
}

/// 실행 manifest(02c §2.6). Python이 읽는 필드 + Rust 기록용 필드.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub bundle_id: String,
    pub ontology_sha256: String,
    pub dimensions_version: String,
    pub engine: EngineRec,
    pub scenario_id: String,
    pub scenario_sha256: String,
    pub seed: u64,
    pub rep: u32,
    pub day_type: String,
    pub window: WindowRec,
    pub dt_s: f64,
    pub render_every_ticks: u32,
    pub representation_ratio: f64,
    pub interactions: bool,
    pub configuration: String,
    pub months: u32,
    /// complete | failed
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    /// 몸·사건·결정 수 등(점검용).
    pub counts: BTreeMap<String, f64>,
    /// 표 이름 → 경로·해시·행 수.
    pub files: BTreeMap<String, FileRec>,
    /// 렌더 묶음 파일 → 해시(Rust 전용).
    pub frames: BTreeMap<String, String>,
    /// 정의의 결정 맥락에 있지만 이 엔진이 대안으로 만들지 않는 행동(맥락 → 행동).
    pub unsupported_actions: BTreeMap<String, Vec<String>>,
    /// 번들 manifest의 가정 표시를 그대로 옮긴다.
    pub pending_inputs: Vec<String>,
    pub assumption_flags: Vec<String>,
}

/// 엔진이 만드는 결정 맥락별 행동(02d I-204-r). contexts.yaml과 비교해 manifest에 남긴다.
pub fn engine_actions(ctx: &str) -> &'static [&'static str] {
    match ctx {
        "VISIT" => &["VISIT_NOW", "VISIT_LATER", "GO_ELSEWHERE", "SKIP"],
        "NEXT" => &["GO_TO", "WATCH", "EXPLORE", "LEAVE", "REST_SEAT"],
        "ROUTE" => &["PROCEED", "DETOUR", "PACE", "DEFER", "ABANDON"],
        "REST" => &["REST_SEAT", "REST_PAID", "CONTINUE", "LEAVE"],
        "PURCHASE" => &["BUY", "NO_BUY"],
        "LOOK" => &["PATH", "FOOTING", "LOOK_AT"],
        _ => &[],
    }
}

pub fn unsupported_actions(w: &World) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for (id, ctx) in &w.defs.contexts {
        let have = engine_actions(id);
        let miss: Vec<String> = ctx.actions.iter().filter(|a| !have.contains(&a.as_str())).cloned().collect();
        if !miss.is_empty() {
            out.insert(id.clone(), miss);
        }
    }
    out
}

/// 몸·장소 인덱스 → 문자열 ID.
pub fn id_maps(w: &World, state: &SimState) -> IdMaps {
    IdMaps { body_id: state.bodies.iter().map(|b| b.entity.clone()).collect(), place_id: w.space.places.iter().map(|p| p.id.clone()).collect() }
}

fn persona_of(w: &World, state: &SimState, body: i64) -> Option<String> {
    if body < 0 {
        return None;
    }
    let b = state.bodies.get(body as usize)?;
    b.profile.map(|i| w.bundle.profiles[i as usize].persona_id.clone())
}

pub fn agents_table(w: &World, state: &SimState) -> Table {
    let bs = &state.bodies;
    let portal = |i: u32| w.space.portals[i as usize].id.clone();
    let group = |b: &crate::state::Body| match b.profile {
        Some(i) => match w.bundle.profiles[i as usize].group {
            mdsim_core::bundle::Group::Domestic => "domestic".to_string(),
            mdsim_core::bundle::Group::Tourist => "tourist".to_string(),
        },
        None => "background".to_string(),
    };
    Table::new()
        .col("body_id", ColData::Str(bs.iter().map(|b| b.entity.clone()).collect()))
        .col("persona_id", ColData::StrOpt(bs.iter().map(|b| b.profile.map(|i| w.bundle.profiles[i as usize].persona_id.clone())).collect()))
        .col("replicate", ColData::I32(bs.iter().map(|b| b.replicate as i32).collect()))
        .col("group", ColData::Str(bs.iter().map(group).collect()))
        .col("arrival_tick", ColData::I64(bs.iter().map(|b| b.arrival_tick as i64).collect()))
        .col("departure_tick", ColData::I64(bs.iter().map(|b| b.departed_tick.map(|t| t as i64).unwrap_or(-1)).collect()))
        .col("portal_in", ColData::Str(bs.iter().map(|b| portal(b.portal_in)).collect()))
        .col("portal_out", ColData::StrOpt(bs.iter().map(|b| b.portal_out.map(portal)).collect()))
        .col("mode", ColData::Str(bs.iter().map(|b| b.mode.as_str().to_string()).collect()))
        .col("spent_krw", ColData::F64(bs.iter().map(|b| b.spent).collect()))
        .col("sense_over_s", ColData::F64(bs.iter().map(|b| b.stats.sense_over_s).collect()))
        .col("way_over", ColData::Bool(bs.iter().map(|b| b.stats.way_over).collect()))
        .col("distance_m", ColData::F64(bs.iter().map(|b| b.stats.distance_m).collect()))
        .col("visits", ColData::I32(bs.iter().map(|b| b.stats.visits as i32).collect()))
        .col("purchases", ColData::I32(bs.iter().map(|b| b.stats.purchases as i32).collect()))
        .col("notices", ColData::I32(bs.iter().map(|b| b.stats.notices as i32).collect()))
        .col("max_fatigue", ColData::F64(bs.iter().map(|b| b.stats.max_fatigue).collect()))
        .col("decisions", ColData::I32(bs.iter().map(|b| b.stats.decisions as i32).collect()))
}

pub fn events_table(events: &[EventRow], ids: &IdMaps) -> Table {
    Table::new()
        .col("tick", ColData::I64(events.iter().map(|e| e.tick as i64).collect()))
        .col("t_s", ColData::F64(events.iter().map(|e| e.t_s).collect()))
        .col("body_id", ColData::StrOpt(events.iter().map(|e| ids.body(e.body)).collect()))
        .col("kind", ColData::Str(events.iter().map(|e| e.kind.clone()).collect()))
        .col("place_id", ColData::StrOpt(events.iter().map(|e| ids.place(e.place)).collect()))
        .col("value", ColData::F64(events.iter().map(|e| e.value).collect()))
        .col("info", ColData::Str(events.iter().map(|e| e.info.clone()).collect()))
}

/// 결정 한 줄(표 쓰기 전 형태).
struct DecOut {
    id: String,
    tick: i64,
    body_id: Option<String>,
    persona_id: Option<String>,
    context: String,
    trigger: String,
    n_alts: i32,
    chosen: i32,
    forced: bool,
    rng_key: Option<String>,
    top: Vec<(String, f64)>,
    access: Option<(f64, f64, f64)>,
}

struct AltOut {
    decision: String,
    alt: i32,
    action: String,
    target: Option<String>,
    prob: f64,
    utility: f64,
    purpose: f64,
    interest: f64,
    cost: f64,
    time: f64,
    money: f64,
    cost_dims: [f64; ND],
    access: Option<(f64, f64, f64)>,
}

fn visit_rows(w: &World, visit: &[VisitDecision]) -> (Vec<DecOut>, Vec<AltOut>) {
    let mut ds = Vec::with_capacity(visit.len());
    let mut als = Vec::new();
    for d in visit {
        let prof = &w.bundle.profiles[d.profile as usize];
        let id = format!("V:{}", prof.persona_id);
        let first_slot = d.alts.iter().filter_map(|a| if let VisitKind::Myeongdong { slot_start_min, .. } = a.kind { Some(slot_start_min) } else { None }).fold(f64::INFINITY, f64::min);
        // 명동 대안의 확률 가중 접근 시간(명동 확률로 정규화)
        let (mut pm, mut acc) = (0.0, (0.0, 0.0, 0.0));
        for a in &d.alts {
            if let VisitKind::Myeongdong { .. } = a.kind {
                pm += a.prob;
                acc.0 += a.prob * a.base_min;
                acc.1 += a.prob * a.wait_min;
                acc.2 += a.prob * a.exit_walk_min;
            }
        }
        let access = (pm > 0.0).then(|| (acc.0 / pm, acc.1 / pm, acc.2 / pm));
        ds.push(DecOut {
            id: id.clone(),
            tick: -1,
            body_id: None,
            persona_id: Some(prof.persona_id.clone()),
            context: Context::Visit.name().into(),
            trigger: "DAY_START".into(),
            n_alts: d.alts.len() as i32,
            chosen: -1,
            forced: d.reaction.forced,
            rng_key: Some(canonical_key(w.rng.seed, "VISIT", &prof.persona_id, 0, 0, 0)),
            top: d.reaction.top3.clone(),
            access,
        });
        for (k, a) in d.alts.iter().enumerate() {
            let t = &d.reaction.terms[k];
            let (action, target, access) = match &a.kind {
                VisitKind::Myeongdong { slot_start_min, mode } => {
                    let action = if (*slot_start_min - first_slot).abs() < 1e-9 { "VISIT_NOW" } else { "VISIT_LATER" };
                    (action.to_string(), Some(format!("{}@{}", mode.as_str(), fmt_hhmm(*slot_start_min))), Some((a.base_min, a.wait_min, a.exit_walk_min)))
                }
                VisitKind::Competitor { id, mode } => ("GO_ELSEWHERE".to_string(), Some(format!("{id}:{}", mode.as_str())), Some((a.base_min, a.wait_min, a.exit_walk_min))),
                VisitKind::Skip => ("SKIP".to_string(), None, None),
            };
            als.push(AltOut {
                decision: id.clone(),
                alt: k as i32,
                action,
                target,
                prob: d.reaction.probs[k],
                utility: d.reaction.utils[k],
                purpose: t.purpose,
                interest: t.interest_total(),
                cost: t.cost_total(),
                time: t.time,
                money: t.money,
                cost_dims: t.cost,
                access,
            });
        }
    }
    (ds, als)
}

fn runtime_rows(w: &World, state: &SimState, ids: &IdMaps, decisions: &[DecisionRow], alts: &[AltRow]) -> (Vec<DecOut>, Vec<AltOut>) {
    let ds = decisions
        .iter()
        .map(|d| DecOut {
            id: format!("D:{}", d.id),
            tick: d.tick as i64,
            body_id: ids.body(d.body),
            persona_id: persona_of(w, state, d.body),
            context: d.context.clone(),
            trigger: d.trigger.clone(),
            n_alts: d.n_alts as i32,
            chosen: d.chosen as i32,
            forced: d.forced,
            rng_key: d.rng_key.clone(),
            top: d.top.clone(),
            access: None,
        })
        .collect();
    let als = alts
        .iter()
        .map(|a| AltOut {
            decision: format!("D:{}", a.decision),
            alt: a.alt as i32,
            action: a.action.clone(),
            target: if a.target.is_empty() { None } else { Some(a.target.clone()) },
            prob: a.prob,
            utility: a.utility,
            purpose: a.purpose,
            interest: a.interest,
            cost: a.cost,
            time: a.time,
            money: a.money,
            cost_dims: a.cost_dims,
            access: None,
        })
        .collect();
    (ds, als)
}

fn decisions_table(ds: &[DecOut]) -> Table {
    let top = |i: usize| -> (Vec<Option<String>>, Vec<Option<f64>>) { (ds.iter().map(|d| d.top.get(i).map(|x| x.0.clone())).collect(), ds.iter().map(|d| d.top.get(i).map(|x| x.1)).collect()) };
    let (t1n, t1v) = top(0);
    let (t2n, t2v) = top(1);
    let (t3n, t3v) = top(2);
    Table::new()
        .col("decision_id", ColData::Str(ds.iter().map(|d| d.id.clone()).collect()))
        .col("tick", ColData::I64(ds.iter().map(|d| d.tick).collect()))
        .col("body_id", ColData::StrOpt(ds.iter().map(|d| d.body_id.clone()).collect()))
        .col("persona_id", ColData::StrOpt(ds.iter().map(|d| d.persona_id.clone()).collect()))
        .col("context", ColData::Str(ds.iter().map(|d| d.context.clone()).collect()))
        .col("trigger", ColData::Str(ds.iter().map(|d| d.trigger.clone()).collect()))
        .col("n_alts", ColData::I32(ds.iter().map(|d| d.n_alts).collect()))
        .col("chosen", ColData::I32(ds.iter().map(|d| d.chosen).collect()))
        .col("forced", ColData::Bool(ds.iter().map(|d| d.forced).collect()))
        .col("rng_key", ColData::StrOpt(ds.iter().map(|d| d.rng_key.clone()).collect()))
        .col("top1_term", ColData::StrOpt(t1n))
        .col("top1", ColData::F64Opt(t1v))
        .col("top2_term", ColData::StrOpt(t2n))
        .col("top2", ColData::F64Opt(t2v))
        .col("top3_term", ColData::StrOpt(t3n))
        .col("top3", ColData::F64Opt(t3v))
        .col("base_min", ColData::F64Opt(ds.iter().map(|d| d.access.map(|a| a.0)).collect()))
        .col("wait_min", ColData::F64Opt(ds.iter().map(|d| d.access.map(|a| a.1)).collect()))
        .col("exit_walk_min", ColData::F64Opt(ds.iter().map(|d| d.access.map(|a| a.2)).collect()))
}

fn alts_table(al: &[AltOut]) -> Table {
    let mut t = Table::new()
        .col("decision_id", ColData::Str(al.iter().map(|a| a.decision.clone()).collect()))
        .col("alt", ColData::I32(al.iter().map(|a| a.alt).collect()))
        .col("action", ColData::Str(al.iter().map(|a| a.action.clone()).collect()))
        .col("target", ColData::StrOpt(al.iter().map(|a| a.target.clone()).collect()))
        .col("prob", ColData::F64(al.iter().map(|a| a.prob).collect()))
        .col("utility", ColData::F64(al.iter().map(|a| a.utility).collect()))
        .col("purpose", ColData::F64(al.iter().map(|a| a.purpose).collect()))
        .col("interest", ColData::F64(al.iter().map(|a| a.interest).collect()))
        .col("cost", ColData::F64(al.iter().map(|a| a.cost).collect()))
        .col("time", ColData::F64(al.iter().map(|a| a.time).collect()))
        .col("money", ColData::F64(al.iter().map(|a| a.money).collect()));
    for d in Dim::ALL {
        t = t.col(&format!("cost_{}", d.key()), ColData::F64(al.iter().map(|a| a.cost_dims[d.ix()]).collect()));
    }
    t.col("base_min", ColData::F64Opt(al.iter().map(|a| a.access.map(|x| x.0)).collect()))
        .col("wait_min", ColData::F64Opt(al.iter().map(|a| a.access.map(|x| x.1)).collect()))
        .col("exit_walk_min", ColData::F64Opt(al.iter().map(|a| a.access.map(|x| x.2)).collect()))
}

/// 실행 기록에 넘기는 값(표는 [`TableStreams`]가 따로 쓴다).
pub struct RunInfo {
    pub run_id: String,
    pub rep: u32,
    pub representation_ratio: f64,
    pub n_visit: usize,
    pub frames: BTreeMap<String, String>,
}

const META: [(&str, &str); 2] = [("schema_version", "1"), ("producer", ENGINE_NAME)];


/// 큰 표(events·decisions·decision_alts·agents·metrics_10min·store_months)를 나눠 쓰는 작성기 묶음.
/// VISIT 결정은 인물 묶음마다, 실행 중 결정·사건은 10분 조각마다 행 그룹 하나로 쓴다(메모리 한도, 02d §1.9).
/// 월 고리에서는 하루 표시(`tag` = 달, 하루 유형)를 열로 더하고 결정 ID 앞에 붙인다(S-E39).
pub struct TableStreams {
    events: StreamWriter,
    decisions: StreamWriter,
    alts: StreamWriter,
    agents: StreamWriter,
    metrics: StreamWriter,
    store_months: Option<StreamWriter>,
    dir: std::path::PathBuf,
    tag: Option<(u32, String)>,
    pub n_events: u64,
    pub n_runtime_decisions: u64,
    pub n_store_decisions: u64,
}

impl TableStreams {
    pub fn new(dir: &Path) -> Result<TableStreams> {
        std::fs::create_dir_all(dir)?;
        Ok(TableStreams {
            events: StreamWriter::new(&dir.join("events.parquet"), &META),
            decisions: StreamWriter::new(&dir.join("decisions.parquet"), &META),
            alts: StreamWriter::new(&dir.join("decision_alts.parquet"), &META),
            agents: StreamWriter::new(&dir.join("agents.parquet"), &META),
            metrics: StreamWriter::new(&dir.join("metrics_10min.parquet"), &META),
            store_months: None,
            dir: dir.to_path_buf(),
            tag: None,
            n_events: 0,
            n_runtime_decisions: 0,
            n_store_decisions: 0,
        })
    }

    /// 월 고리의 하루 표시. None이면 하루 실행(열·앞붙임 없음).
    pub fn set_tag(&mut self, tag: Option<(u32, String)>) {
        self.tag = tag;
    }

    fn tagged(&self, t: Table) -> Table {
        match &self.tag {
            Some((m, d)) => {
                let n = t.len();
                t.col("month", ColData::I32(vec![*m as i32; n])).col("day_type", ColData::Str(vec![d.clone(); n]))
            }
            None => t,
        }
    }

    fn prefix_ids(&self, ds: &mut [DecOut], als: &mut [AltOut]) {
        if let Some((m, d)) = &self.tag {
            let pre = format!("m{m}:{d}:");
            for x in ds.iter_mut() {
                x.id = format!("{pre}{}", x.id);
            }
            for x in als.iter_mut() {
                x.decision = format!("{pre}{}", x.decision);
            }
        }
    }

    /// VISIT 결정 한 묶음(항별 기여가 남아 있어야 한다).
    pub fn visit_chunk(&mut self, w: &World, decs: &[VisitDecision]) -> Result<()> {
        let (mut ds, mut als) = visit_rows(w, decs);
        self.prefix_ids(&mut ds, &mut als);
        self.decisions.write(&self.tagged(decisions_table(&ds)))?;
        self.alts.write(&self.tagged(alts_table(&als)))
    }

    /// 실행 중 기록 조각 하나.
    pub fn runtime_part(&mut self, w: &World, state: &SimState, ids: &IdMaps, part: &RowsPart) -> Result<()> {
        self.events.write(&self.tagged(events_table(&part.events, ids)))?;
        let (mut ds, mut als) = runtime_rows(w, state, ids, &part.decisions, &part.alts);
        self.prefix_ids(&mut ds, &mut als);
        self.decisions.write(&self.tagged(decisions_table(&ds)))?;
        self.alts.write(&self.tagged(alts_table(&als)))?;
        self.n_events += part.events.len() as u64;
        self.n_runtime_decisions += part.decisions.len() as u64;
        Ok(())
    }

    /// 하루 실행이 끝났을 때 몸 표와 10분 지표.
    pub fn day_end(&mut self, w: &World, state: &SimState) -> Result<()> {
        self.agents.write(&self.tagged(agents_table(w, state)))?;
        self.metrics.write(&self.tagged(state.acc.metrics_table(w.space)))
    }

    /// 상점 결정(context STORE, decision_id `S:<달>:<place_id>`)과 store_months 행(달 끝마다).
    pub fn store_month(&mut self, month: u32, decisions: &[crate::shops::StoreDecision], rows: &[StoreMonthRow]) -> Result<()> {
        let tag = self.tag.take();
        self.tag = Some((month, "month".into()));
        let mut ds = Vec::new();
        let mut als = Vec::new();
        for d in decisions {
            let id = format!("S:{month}:{}", d.place_id);
            let chosen = &d.alts[d.chosen];
            let mut top: Vec<(String, f64)> = vec![("profit".into(), chosen.profit), ("cost:D-COST".into(), -chosen.phi_cost), ("cost:D-DEMAND".into(), -chosen.phi_demand), ("action".into(), -chosen.action_cost)];
            top.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()).then(a.0.cmp(&b.0)));
            top.truncate(3);
            ds.push(DecOut { id: id.clone(), tick: -1, body_id: None, persona_id: None, context: "STORE".into(), trigger: "MONTH_END".into(), n_alts: d.alts.len() as i32, chosen: d.chosen as i32, forced: false, rng_key: Some(d.rng_key.clone()), top, access: None });
            for (k, a) in d.alts.iter().enumerate() {
                als.push(AltOut { decision: id.clone(), alt: k as i32, action: a.action.clone(), target: Some(d.place_id.clone()), prob: a.prob, utility: a.utility, purpose: a.profit, interest: 0.0, cost: a.phi_cost + a.phi_demand + a.action_cost, time: 0.0, money: 0.0, cost_dims: [0.0; ND], access: None });
            }
        }
        self.decisions.write(&self.tagged(decisions_table(&ds)))?;
        self.alts.write(&self.tagged(alts_table(&als)))?;
        self.n_store_decisions += ds.len() as u64;
        self.tag = tag;
        if self.store_months.is_none() {
            self.store_months = Some(StreamWriter::new(&self.dir.join("store_months.parquet"), &META));
        }
        self.store_months.as_mut().unwrap().write(&store_months_table(rows))
    }

    fn finish(self, ids: &IdMaps, files: &mut BTreeMap<String, FileRec>) -> Result<()> {
        let empties = [
            ("events", self.tagged(events_table(&[], ids))),
            ("decisions", self.tagged(decisions_table(&[]))),
            ("decision_alts", self.tagged(alts_table(&[]))),
        ];
        let writers = [self.events, self.decisions, self.alts];
        for ((name, empty), wr) in empties.into_iter().zip(writers) {
            let (sha256, rows) = wr.finish(&empty)?;
            files.insert(name.to_string(), FileRec { path: format!("{name}.parquet"), sha256, rows });
        }
        for (name, wr) in [("agents", self.agents), ("metrics_10min", self.metrics)] {
            let (sha256, rows) = wr.finish(&Table::new().col("body_id", ColData::Str(vec![])))?;
            files.insert(name.to_string(), FileRec { path: format!("{name}.parquet"), sha256, rows });
        }
        if let Some(wr) = self.store_months {
            let (sha256, rows) = wr.finish(&store_months_table(&[]))?;
            files.insert("store_months".to_string(), FileRec { path: "store_months.parquet".into(), sha256, rows });
        }
        Ok(())
    }
}

/// store_months 한 행(02c §2.6 + Rust 덧붙임 열).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreMonthRow {
    pub month: u32,
    pub place_id: String,
    pub action: String,
    pub revenue_krw: f64,
    pub cost_burden: f64,
    pub demand_gap: f64,
    pub open: bool,
    pub price_level_krw: f64,
    pub open_min: f64,
    pub close_min: f64,
    pub service_rate_per_min: f64,
    pub category: String,
    pub customers_month: f64,
    pub staff_count: f64,
    pub revenue_day: [f64; 3],
    pub days: [u32; 3],
    pub prob: f64,
    pub assumed: bool,
}

pub fn store_months_table(r: &[StoreMonthRow]) -> Table {
    let dts = ["weekday", "weekend", "event_day"];
    let mut t = Table::new()
        .col("month", ColData::I32(r.iter().map(|x| x.month as i32).collect()))
        .col("place_id", ColData::Str(r.iter().map(|x| x.place_id.clone()).collect()))
        .col("action", ColData::Str(r.iter().map(|x| x.action.clone()).collect()))
        .col("revenue_krw", ColData::F64(r.iter().map(|x| x.revenue_krw).collect()))
        .col("cost_burden", ColData::F64(r.iter().map(|x| x.cost_burden).collect()))
        .col("demand_gap", ColData::F64(r.iter().map(|x| x.demand_gap).collect()))
        .col("open", ColData::Bool(r.iter().map(|x| x.open).collect()))
        .col("price_level_krw", ColData::F64(r.iter().map(|x| x.price_level_krw).collect()))
        .col("open_min", ColData::I32(r.iter().map(|x| x.open_min.round() as i32).collect()))
        .col("close_min", ColData::I32(r.iter().map(|x| x.close_min.round() as i32).collect()))
        .col("service_rate_per_min", ColData::F64(r.iter().map(|x| x.service_rate_per_min).collect()))
        .col("category", ColData::Str(r.iter().map(|x| x.category.clone()).collect()))
        .col("customers_month", ColData::F64(r.iter().map(|x| x.customers_month).collect()))
        .col("staff_count", ColData::F64(r.iter().map(|x| x.staff_count).collect()));
    for (k, dt) in dts.iter().enumerate() {
        t = t.col(&format!("revenue_day_{dt}"), ColData::F64(r.iter().map(|x| x.revenue_day[k]).collect()));
    }
    for (k, dt) in dts.iter().enumerate() {
        t = t.col(&format!("days_{dt}"), ColData::I32(r.iter().map(|x| x.days[k] as i32).collect()));
    }
    t.col("prob", ColData::F64(r.iter().map(|x| x.prob).collect())).col("assumed", ColData::Bool(r.iter().map(|x| x.assumed).collect()))
}

pub fn base_manifest(w: &World, rep: u32, run_id: &str, representation_ratio: f64) -> RunManifest {
    let (a, b) = w.scn.window_min().unwrap_or((0.0, 0.0));
    RunManifest {
        schema_version: RUN_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        bundle_id: w.bundle.manifest.bundle_id.clone(),
        ontology_sha256: w.defs.sha256.clone(),
        dimensions_version: w.defs.dims_version.clone(),
        engine: EngineRec { name: ENGINE_NAME.into(), version: env!("CARGO_PKG_VERSION").into() },
        scenario_id: w.scn.id.clone(),
        scenario_sha256: w.scenario_sha256.clone(),
        seed: w.scn.seed,
        rep,
        day_type: w.scn.day_type.clone(),
        window: WindowRec { start: w.scn.window.start.clone(), end: w.scn.window.end.clone(), start_min: a, end_min: b },
        dt_s: w.scn.dt_s,
        render_every_ticks: w.scn.render_every_ticks,
        representation_ratio,
        interactions: w.scn.interactions,
        configuration: w.scn.configuration.clone().unwrap_or_else(|| "static_stores".into()),
        months: w.scn.months.unwrap_or(0),
        status: "failed".into(),
        error: None,
        counts: BTreeMap::new(),
        files: BTreeMap::new(),
        frames: BTreeMap::new(),
        unsupported_actions: unsupported_actions(w),
        pending_inputs: w.bundle.manifest.pending_inputs.clone(),
        assumption_flags: w.bundle.manifest.assumption_flags.clone(),
    }
}

/// 남은 표(agents·metrics_10min)와 manifest(status = complete)를 쓴다. 큰 표는 `streams`가 이미 썼다.
pub fn write_run(dir: &Path, w: &World, state: &SimState, info: &RunInfo, streams: TableStreams) -> Result<RunManifest> {
    std::fs::create_dir_all(dir)?;
    let ids = id_maps(w, state);
    let (n_events, n_dec) = (streams.n_events, streams.n_runtime_decisions);
    let mut files = BTreeMap::new();
    let mut streams = streams;
    streams.day_end(w, state)?;
    streams.finish(&ids, &mut files)?;
    let mut m = base_manifest(w, info.rep, &info.run_id, info.representation_ratio);
    m.status = "complete".into();
    m.files = files;
    m.frames = info.frames.clone();
    let alive_end = state.bodies.iter().filter(|b| b.alive).count();
    for (k, v) in [
        ("bodies", state.bodies.len() as f64),
        ("bodies_domestic_or_tourist", state.bodies.iter().filter(|b| b.profile.is_some()).count() as f64),
        ("bodies_background", state.bodies.iter().filter(|b| b.profile.is_none()).count() as f64),
        ("bodies_alive_at_end", alive_end as f64),
        ("events", n_events as f64),
        ("decisions_runtime", n_dec as f64),
        ("decisions_visit", info.n_visit as f64),
        ("ticks", state.tick as f64),
        ("excluded_places", w.space.excluded_places.len() as f64),
    ] {
        m.counts.insert(k.to_string(), v);
    }
    write_manifest(dir, &m)?;
    Ok(m)
}

/// 월 고리 실행의 표를 닫고 manifest(status = complete)를 쓴다.
pub fn write_monthly(dir: &Path, w: &World, info: &RunInfo, streams: TableStreams, counts: BTreeMap<String, f64>, finance_assumed: bool) -> Result<RunManifest> {
    let (n_events, n_dec, n_store) = (streams.n_events, streams.n_runtime_decisions, streams.n_store_decisions);
    let mut files = BTreeMap::new();
    streams.finish(&IdMaps::default(), &mut files)?;
    let mut m = base_manifest(w, info.rep, &info.run_id, info.representation_ratio);
    m.status = "complete".into();
    m.files = files;
    m.counts = counts;
    m.counts.insert("events".into(), n_events as f64);
    m.counts.insert("decisions_runtime".into(), n_dec as f64);
    m.counts.insert("decisions_store".into(), n_store as f64);
    if finance_assumed && !m.assumption_flags.iter().any(|f| f == "store_finance_assumed") {
        m.assumption_flags.push("store_finance_assumed".into());
    }
    m.unsupported_actions.remove("STORE");
    write_manifest(dir, &m)?;
    Ok(m)
}

pub fn write_manifest(dir: &Path, m: &RunManifest) -> Result<()> {
    let text = serde_json::to_string_pretty(m).map_err(|e| SimError::Io(e.to_string()))?;
    std::fs::write(dir.join("manifest.json"), text + "\n")?;
    Ok(())
}

/// 실패한 실행의 manifest(status = failed, 단계와 메시지, C-228 ①).
pub fn write_failed(dir: &Path, w: &World, rep: u32, run_id: &str, err: &SimError) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut m = base_manifest(w, rep, run_id, 0.0);
    m.error = Some(err.to_string());
    write_manifest(dir, &m)
}

/// manifest의 파일 해시를 다시 계산해 대조한다(E226-5). 다른 파일 이름 목록을 돌려준다.
pub fn verify_run(dir: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(dir.join("manifest.json"))?;
    let m: RunManifest = serde_json::from_str(&text).map_err(|e| SimError::Io(e.to_string()))?;
    let mut bad = Vec::new();
    for (name, f) in &m.files {
        if mdsim_core::pq::file_sha256(&dir.join(&f.path))? != f.sha256 {
            bad.push(name.clone());
        }
    }
    for (path, sha) in &m.frames {
        if mdsim_core::pq::file_sha256(&dir.join(path))? != *sha {
            bad.push(path.clone());
        }
    }
    Ok(bad)
}
