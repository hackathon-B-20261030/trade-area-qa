//! `mdsim` 명령 (02d I-228-b, C-228). 정의·번들·시나리오를 읽어 시뮬레이션을 돌리고 실행 기록과 그림을 쓴다.
//!
//! 명령:
//!   mdsim fixture  --out <폴더>
//!   mdsim validate --defs <폴더> --bundle <폴더> [--root <폴더>] [--scenario <파일>]
//!   mdsim run      --defs <폴더> --bundle <폴더> --scenario <파일> --out <폴더> [--root <폴더>]
//!                  [--run-id <ID>] [--resume <체크포인트>] [--no-checkpoints] [--no-plots] [--threads N]
//!   mdsim probe    --defs <폴더> --in <상황.jsonl> --out <결과.jsonl> [--bundle <폴더> --root <폴더>]
//!   mdsim plot     --run <실행 폴더> --defs <폴더> --bundle <폴더> --scenario <파일> [--root <폴더>]
//!   mdsim verify   --run <실행 폴더>
//!
//! 오류는 표준 오류에 JSON 한 줄({code, stage, message})로 쓰고 02d §1.7의 종료 코드로 끝난다.

mod report;

use mdsim_core::bundle::Bundle;
use mdsim_core::defs::{hex, Defs};
use mdsim_core::fixture::write_fixture;
use mdsim_core::pq::file_sha256;
use mdsim_core::scenario::Scenario;
use mdsim_core::{Result, SimError};
use mdsim_runtime::output::{id_maps, write_failed, write_monthly, write_run, RunInfo, TableStreams};
use mdsim_runtime::record::{restore_state, FileSink};
use mdsim_runtime::space::Space;
use mdsim_runtime::step::{Sim, World};
use mdsim_runtime::visit::{plan_arrivals, plan_background, representation_ratio, sort_spawns, visit_decisions_streamed};

/// VISIT 결정을 이만큼씩 나눠 계산하고 기록한다(메모리 한도).
const VISIT_CHUNK: usize = 4096;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Args {
    cmd: String,
    opts: BTreeMap<String, String>,
    flags: Vec<String>,
}

fn parse_args() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or_else(|| SimError::Run(usage()))?;
    let mut opts = BTreeMap::new();
    let mut flags = Vec::new();
    let rest: Vec<String> = it.collect();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        let Some(key) = a.strip_prefix("--") else { return Err(SimError::Run(format!("모르는 인자 '{a}'\n{}", usage()))) };
        if matches!(key, "no-checkpoints" | "no-plots") {
            flags.push(key.to_string());
            i += 1;
            continue;
        }
        let v = rest.get(i + 1).ok_or_else(|| SimError::Run(format!("--{key}에 값이 없음")))?;
        opts.insert(key.to_string(), v.clone());
        i += 2;
    }
    Ok(Args { cmd, opts, flags })
}

fn usage() -> String {
    "사용법: mdsim <fixture|validate|run|probe|plot|verify> [옵션] (머리 주석 참고)".to_string()
}

impl Args {
    fn path(&self, k: &str) -> Result<PathBuf> {
        self.opts.get(k).map(PathBuf::from).ok_or_else(|| SimError::Run(format!("--{k}가 필요하다")))
    }
    fn root(&self) -> PathBuf {
        self.opts.get("root").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
    }
    fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }
}

/// 단계 이름을 붙여 오류를 돌려준다(C-228 ①).
fn at<T>(stage: &'static str, r: Result<T>) -> std::result::Result<T, (&'static str, SimError)> {
    r.map_err(|e| (stage, e))
}

type Staged<T> = std::result::Result<T, (&'static str, SimError)>;

fn main() {
    let res: Staged<()> = (|| {
        let a = at("args", parse_args())?;
        // 틱마다 여러 번 나눠 합치므로 스레드가 많으면 깨우기 비용이 이득을 넘는다: 기본은 min(코어, 8)(S-E29).
        let n = match a.opts.get("threads") {
            Some(n) => n.parse().map_err(|_| ("args", SimError::Run("--threads는 정수".into())))?,
            None => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8),
        };
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
        match a.cmd.as_str() {
            "fixture" => cmd_fixture(&a),
            "validate" => cmd_validate(&a),
            "run" => cmd_run(&a),
            "probe" => cmd_probe(&a),
            "plot" => cmd_plot(&a),
            "verify" => cmd_verify(&a),
            other => Err(("args", SimError::Run(format!("모르는 명령 '{other}'\n{}", usage())))),
        }
    })();
    if let Err((stage, e)) = res {
        let line = serde_json::json!({"code": e.exit_code(), "stage": stage, "message": e.to_string()});
        eprintln!("{line}");
        std::process::exit(e.exit_code());
    }
}

