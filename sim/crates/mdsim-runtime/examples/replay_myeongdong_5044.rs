//! Direct, auditable arrivals on the prepared Myeongdong input. No visit-demand draw.
use mdsim_core::bundle::{Bundle, Mode, PortalKind};
use mdsim_core::defs::Defs;
use mdsim_core::pq::file_sha256;
use mdsim_core::scenario::Scenario;
use mdsim_runtime::output::{id_maps, write_run, RunInfo, TableStreams};
use mdsim_runtime::record::FileSink;
use mdsim_runtime::space::Space;
use mdsim_runtime::step::{Sim, World};
use mdsim_runtime::visit::{sort_spawns, stable_ix, Spawn, SpawnKind};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;

fn arrival_tick(index: usize, count: usize, n_ticks: u64) -> u64 {
    (index as u64 * n_ticks) / count as u64
}

fn write_status(dir: &Path, status: &Value) -> Result<(), Box<dyn Error>> {
    let tmp = dir.join("run_status.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(status)?)?;
    std::fs::rename(tmp, dir.join("run_status.json"))?;
    Ok(())
}

fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() != 6 && args.len() != 7 {
        return Err("usage: replay_myeongdong_5044 ROOT DEFS BUNDLE SCENARIO OUTPUT [EXPECTED_COUNT]".into());
    }
    let root = Path::new(&args[1]);
    let defs_path = Path::new(&args[2]);
    let bundle_path = Path::new(&args[3]);
    let scenario_path = Path::new(&args[4]);
    let out = Path::new(&args[5]);
    let expected: usize = if args.len() == 7 { args[6].parse()? } else { 5044 };
    if expected == 0 { return Err("expected count must be positive".into()); }
    if out.exists() && std::fs::read_dir(out)?.next().is_some() {
        return Err("output directory must be empty".into());
    }
    std::fs::create_dir_all(out)?;
    let source: Value = serde_json::from_slice(&std::fs::read(root.join("source.json"))?)?;
    let defs = Defs::load(defs_path)?;
    let bundle = Bundle::load(bundle_path, root, &defs)?;
    let scn = Scenario::load(scenario_path)?;
    let entry = bundle.scenario_entry(scenario_path)?;
    if entry.seed != scn.seed || entry.day_type != scn.day_type {
        return Err("scenario seed or day type does not match the bundle manifest".into());
    }
    let space = Space::build(&defs, &bundle)?;
    if let Some(overrides) = source["run_options"]["building_overrides"].as_object() {
        for id in overrides.keys() {
            if space.place_ix(&format!("S:{id}")).is_none() {
                return Err(format!("지정한 건물 {id}의 장소가 보행 가능 공간에 연결되지 않았습니다").into());
            }
        }
    }
    if bundle.profiles.len() != expected || space.portals.is_empty() {
        return Err(format!("expected {expected} profiles and at least one portal; got {} profiles and {} portals", bundle.profiles.len(), space.portals.len()).into());
    }
    let unique: HashSet<_> = bundle.profiles.iter().map(|p| &p.persona_id).collect();
    if unique.len() != expected { return Err("profile IDs are not unique".into()); }
    let ticks = scn.n_ticks();
    if ticks == 0 { return Err("scenario contains no ticks".into()); }
    let spawns = sort_spawns(bundle.profiles.iter().enumerate().map(|(i, p)| {
        let tick = arrival_tick(i, expected, ticks);
        let portal = stable_ix(&p.persona_id, space.portals.len());
        let mode = match space.portals[portal].kind {
            PortalKind::Subway => Mode::Subway,
            PortalKind::Bus | PortalKind::TourBus => Mode::Bus,
            _ => Mode::Walk,
        };
        Spawn {
            tick,
            kind: SpawnKind::Domestic { profile: i as u32, replicate: 0 },
            portal: portal as u32,
            mode,
            deadline_tick: (tick + (65.0 * 60.0 / scn.dt_s).round() as u64).min(ticks),
            entity: format!("{}#0", p.persona_id),
        }
    }).collect());
    let world = World::new(&defs, &bundle, &space, &scn, file_sha256(scenario_path)?)?;
    let mut status = json!({
        "schema_version": 1, "status": "running", "window_start_s": scn.window_s().0,
        "window_end_s": scn.window_s().1, "dt_s": scn.dt_s,
        "render_every_ticks": scn.render_every_ticks, "planned_bodies": expected,
        "spawned_bodies": 0, "completed_tick": 0, "frames": [],
        "alignment": source["alignment"], "limitations": source["limitations"],
        "provenance": {"input": "synthetic_personas", "arrival": "forced", "source_sha256": source["source_sha256"], "run_options": source["run_options"], "engine_build": source["engine_build"]}
    });
    write_status(out, &status)?;
    let result: Result<(), Box<dyn Error>> = (|| {
        let mut sink = FileSink::new(out, scn.window_s().0, scn.dt_s)?;
        sink.write_checkpoints = false;
        let mut sim = Sim::new(&world, &spawns);
        let chunk_ticks = scn.ticks_per_chunk();
        while sim.state.tick < ticks {
            let start = sim.state.tick;
            let end = (start + chunk_ticks).min(ticks);
            sim.run_until(end, &mut sink)?;
            let path = format!("frames/frame_{}.bin", sink.hhmm(start));
            let sha = sink.chunk_files.get(&path).ok_or("engine did not complete expected frame chunk")?;
            status["frames"].as_array_mut().ok_or("invalid status frames")?.push(json!({
                "path": path, "sha256": sha, "tick_start": start, "tick_end": end
            }));
            status["completed_tick"] = json!(sim.state.tick);
            status["spawned_bodies"] = json!(sim.state.bodies.len());
            write_status(out, &status)?;
            eprintln!("tick {}/{}; admitted {}/{}; chunks {}", sim.state.tick, ticks, sim.state.bodies.len(), expected, sink.chunk_files.len());
        }
        if sim.state.bodies.len() != expected || sim.state.bodies.iter().map(|b| &b.entity).collect::<HashSet<_>>().len() != expected {
            return Err("engine did not admit each profile exactly once".into());
        }
        let ids = id_maps(&world, &sim.state);
        let mut streams = TableStreams::new(out)?;
        FileSink::for_each_part(out, &mut |part| streams.runtime_part(&world, &sim.state, &ids, &part))?;
        let info = RunInfo {
            run_id: out.file_name().and_then(|s| s.to_str()).unwrap_or("myeongdong_direct").to_string(),
            rep: 0, representation_ratio: 1.0, n_visit: 0, frames: sink.chunk_files.clone(),
        };
        write_run(out, &world, &sim.state, &info, streams)?;
        status["status"] = json!("complete");
        write_status(out, &status)?;
        Ok(())
    })();
    if let Err(e) = &result {
        status["status"] = json!("failed");
        status["error"] = json!(e.to_string());
        let _ = write_status(out, &status);
    }
    result
}

fn main() {
    if let Err(e) = run(&std::env::args().collect::<Vec<_>>()) {
        eprintln!("direct replay failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::arrival_tick;
    #[test]
    fn direct_arrivals_are_complete_and_within_window() {
        for (count, ticks) in [(1, 21600), (12, 21600), (5044, 21600), (5044, 2)] {
            let arrivals: Vec<_> = (0..count).map(|i| arrival_tick(i, count, ticks)).collect();
            assert_eq!(arrivals.len(), count);
            assert_eq!(arrivals[0], 0);
            assert!(arrivals.iter().all(|&t| t < ticks));
            assert!(arrivals.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}
