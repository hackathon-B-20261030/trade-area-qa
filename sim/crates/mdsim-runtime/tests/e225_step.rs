//! E225: 스냅샷과 틱 실행기(C-225)의 완료 기준 ①~⑤ (⑥ manifest 필드는 실행 폴더 쪽 시험).
mod common;

use common::{prep, prof_of, scenario, state_hash, with_threads, TeeSink};
use mdsim_runtime::behavior::effective_caps;
use mdsim_runtime::collective::Collective;
use mdsim_runtime::env::env_at;
use mdsim_runtime::perception::{look_decide, look_situation, perceive};
use mdsim_runtime::record::{checkpoint_bytes, restore_state, MemSink};
use mdsim_runtime::state::Aware;
use mdsim_runtime::step::{Sim, SimState};
use std::collections::{BTreeMap, BTreeSet};

/// 같은 S(t)와 입력은 같은 S(t+Δt)를 낸다: 체크포인트 바이트에서 되살린 두 상태와 원래 상태를 한 틱씩 20틱 진행하면
/// 매 틱 스냅샷 해시와 기록(사건·결정)이 같다.
#[test]
fn e225_1_step_pure() {
    let pr = prep(scenario("quick"));
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    sim.run_until(900, &mut sink).expect("run");
    let bytes = checkpoint_bytes(&sim.state).expect("ckpt");
    assert_eq!(checkpoint_bytes(&sim.state).expect("ckpt"), bytes, "같은 상태 → 같은 체크포인트 바이트");
    let (s1, s2) = (restore_state(&bytes).expect("restore"), restore_state(&bytes).expect("restore"));
    assert_eq!(s1, sim.state, "체크포인트 왕복은 상태를 그대로 되살린다");
    let mut a = Sim::from_state(pr.w, pr.spawns, s1).expect("from_state");
    let mut b = Sim::from_state(pr.w, pr.spawns, s2).expect("from_state");
    let (mut ka, mut kb, mut ko) = (MemSink::default(), MemSink::default(), MemSink::default());
    for _ in 0..20 {
        a.step(&mut ka).expect("step");
        b.step(&mut kb).expect("step");
        sim.step(&mut ko).expect("step");
        let h = state_hash(&a.state);
        assert_eq!(h, state_hash(&b.state));
        assert_eq!(h, state_hash(&sim.state));
    }
    assert_eq!((&ka.events, &ka.decisions, &ka.alts), (&kb.events, &kb.decisions, &kb.alts));
    assert_eq!((&ka.events, &ka.decisions, &ka.alts), (&ko.events, &ko.decisions, &ko.alts));
    assert!(!ka.decisions.is_empty() || !ka.events.is_empty(), "20틱 동안 무언가 일어나야 비교가 의미 있다");
}

/// 같은 실행을 병렬 작업자 수 1·3·8로 돌린 결과(렌더 묶음 해시, 사건·결정·대안, 끝 스냅샷)가 같다 —
/// 몸별 단계의 처리 순서와 분할이 결과를 바꾸지 않는다.
#[test]
fn e225_2_order_and_partition() {
    let pr = prep(scenario("quick"));
    let run = |threads: usize| -> (MemSink, String) {
        with_threads(threads, || {
            let mut sim = pr.sim();
            let mut sink = MemSink::default();
            sim.run_until(2400, &mut sink).expect("run");
            let h = state_hash(&sim.state);
            (sink, h)
        })
    };
    let (base, h0) = run(1);
    assert_eq!(base.chunk_hashes.len(), 2);
    for threads in [3, 8] {
        let (s, h) = run(threads);
        assert_eq!(s.chunk_hashes, base.chunk_hashes, "작업자 {threads}: 렌더 묶음");
        assert_eq!(s.events, base.events, "작업자 {threads}: 사건");
        assert_eq!((&s.decisions, &s.alts), (&base.decisions, &base.alts), "작업자 {threads}: 결정");
        assert_eq!(h, h0, "작업자 {threads}: 끝 스냅샷");
    }
}