fn cmd_fixture(a: &Args) -> Staged<()> {
    let out = at("args", a.path("out"))?;
    let p = at("fixture", write_fixture(&out))?;
    let rel = |x: &Path| x.strip_prefix(&p.root).unwrap_or(x).display().to_string();
    let j = serde_json::json!({
        "root": p.root.display().to_string(),
        "defs": rel(&p.defs),
        "bundle": rel(&p.bundle), "scenario": rel(&p.scenario), "scenario_r1": rel(&p.scenario_r1),
        "bundle_quick": rel(&p.bundle_quick), "scenario_quick": rel(&p.scenario_quick),
        "bundle_variant": rel(&p.bundle_variant), "scenario_variant": rel(&p.scenario_variant),
        "bundle_monthly": rel(&p.bundle_monthly), "scenario_monthly": rel(&p.scenario_monthly),
    });
    println!("{}", serde_json::to_string_pretty(&j).unwrap());
    Ok(())
}

struct Loaded {
    defs: Defs,
    bundle: Bundle,
}

fn load(a: &Args) -> Staged<Loaded> {
    let defs = at("load_defs", Defs::load(&at("args", a.path("defs"))?))?;
    let bundle = at("load_bundle", Bundle::load(&at("args", a.path("bundle"))?, &a.root(), &defs))?;
    Ok(Loaded { defs, bundle })
}

/// 시나리오를 읽고 번들 manifest의 항목(해시)과 시드·하루 유형을 대조한다.
fn load_scenario(bundle: &Bundle, path: &Path) -> Staged<(Scenario, u32, String)> {
    let scn = at("load_scenario", Scenario::load(path))?;
    let entry = at("load_scenario", bundle.scenario_entry(path))?;
    if entry.seed != scn.seed || entry.day_type != scn.day_type {
        return Err(("load_scenario", SimError::contract("scenario", format!("시나리오의 seed·day_type({}, {})이 manifest 항목({}, {})과 다르다", scn.seed, scn.day_type, entry.seed, entry.day_type))));
    }
    let sha = at("load_scenario", file_sha256(path))?;
    Ok((scn, entry.rep, sha))
}

fn cmd_validate(a: &Args) -> Staged<()> {
    let l = load(a)?;
    let space = at("build_space", Space::build(&l.defs, &l.bundle))?;
    let mut j = serde_json::json!({
        "ontology_sha256": l.defs.sha256,
        "dimensions_version": l.defs.dims_version,
        "bundle_id": l.bundle.manifest.bundle_id,
        "nodes": l.bundle.nodes.len(), "edges": l.bundle.edges.len(), "buildings": l.bundle.buildings.len(),
        "places": l.bundle.places.len(), "places_excluded": space.excluded_places.len(),
        "portals": l.bundle.portals.len(), "profiles": l.bundle.profiles.len(),
        "competitors": l.bundle.competitors.len(), "background_rows": l.bundle.background.len(),
        "scenarios": l.bundle.manifest.scenarios.len(),
        "pending_inputs": l.bundle.manifest.pending_inputs,
    });
    if let Some(sp) = a.opts.get("scenario") {
        let (scn, rep, sha) = load_scenario(&l.bundle, Path::new(sp))?;
        j["scenario"] = serde_json::json!({"id": scn.id, "day_type": scn.day_type, "rep": rep, "seed": scn.seed, "sha256": sha, "ticks": scn.n_ticks()});
    }
    println!("{}", serde_json::to_string_pretty(&j).unwrap());
    Ok(())
}

fn default_run_id(bundle_id: &str, scenario_sha: &str) -> String {
    hex(&Sha256::digest(format!("{bundle_id}:{scenario_sha}:{}", env!("CARGO_PKG_VERSION")).as_bytes()))[..16].to_string()
}

