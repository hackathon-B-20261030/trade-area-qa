//! 합성 고정 자료로 빠른 시나리오를 끝까지 돌려 요약을 찍는다(개발 확인용).
use mdsim_core::bundle::Bundle;
use mdsim_core::defs::Defs;
use mdsim_core::fixture::write_fixture;
use mdsim_core::scenario::Scenario;
use mdsim_runtime::record::MemSink;
use mdsim_runtime::space::Space;
use mdsim_runtime::step::{Sim, World};
use mdsim_runtime::visit::*;
use std::time::Instant;

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "quick".into());
    let root = std::env::temp_dir().join("mdsim-smoke");
    let _ = std::fs::remove_dir_all(&root);
    let fx = write_fixture(&root).unwrap();
    let defs = Defs::load(&fx.defs).unwrap();
    let bundle = Bundle::load(&fx.bundle, &fx.root, &defs).unwrap();
    let space = Space::build(&defs, &bundle).unwrap();
    let path = match which.as_str() { "quick" => fx.scenario_quick.clone(), "variant" => fx.scenario_variant.clone(), _ => fx.scenario.clone() };
    let scn = Scenario::load(&path).unwrap();
    let w = World::new(&defs, &bundle, &space, &scn, "smoke".into()).unwrap();
    let t0 = Instant::now();
    let dec = visit_decisions(&defs, &space, &bundle, &scn, &w.p, &w.rng);
    let ratio = representation_ratio(&dec, &scn, &w.p);
    let mut spawns = plan_arrivals(&space, &bundle, &scn, &w.p, &w.rng, &dec, ratio);
    let bg = plan_background(&space, &bundle, &scn, &w.p, &w.rng);
    let (nd, nb) = (spawns.len(), bg.len());
    spawns.extend(bg);
    let spawns = sort_spawns(spawns);
    println!("visit: P_sum={:.1} ratio={:.2} domestic={} background={} ({:.2}s)", myeongdong_prob_sum(&dec), ratio, nd, nb, t0.elapsed().as_secs_f64());
    let t1 = Instant::now();
    let mut sim = Sim::new(&w, &spawns);
    let mut sink = MemSink::default();
    sim.run(&mut sink).unwrap();
    let el = t1.elapsed().as_secs_f64();
    let kinds = sink.events.iter().fold(std::collections::BTreeMap::<String, usize>::new(), |mut m, e| { *m.entry(e.kind.clone()).or_default() += 1; m });
    let ctx = sink.decisions.iter().fold(std::collections::BTreeMap::<String, usize>::new(), |mut m, d| { *m.entry(d.context.clone()).or_default() += 1; m });
    let chosen = sink.decisions.iter().filter(|d| d.context != "LOOK").fold(std::collections::BTreeMap::<String, usize>::new(), |mut m, d| {
        if let Some(a) = sink.alts.iter().find(|a| a.decision == d.id && a.alt == d.chosen) {
            *m.entry(format!("{}:{}", d.context, a.action)).or_default() += 1;
        }
        m
    });
    println!("chosen: {chosen:?}");
    println!("run: ticks={} {:.2}s ({:.2} ms/tick)", sim.state.tick, el, el * 1000.0 / sim.state.tick as f64);
    println!("events: {kinds:?}");
    println!("decisions: {ctx:?}");
    let alive = sim.state.bodies.iter().filter(|b| b.alive).count();
    println!("bodies: total={} alive_end={}", sim.state.bodies.len(), alive);
    for f in &sim.state.acc.frames {
        println!("  {:.0}분: arr={} dep={} conc_mean={:.0} conc_max={} speed={:.2} notices={} enters={} purchases={} revenue={:.0}", f.start_min, f.arrivals, f.departures, f.conc_sum / f.samples.max(1) as f64, f.conc_max, f.speed_sum / f.speed_n.max(1) as f64, f.notices, f.enters, f.purchases, f.revenue);
    }
    let dom: Vec<_> = sim.state.bodies.iter().filter(|b| !b.is_background()).collect();
    let g = dom.iter().fold((0.0, 0.0, 0.0), |a, b| (a.0 + b.stats.gaze_path_s, a.1 + b.stats.gaze_footing_s, a.2 + b.stats.gaze_place_s));
    let tot = g.0 + g.1 + g.2;
    println!("gaze share: path={:.2} footing={:.2} place={:.2}", g.0 / tot, g.1 / tot, g.2 / tot);
    let blocked = sim.state.bodies.iter().filter(|b| b.alive && b.blocked_ticks > 20).count();
    println!("blocked>10s at end: {blocked}; chunks={}", sink.chunk_hashes.len());
    for b in sim.state.bodies.iter().filter(|b| b.alive && b.blocked_ticks > 20).take(12) {
        let flag = space.flag_at(b.pos);
        let tgt = b.route.as_ref().map(|r| (r.idx, r.edges.len(), r.final_pos));
        let ce = b.cur_edge.map(|e| (space.edges[e as usize].id.clone(), space.edges[e as usize].sidewalks));
        println!("  body {} bg={} act={:?} pos=({:.1},{:.1}) flag={flag:#04x} target={:?} route={:?} cur_edge={:?} blocked={}", b.id, b.is_background(), b.activity, b.pos.x, b.pos.z, b.target, tgt, ce, b.blocked_ticks);
    }
}

#[allow(dead_code)]
fn _diag() {}
