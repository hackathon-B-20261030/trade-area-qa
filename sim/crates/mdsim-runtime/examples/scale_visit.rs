//! VISIT 결정의 규모 시험: 고정 자료 인물을 복제해 N명으로 늘리고 방문지 선택만 돌려 시간·메모리를 잰다.
//! cargo run --release -p mdsim-runtime --example scale_visit -- 65000
use mdsim_core::bundle::Bundle;
use mdsim_core::defs::Defs;
use mdsim_core::fixture::write_fixture;
use mdsim_core::scenario::Scenario;
use mdsim_runtime::space::Space;
use mdsim_runtime::step::World;
use mdsim_runtime::visit::{representation_ratio, visit_decisions_streamed};
use std::time::Instant;

fn rss_mb() -> f64 {
    let mut ru: libc_rusage = unsafe { std::mem::zeroed() };
    unsafe { getrusage(0, &mut ru) };
    ru.ru_maxrss as f64 / 1024.0 / 1024.0
}

#[repr(C)]
struct libc_rusage {
    ru_utime: [i64; 2],
    ru_stime: [i64; 2],
    ru_maxrss: i64,
    _rest: [i64; 13],
}

extern "C" {
    fn getrusage(who: i32, usage: *mut libc_rusage) -> i32;
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(65_000);
    let root = std::env::temp_dir().join("mdsim-scale");
    let _ = std::fs::remove_dir_all(&root);
    let fx = write_fixture(&root).unwrap();
    let defs = Defs::load(&fx.defs).unwrap();
    let mut bundle = Bundle::load(&fx.bundle, &fx.root, &defs).unwrap();
    let base = bundle.profiles.clone();
    bundle.profiles = (0..n).map(|i| {
        let mut p = base[i % base.len()].clone();
        p.persona_id = format!("{}-{i}", p.persona_id);
        p
    }).collect();
    let space = Space::build(&defs, &bundle).unwrap();
    let scn = Scenario::load(&fx.scenario).unwrap();
    let w = World::new(&defs, &bundle, &space, &scn, "scale".into()).unwrap();
    println!("before visit: maxrss {:.0} MB", rss_mb());
    let t = Instant::now();
    let dec = visit_decisions_streamed(&defs, &space, &bundle, &scn, &w.p, &w.rng, 4096, &mut |_| Ok(())).unwrap();
    let alts: usize = dec.iter().map(|d| d.alts.len()).sum();
    println!("visit: {} personas, {} alts ({:.1}/persona), {:.2}s, ratio {:.3}, maxrss {:.0} MB", dec.len(), alts, alts as f64 / dec.len() as f64, t.elapsed().as_secs_f64(), representation_ratio(&dec, &scn, &w.p), rss_mb());
}