fn cmd_run(a: &Args) -> Staged<()> {
    let t0 = Instant::now();
    let out = at("args", a.path("out"))?;
    let resume = a.opts.get("resume").map(PathBuf::from);
    if resume.is_none() && out.exists() && std::fs::read_dir(&out).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(("args", SimError::Run(format!("산출 폴더 {}가 이미 있다(덮어쓰지 않는다)", out.display()))));
    }
    let l = load(a)?;
    let scen_path = at("args", a.path("scenario"))?;
    let (scn, rep, scen_sha) = load_scenario(&l.bundle, &scen_path)?;
    let space = at("build_space", Space::build(&l.defs, &l.bundle))?;
    let w = at("world", World::new(&l.defs, &l.bundle, &space, &scn, scen_sha.clone()))?;
    let run_id = a.opts.get("run-id").cloned().unwrap_or_else(|| default_run_id(&l.bundle.manifest.bundle_id, &scen_sha));
    if scn.months.unwrap_or(0) >= 1 {
        if resume.is_some() {
            return Err(("args", SimError::Run("월 고리(months ≥ 1)는 체크포인트 이어 달리기를 지원하지 않는다(S-E40)".into())));
        }
        let result: Staged<serde_json::Value> = (|| {
            let mut streams = at("write_records", TableStreams::new(&out))?;
            let oc = at("monthly", mdsim_runtime::monthly::run_monthly(&l.defs, &l.bundle, &space, &scn, &scen_sha, &out, &mut streams, VISIT_CHUNK))?;
            let closed = oc.stores.iter().filter(|s| s.closed).count();
            let mut counts = BTreeMap::new();
            for (k, v) in [
                ("months", scn.months.unwrap_or(0) as f64),
                ("stores", oc.stores.len() as f64),
                ("stores_closed", closed as f64),
                ("bodies_all_days", oc.bodies),
                ("days_weekday", oc.days[0] as f64),
                ("days_weekend", oc.days[1] as f64),
                ("days_event_day", oc.days[2] as f64),
            ] {
                counts.insert(k.to_string(), v);
            }
            let finance_assumed = oc.stores.iter().any(|s| s.finance_assumed);
            let info = RunInfo { run_id: run_id.clone(), rep, representation_ratio: oc.ratio, n_visit: l.bundle.profiles.len(), frames: BTreeMap::new() };
            let m = at("write_records", write_monthly(&out, &w, &info, streams, counts, finance_assumed))?;
            let plots = match (&oc.last, a.flag("no-plots")) {
                (Some(last), false) => {
                    let w2 = at("plots", World::new(&l.defs, &l.bundle, &last.space, &last.scn, scen_sha.clone()))?;
                    serde_json::json!(at("plots", report::write_report_monthly(&out, &w2, last, &oc.rows))?)
                }
                _ => serde_json::Value::Null,
            };
            Ok(serde_json::json!({
                "run_id": m.run_id, "status": m.status, "out": out.display().to_string(), "months": scn.months,
                "configuration": m.configuration, "stores": oc.stores.len(), "stores_closed": closed,
                "store_decisions": m.counts.get("decisions_store"), "total_s": t0.elapsed().as_secs_f64(), "plots": plots,
            }))
        })();
        return match result {
            Ok(j) => {
                println!("{}", serde_json::to_string_pretty(&j).unwrap());
                Ok(())
            }
            Err((stage, e)) => {
                let _ = write_failed(&out, &w, rep, &run_id, &e.clone().at_stage(stage));
                Err((stage, e))
            }
        };
    }
    let result: Staged<serde_json::Value> = (|| {
        let mut streams = at("write_records", TableStreams::new(&out))?;
        let visit = at("visit", visit_decisions_streamed(&l.defs, &space, &l.bundle, &scn, &w.p, &w.rng, VISIT_CHUNK, &mut |chunk| streams.visit_chunk(&w, chunk)))?;
        let ratio = representation_ratio(&visit, &scn, &w.p);
        let mut spawns = plan_arrivals(&space, &l.bundle, &scn, &w.p, &w.rng, &visit, ratio);
        spawns.extend(plan_background(&space, &l.bundle, &scn, &w.p, &w.rng));
        let spawns = sort_spawns(spawns);
        let mut sink = at("simulate", FileSink::new(&out, scn.window_s().0, scn.dt_s))?;
        sink.write_checkpoints = !a.flag("no-checkpoints");
        let t_sim = Instant::now();
        let mut sim = match &resume {
            Some(ck) => {
                let bytes = at("resume", std::fs::read(ck).map_err(SimError::from))?;
                let st = at("resume", restore_state(&bytes))?;
                if st.scenario_sha256 != scen_sha || st.defs_sha256 != l.defs.sha256 {
                    return Err(("resume", SimError::Run("체크포인트의 시나리오·정의 해시가 이번 실행과 다르다".into())));
                }
                at("resume", sink.prepare_resume(st.tick))?;
                at("resume", Sim::from_state(&w, &spawns, st))?
            }
            None => Sim::new(&w, &spawns),
        };
        at("simulate", sim.run(&mut sink))?;
        let sim_s = t_sim.elapsed().as_secs_f64();
        let ids = id_maps(&w, &sim.state);
        let mut plot_rows = report::PlotRows::default();
        at("write_records", FileSink::for_each_part(&out, &mut |part| {
            streams.runtime_part(&w, &sim.state, &ids, &part)?;
            plot_rows.absorb(part);
            Ok(())
        }))?;
        let info = RunInfo { run_id: run_id.clone(), rep, representation_ratio: ratio, n_visit: visit.len(), frames: sink.chunk_files.clone() };
        let m = at("write_records", write_run(&out, &w, &sim.state, &info, streams))?;
        let rows = plot_rows.into_rows();
        let plots = if a.flag("no-plots") {
            serde_json::Value::Null
        } else {
            let r = at("plots", report::write_report(&out, &w, &sim.state, &visit, &rows))?;
            serde_json::json!(r)
        };
        Ok(serde_json::json!({
            "run_id": m.run_id, "status": m.status, "out": out.display().to_string(),
            "bodies": m.counts.get("bodies"), "events": m.counts.get("events"),
            "decisions_runtime": m.counts.get("decisions_runtime"), "representation_ratio": ratio,
            "ticks": sim.state.tick, "simulate_s": sim_s, "total_s": t0.elapsed().as_secs_f64(),
            "plots": plots,
        }))
    })();
    match result {
        Ok(j) => {
            println!("{}", serde_json::to_string_pretty(&j).unwrap());
            Ok(())
        }
        Err((stage, e)) => {
            let _ = write_failed(&out, &w, rep, &run_id, &e.clone().at_stage(stage));
            Err((stage, e))
        }
    }
}

