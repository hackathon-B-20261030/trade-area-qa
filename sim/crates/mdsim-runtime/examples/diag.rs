use mdsim_core::bundle::Bundle;
use mdsim_core::defs::Defs;
use mdsim_core::fixture::write_fixture;
use mdsim_core::geom::V2;
use mdsim_core::scenario::Scenario;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::record::MemSink;
use mdsim_runtime::space::Space;
use mdsim_runtime::step::{Sim, World};
use mdsim_runtime::visit::*;

fn main() {
    let root = std::env::temp_dir().join("mdsim-diag");
    let _ = std::fs::remove_dir_all(&root);
    let fx = write_fixture(&root).unwrap();
    let defs = Defs::load(&fx.defs).unwrap();
    let bundle = Bundle::load(&fx.bundle, &fx.root, &defs).unwrap();
    let space = Space::build(&defs, &bundle).unwrap();
    let scn = Scenario::load(&fx.scenario_quick).unwrap();
    let w = World::new(&defs, &bundle, &space, &scn, "d".into()).unwrap();
    let dec = visit_decisions(&defs, &space, &bundle, &scn, &w.p, &w.rng);
    let ratio = representation_ratio(&dec, &scn, &w.p);
    let mut spawns = plan_arrivals(&space, &bundle, &scn, &w.p, &w.rng, &dec, ratio);
    spawns.extend(plan_background(&space, &bundle, &scn, &w.p, &w.rng));
    let spawns = sort_spawns(spawns);
    let mut sim = Sim::new(&w, &spawns);
    let mut sink = MemSink::default();
    sim.run_until(1200, &mut sink).unwrap();
    let b = sim.state.bodies.iter().find(|b| b.alive && b.blocked_ticks > 100).expect("blocked body").clone();
    println!("body {} pos=({:.2},{:.2}) heading={:.2} act={:?} cur_edge={:?}", b.id, b.pos.x, b.pos.z, b.heading, b.activity, b.cur_edge);
    let r = b.route.as_ref().unwrap();
    println!("route idx={} edges={:?} side={:?} final=({:.1},{:.1})", r.idx, r.edges.iter().map(|(e, f)| (space.edges[*e as usize].id.clone(), *f)).collect::<Vec<_>>(), r.side, r.final_pos.x, r.final_pos.z);
    let env = &sim.state.env;
    let coll = Collective::build(&sim.state.bodies, &space, env, 2.0);
    let mut n = Vec::new();
    coll.neighbors(b.pos, 3.0, &mut n);
    println!("neighbors within 3m: {:?}", n.iter().map(|&j| (j, coll.pos[j as usize].dist(b.pos))).collect::<Vec<_>>());
    for k in 0..16 {
        let a = k as f64 * std::f64::consts::TAU / 16.0;
        let u = V2::from_angle(a);
        print!("{:.0}°:{:.2} ", a.to_degrees(), space.free_distance(b.pos, u, 10.0, env));
    }
    println!();
    println!("walkable here={} flag={:#04x}", space.walkable(b.pos, env), space.flag_at(b.pos));
}