/// 10분 경계(틱 1200)의 체크포인트에서 다시 돌린 결과가 끊김 없는 실행과 비트 단위로 같다: 그 뒤 렌더 묶음 바이트,
/// 사건·결정, 끝 스냅샷. 묶음 끝에 기록이 남긴 체크포인트도 그 틱의 상태 바이트와 같다.
#[test]
fn e225_3_checkpoint_resume_bytes() {
    let pr = prep(scenario("quick"));
    let mut whole = pr.sim();
    let mut a = TeeSink::default();
    a.mem.keep_checkpoints = true;
    whole.run_until(3600, &mut a).expect("run");
    let mut head = pr.sim();
    let mut first = MemSink::default();
    head.run_until(1200, &mut first).expect("run");
    let ckpt = checkpoint_bytes(&head.state).expect("ckpt");
    assert_eq!(a.mem.checkpoints[0], (1200, ckpt.clone()), "묶음 끝 체크포인트 = 그 틱 상태의 바이트");
    let mut resumed = Sim::from_state(pr.w, pr.spawns, restore_state(&ckpt).expect("restore")).expect("from_state");
    let mut b = TeeSink::default();
    resumed.run_until(3600, &mut b).expect("run");
    assert_eq!(b.chunks.len(), 2);
    for (k, chunk) in b.chunks.iter().enumerate() {
        assert_eq!(chunk, &a.chunks[k + 1], "재개 뒤 {k}번째 렌더 묶음 바이트");
        assert_eq!(b.mem.chunk_hashes[k], a.mem.chunk_hashes[k + 1]);
    }
    let after = |s: &TeeSink| -> (Vec<_>, Vec<_>) { (s.mem.events.iter().filter(|e| e.tick >= 1200).cloned().collect(), s.mem.decisions.iter().filter(|d| d.tick >= 1200).cloned().collect()) };
    assert_eq!(after(&b), after(&a), "재개 뒤 사건·결정");
    assert_eq!(state_hash(&resumed.state), state_hash(&whole.state), "끝 스냅샷");
    assert_eq!(checkpoint_bytes(&resumed.state).expect("ckpt"), checkpoint_bytes(&whole.state).expect("ckpt"));
    // 다른 실행(시나리오)의 체크포인트는 받지 않는다
    let other = prep(scenario("variant"));
    assert!(Sim::from_state(other.w, other.spawns, restore_state(&ckpt).unwrap()).is_err(), "시나리오 해시가 다른 체크포인트 거부");
}