fn cmd_probe(a: &Args) -> Staged<()> {
    let defs = at("load_defs", Defs::load(&at("args", a.path("defs"))?))?;
    let p = at("load_defs", mdsim_runtime::params::RtParams::load(&defs.params))?;
    let bundle = match a.opts.get("bundle") {
        Some(b) => Some(at("load_bundle", Bundle::load(Path::new(b), &a.root(), &defs))?),
        None => None,
    };
    let input = at("args", a.path("in"))?;
    let text = at("probe", std::fs::read_to_string(&input).map_err(SimError::from))?;
    let mut out = String::new();
    for (ln, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = at("probe", serde_json::from_str(line).map_err(|e| SimError::contract("probe", format!("{}행: {e}", ln + 1))))?;
        let prof = match (v.get("profile"), v.get("persona_id").and_then(|x| x.as_str()), &bundle) {
            (Some(pj), _, _) => at("probe", mdsim_runtime::probe::profile_from_json(pj))?,
            (None, Some(pid), Some(b)) => b.profiles.iter().find(|p| p.persona_id == pid).cloned().ok_or_else(|| ("probe", SimError::contract("probe", format!("{}행: 인물 {pid}가 번들에 없음", ln + 1))))?,
            _ => return Err(("probe", SimError::contract("probe", format!("{}행: profile 또는 (persona_id와 --bundle)이 필요하다", ln + 1)))),
        };
        let r = at("probe", mdsim_runtime::probe::evaluate(&defs, &p, &prof, &v))?;
        out.push_str(&serde_json::to_string(&r).unwrap());
        out.push('\n');
    }
    at("probe", std::fs::write(at("args", a.path("out"))?, out).map_err(SimError::from))?;
    Ok(())
}

fn cmd_plot(a: &Args) -> Staged<()> {
    let run = at("args", a.path("run"))?;
    let l = load(a)?;
    let scen_path = at("args", a.path("scenario"))?;
    let (scn, _rep, scen_sha) = load_scenario(&l.bundle, &scen_path)?;
    let space = at("build_space", Space::build(&l.defs, &l.bundle))?;
    let w = at("world", World::new(&l.defs, &l.bundle, &space, &scn, scen_sha))?;
    let r = at("plots", report::rebuild_report(&run, &w))?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!(r)).unwrap());
    Ok(())
}

fn cmd_verify(a: &Args) -> Staged<()> {
    let run = at("args", a.path("run"))?;
    let bad = at("verify", mdsim_runtime::output::verify_run(&run))?;
    if bad.is_empty() {
        println!("{}", serde_json::json!({"ok": true}));
        Ok(())
    } else {
        Err(("verify", SimError::Invariant(format!("해시가 다른 파일: {bad:?}"))))
    }
}