/// 각 단계는 S(t)와 같은 틱의 앞 단계 결과만 읽는다: 도착이 없는 틱 t의 스냅샷 S(t)만으로
/// ④ 지각(알아차림 사건)과 ⑤ 시선 결정(추적 대상 몸의 LOOK 대안 수·선택·대안별 확률)을 다시 계산하면 실행 기록과
/// 같다(본 간선 밀도가 틱 t 위치의 값인지는 e210_1이 본다). (02d에는 단계별 읽기·쓰기 등록부가 없어, 등록부로 "S(t+Δt)를 읽는 가짜 규칙"을 걸러내는
/// 시험 대신 다시 계산이 같은지로 본다.)
#[test]
fn e225_4_phase_reads() {
    let pr = prep(scenario("quick"));
    let w = pr.w;
    let spawn_ticks: BTreeSet<u64> = pr.spawns.iter().map(|s| s.tick).collect();
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    let mut saved: Vec<SimState> = Vec::new();
    while sim.state.tick < 2400 {
        let t = sim.state.tick;
        if t % 37 == 5 && t > 300 && !spawn_ticks.contains(&t) {
            saved.push(sim.state.clone());
        }
        sim.run_until(t + 1, &mut sink).expect("step");
    }
    let alts: BTreeMap<u64, Vec<&mdsim_runtime::record::AltRow>> = sink.alts.iter().fold(BTreeMap::new(), |mut m, a| {
        m.entry(a.decision).or_insert_with(Vec::new).push(a);
        m
    });
    let (mut n_bodies, mut n_look) = (0, 0);
    for st in &saved {
        let t = st.tick;
        let env = env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(t));
        let coll = Collective::build(&st.bodies, w.space, &env, w.p.crowd.local_radius_m);
        let ql: Vec<u32> = st.queues.iter().map(|q| q.waiting.len() as u32).collect();
        for b in st.bodies.iter().filter(|b| b.alive && !b.is_background() && b.activity.on_street()) {
            let prof = prof_of(w, b);
            let po = perceive(w.space, &env, &coll, &ql, &w.p, prof, b, &w.rng, t, w.dt());
            let logged: BTreeSet<u32> = sink.events.iter().filter(|e| e.tick == t && e.kind == "notice" && e.body == b.id as i64).map(|e| e.place.unwrap()).collect();
            assert_eq!(po.notices.iter().copied().collect::<BTreeSet<_>>(), logged, "틱 {t} 몸 {}: 알아차림", b.id);
            n_bodies += 1;
            // ⑤ LOOK: ④의 결과(새 인지 항목·관찰 값)를 몸에 적용한 뒤의 상황
            let Some(d) = sink.decisions.iter().find(|d| d.tick == t && d.body == b.id as i64 && d.context == "LOOK") else { continue };
            let mut bb = b.clone();
            for &p in &po.notices {
                let i = bb.aware.partition_point(|a| a.place < p);
                bb.aware.insert(i, Aware { place: p, first_tick: t, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: false });
            }
            for &(p, open, q) in &po.seen {
                if let Some(a) = bb.aware.iter_mut().find(|a| a.place == p) {
                    (a.last_seen_tick, a.seen_open, a.seen_queue) = (Some(t), Some(open), Some(q));
                }
            }
            let rho = coll.local_density[b.id as usize];
            let ch = look_situation(w.space, &env, &w.p, &w.defs.dims, &bb, &po.visible, rho, t, w.dt(), prof.perc.head_turn_deg);
            let caps = effective_caps(w.defs, prof, &bb, 1e6);
            let r = look_decide(&ch, prof, &bb, &w.defs.dims, &w.p, &w.rng, t, &caps);
            assert_eq!((d.n_alts as usize, d.chosen as usize), (ch.len(), r.chosen), "틱 {t} 몸 {}: LOOK", b.id);
            let probs: Vec<f64> = alts[&d.id].iter().map(|a| a.prob).collect();
            assert_eq!(probs, r.probs, "틱 {t} 몸 {}: LOOK 대안별 확률", b.id);
            n_look += 1;
        }
    }
    assert!(n_bodies > 200 && n_look > 10, "다시 계산한 지각 {n_bodies}, LOOK {n_look}");
}

/// 활동 인원(t+Δt) = 활동 인원(t) + 도착 − 퇴장: 빠른 시나리오 1시간 전체를 틱마다 대조한다(엔진의 보존 검사도 통과).
#[test]
fn e225_5_conservation() {
    let pr = prep(scenario("quick"));
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    let n = pr.w.scn.n_ticks();
    let mut alive = 0i64;
    let mut seen_events = 0usize;
    let (mut total_in, mut total_out) = (0, 0);
    while sim.state.tick < n {
        let t = sim.state.tick;
        sim.run_until(t + 1, &mut sink).expect("엔진 보존 검사 통과");
        let new = &sink.events[seen_events..];
        let spawned = new.iter().filter(|e| e.kind == "spawn").count() as i64;
        let gone = new.iter().filter(|e| e.kind == "despawn").count() as i64;
        seen_events = sink.events.len();
        alive += spawned - gone;
        total_in += spawned;
        total_out += gone;
        assert_eq!(sim.state.bodies.iter().filter(|b| b.alive).count() as i64, alive, "틱 {t}");
        assert_eq!(sim.state.bodies.len() as i64, total_in, "생긴 몸은 모두 상태에 남는다");
    }
    assert!(total_in > 200 && total_out > 50, "도착 {total_in}, 퇴장 {total_out}");
    assert_eq!(sim.state.next_spawn, pr.spawns.iter().filter(|s| s.tick < n).count());
}
