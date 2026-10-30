//! E226: 기록(C-226)의 완료 기준 ①·③·④ (⑤ manifest 해시는 실행 폴더 쪽 시험). ②·⑥ 예산은 기본 실행에서 재는
//! 느린 시험이라 #[ignore]다(`cargo test -- --ignored`).
mod common;

use common::{observed_quick, prep, scenario, sha_hex, TeeSink};
use mdsim_core::geom::angle_between;
use mdsim_runtime::record::{decode_chunk, encode_frame, AltRow, DecisionRow, EventRow, FrameRec, MemSink, Sink, FLAG_BACKGROUND, FLAG_FOOTING, FLAG_LOOK_PLACE, POS_SCALE_M, RECORD_BYTES};
use mdsim_runtime::state::{Activity, GazeTarget};
use mdsim_runtime::step::SimState;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// 렌더 틱의 몸 상태(그 틱의 step 전 = 렌더 프레임이 찍는 상태).
#[derive(Clone, Copy)]
struct Raw {
    id: u32,
    pos: mdsim_core::geom::V2,
    heading: f64,
    gaze: f64,
    activity: Activity,
    background: bool,
    gaze_target: GazeTarget,
}

/// 빠른 시나리오 25분: 렌더 묶음 바이트와 렌더 틱마다의 실제 몸 상태.
fn recorded() -> &'static (TeeSink, BTreeMap<u64, Vec<Raw>>, u32) {
    static R: OnceLock<(TeeSink, BTreeMap<u64, Vec<Raw>>, u32)> = OnceLock::new();
    R.get_or_init(|| {
        let pr = prep(scenario("quick"));
        let re = pr.w.scn.render_every_ticks;
        let mut sim = pr.sim();
        let mut sink = TeeSink::default();
        let mut raw = BTreeMap::new();
        while sim.state.tick < 3000 {
            let t = sim.state.tick;
            if t.is_multiple_of(re as u64) {
                raw.insert(t, sim.state.bodies.iter().filter(|b| b.alive).map(|b| Raw { id: b.id, pos: b.pos, heading: b.heading, gaze: b.gaze, activity: b.activity, background: b.is_background(), gaze_target: b.gaze_target }).collect());
            }
            sim.run_until(t + 1, &mut sink).expect("step");
        }
        (sink, raw, re)
    })
}

/// 렌더 프레임에서 복원한 위치가 시뮬레이션 위치와 양자화 오차(축마다 POS_SCALE_M/2 = 2.5cm) 안에서 같고, 방향·시선은
/// 1/256바퀴의 반 안, 활동·표지도 같다. 묶음 바이트는 엔진 기록(MemSink)이 해시한 바이트와 같다.
#[test]
fn e226_1_frame_roundtrip() {
    let (sink, raw, _) = recorded();
    assert_eq!(sink.chunks.len(), 2, "10분 묶음 둘(0~1200, 1200~2400틱)");
    let half_step = std::f64::consts::PI / 256.0 + 1e-9;
    let mut n = 0;
    for (k, (s, _, bytes)) in sink.chunks.iter().enumerate() {
        assert_eq!(&sink.mem.chunk_hashes[k], &(*s, sha_hex(bytes)), "엔진 기록의 묶음 해시와 같은 바이트");
        let (_, frames) = decode_chunk(bytes).expect("decode");
        for (tick, recs) in frames {
            let want = &raw[&tick];
            assert_eq!(recs.len(), want.len(), "틱 {tick}: 살아 있는 몸 수");
            for (r, b) in recs.iter().zip(want) {
                assert_eq!(r.body, b.id);
                let p = r.pos();
                assert!((p.x - b.pos.x).abs() <= POS_SCALE_M / 2.0 + 1e-9 && (p.z - b.pos.z).abs() <= POS_SCALE_M / 2.0 + 1e-9, "틱 {tick} 몸 {}: 위치 오차", b.id);
                assert!(angle_between(r.heading_rad(), b.heading) <= half_step, "방향");
                assert!(angle_between(r.gaze_rad(), b.gaze) <= half_step, "시선");
                assert_eq!(r.activity, b.activity.code());
                assert_eq!(r.flags & FLAG_BACKGROUND != 0, b.background);
                assert_eq!(r.flags & FLAG_LOOK_PLACE != 0, matches!(b.gaze_target, GazeTarget::Place(_)));
                assert_eq!(r.flags & FLAG_FOOTING != 0, b.gaze_target == GazeTarget::Footing);
                n += 1;
            }
        }
    }
    assert!(n > 10_000, "대조한 레코드 {n}");
    // 손으로 만든 레코드(경계값 포함)의 쓰기 → 읽기는 그대로다
    let recs = vec![
        FrameRec { body: 0, x: i16::MIN, z: i16::MAX, heading: 0, gaze: 255, focal_range: 0, focal_angle: 255, activity: 1, flags: 0 },
        FrameRec { body: u32::MAX, x: -1, z: 1, heading: 128, gaze: 64, focal_range: 30, focal_angle: 70, activity: 8, flags: 31 },
    ];
    let mut bytes = mdsim_runtime::record::chunk_header(10, 12, 0.5, 2, 1).into_bytes();
    bytes.push(b'\n');
    encode_frame(10, &recs, &mut bytes);
    let (_, frames) = decode_chunk(&bytes).expect("decode");
    assert_eq!(frames, vec![(10, recs.clone())]);
    assert_eq!(bytes.len() - bytes.iter().position(|&c| c == b'\n').unwrap() - 1, 8 + recs.len() * RECORD_BYTES, "프레임 = 머리 8바이트 + 14바이트 × n");
}

/// 10분 묶음은 하나씩 독립적으로 읽힌다: 묶음마다 혼자 읽어도 머리말(형식·틱 범위·프레임 수·레코드 크기·좌표 척도)과
/// 프레임이 맞고, 묶음들이 틱 범위를 빈틈없이 잇는다. 잘린 묶음은 오류로 알린다.
#[test]
fn e226_3_chunk_independent() {
    let (sink, raw, re) = recorded();
    let mut expect_start = 0u64;
    for (s, e, bytes) in &sink.chunks {
        let (hdr, frames) = decode_chunk(bytes).expect("묶음 하나만으로 읽힌다");
        assert_eq!(hdr["format"], "mdsim-frames-v1");
        assert_eq!((hdr["tick_start"].as_u64(), hdr["tick_end"].as_u64()), (Some(*s), Some(*e)));
        assert_eq!(*s, expect_start, "묶음이 빈틈없이 이어진다");
        assert_eq!(hdr["record_bytes"].as_u64(), Some(RECORD_BYTES as u64));
        assert_eq!(hdr["pos_scale_m"].as_f64(), Some(POS_SCALE_M));
        assert_eq!(hdr["render_every"].as_u64(), Some(*re as u64));
        assert_eq!(hdr["n_frames"].as_u64(), Some(frames.len() as u64));
        assert_eq!(frames.len() as u64, (e - s) / *re as u64, "렌더 틱 수");
        let ticks: Vec<u64> = frames.iter().map(|f| f.0).collect();
        let want: Vec<u64> = (*s..*e).step_by(*re as usize).collect();
        assert_eq!(ticks, want, "묶음 안 틱은 [시작, 끝)의 렌더 틱");
        assert!(ticks.iter().all(|t| raw.contains_key(t)));
        expect_start = *e;
        // 잘린 묶음: 마지막 레코드 중간에서 자르면 오류
        let cut = &bytes[..bytes.len() - RECORD_BYTES / 2];
        assert!(decode_chunk(cut).is_err() || frames.last().map(|f| f.1.is_empty()).unwrap_or(true), "잘린 묶음을 조용히 받아들임");
    }
    assert!(decode_chunk(b"no header").is_err());
}

/// 사건 값은 02c §2.6을 따른다(S-E55): exit value = 같은 몸·장소 enter부터 머문 초, rest_end value = rest_start부터
/// 쉰 초(rest_start value 0), queue_leave info는 `served`(같은 틱 enter가 바로 뒤)·`abandoned`이고 value는 기다린 초,
/// 행사 보기(watch) 바로 앞에는 같은 틱 enter가 있다. 행사 보기는 실행 표본에 드물어 행사 앵커에 닿은 몸 하나로 따로 본다.
#[test]
fn e226_4_event_values_follow_contract() {
    let var = common::observe(common::prep(common::scenario("variant")), 4200);
    let watch = watch_events();
    let (mut n_exit, mut n_served) = (0, 0);
    for events in [&observed_quick().sink.events, &var.sink.events, &watch] {
        let dt = observed_quick().prep.w.dt();
        let mut open_visit: BTreeMap<(i64, u32), u64> = BTreeMap::new();
        let mut open_rest: BTreeMap<(i64, u32), u64> = BTreeMap::new();
        for (i, e) in events.iter().enumerate() {
            let Some(p) = e.place else { continue };
            let key = (e.body, p);
            match e.kind.as_str() {
                "enter" => {
                    open_visit.insert(key, e.tick);
                }
                "exit" => {
                    let t0 = open_visit.remove(&key).unwrap_or_else(|| panic!("enter 없는 exit {e:?}"));
                    assert!((e.value - (e.tick - t0) as f64 * dt).abs() < 1e-9, "exit value {} vs 머문 {}초", e.value, (e.tick - t0) as f64 * dt);
                    n_exit += 1;
                }
                "rest_start" => {
                    assert_eq!(e.value, 0.0, "rest_start value");
                    open_rest.insert(key, e.tick);
                }
                "rest_end" => {
                    let t0 = open_rest.remove(&key).unwrap_or_else(|| panic!("rest_start 없는 rest_end {e:?}"));
                    assert!((e.value - (e.tick - t0) as f64 * dt).abs() < 1e-9, "rest_end value");
                }
                "queue_leave" => {
                    assert!(e.info == "served" || e.info == "abandoned", "queue_leave info {:?}", e.info);
                    assert!(e.value >= 0.0, "기다린 초");
                    if e.info == "served" {
                        let next = &events[i + 1];
                        assert!(next.kind == "enter" && next.body == e.body && next.place == e.place && next.tick == e.tick, "served 뒤 enter: {next:?}");
                        n_served += 1;
                    }
                }
                "watch" => {
                    let prev = &events[i - 1];
                    assert!(prev.kind == "enter" && prev.body == e.body && prev.place == e.place && prev.tick == e.tick, "watch 앞 enter: {prev:?}");
                }
                _ => {}
            }
        }
    }
    assert!(n_exit > 10 && n_served > 10, "exit {n_exit}, served {n_served}");
    assert!(watch.iter().any(|e| e.kind == "watch") && watch.iter().any(|e| e.kind == "exit"), "행사 보기 시작과 끝: {watch:?}");
}

/// 진행 중인 행사(15:45~16:15)의 앵커에 닿은 몸 하나가 보기를 시작하고 끝내는 동안의 사건(빈 실행, 15:50~16:10).
fn watch_events() -> Vec<mdsim_runtime::record::EventRow> {
    let m = common::mid_quick();
    let pr = common::prep_empty(common::with_window(common::scenario("variant"), "15:50", "16:10"));
    let w = pr.w;
    let ev = w.space.places.iter().position(|p| p.kind == mdsim_runtime::space::PlaceKind::Event).expect("행사 장소") as u32;
    let mut sim = pr.sim();
    let mut b = common::quiet_walker(m);
    b.id = 0;
    b.entity = "watch#0".into();
    b.arrival_tick = 0;
    b.deadline_tick = w.scn.n_ticks() + 100_000;
    b.pos = w.space.places[ev as usize].anchor;
    b.cur_edge = Some(w.space.places[ev as usize].edge);
    b.target = mdsim_runtime::state::Target::Place(ev);
    b.route = None;
    b.arrived = true;
    b.pending = None;
    b.activity = mdsim_runtime::state::Activity::Walk;
    sim.state.bodies.push(b);
    let mut sink = mdsim_runtime::record::MemSink::default();
    sim.run(&mut sink).expect("run");
    sink.events
}

/// 모든 사건은 에이전트 ID(환경 사건은 없음 = −1)·틱과 연결된다: 틱은 실행 안이고 시각은 그 틱의 시각,
/// 몸은 그 틱에 있던 몸, 장소는 있는 장소이며 장소가 필요한 사건에는 장소가 있다. 결정·대안 기록도 몸·결정과 이어진다.
#[test]
fn e226_4_events_linked() {
    let o = observed_quick();
    let w = o.prep.w;
    let n = o.snaps.len() as u64 - 1;
    let needs_place = ["notice", "enter", "exit", "purchase", "queue_join", "queue_leave", "rest_start", "rest_end", "watch", "abandon"];
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &o.sink.events {
        assert!(e.tick < n && e.t_s == w.t_s(e.tick), "사건 틱·시각 {e:?}");
        *kinds.entry(e.kind.as_str()).or_default() += 1;
        if e.kind == "env" {
            assert_eq!(e.body, -1, "환경 사건에는 몸이 없다");
            continue;
        }
        assert!(e.body >= 0, "몸 사건의 몸 ID {e:?}");
        let b = o.state.bodies.get(e.body as usize).expect("있는 몸");
        assert!(b.arrival_tick <= e.tick && b.departed_tick.map(|t| e.tick <= t).unwrap_or(true), "몸 {}이 없던 틱의 사건 {}", b.id, e.kind);
        if let Some(p) = e.place {
            assert!((p as usize) < w.space.places.len(), "있는 장소");
        }
        if needs_place.contains(&e.kind.as_str()) {
            assert!(e.place.is_some(), "{} 사건에 장소가 없음", e.kind);
        }
        if e.kind == "spawn" || e.kind == "despawn" {
            assert!(w.space.portal_ix(&e.info).is_some(), "출입구 ID {}", e.info);
        }
    }
    for k in ["spawn", "despawn", "notice", "enter", "queue_join"] {
        assert!(kinds.get(k).copied().unwrap_or(0) > 0, "{k} 사건이 있어야 한다: {kinds:?}");
    }
    let ids: BTreeMap<u64, &DecisionRow> = o.sink.decisions.iter().map(|d| (d.id, d)).collect();
    assert_eq!(ids.len(), o.sink.decisions.len(), "결정 ID는 하나씩");
    for d in &o.sink.decisions {
        let b = &o.state.bodies[d.body as usize];
        assert!(b.arrival_tick <= d.tick && d.tick < n);
        assert!(d.rng_key.is_some(), "결정 {}: 난수 키 기록", d.id);
    }
    let mut per: BTreeMap<u64, u32> = BTreeMap::new();
    for a in &o.sink.alts {
        let d = ids.get(&a.decision).expect("대안의 결정");
        assert!(a.alt < d.n_alts);
        *per.entry(a.decision).or_default() += 1;
    }
    for (id, k) in per {
        assert_eq!(k, ids[&id].n_alts, "결정 {id}의 대안 수");
    }
}

/// 기본 실행(14:00~17:00, 목표 동시 2,000명)의 예산 측정: 최대 상주 메모리(RSS, `ps`로 묶음 끝마다 잰다)와
/// 렌더 프레임 크기. 한 번 돌려 두 시험이 함께 쓴다.
struct Budget {
    peak_rss_kb: u64,
    frame_bytes: u64,
    hours: f64,
    peak_alive: usize,
}

struct RssSink {
    inner: MemSink,
    peak_kb: u64,
    frame_bytes: u64,
    peak_alive: usize,
}

fn rss_kb() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

impl Sink for RssSink {
    fn frame(&mut self, tick: u64, recs: &[FrameRec]) {
        self.frame_bytes += 8 + (recs.len() * RECORD_BYTES) as u64;
        self.peak_alive = self.peak_alive.max(recs.len());
        self.inner.frame(tick, recs);
    }

    fn event(&mut self, e: EventRow) {
        self.inner.event(e);
    }

    fn decision(&mut self, d: DecisionRow, alts: Vec<AltRow>) {
        self.inner.decision(d, alts);
    }

    fn chunk_end(&mut self, s: u64, e: u64, state: &SimState, dt_s: f64, render_every: u32) -> mdsim_core::Result<()> {
        self.peak_kb = self.peak_kb.max(rss_kb());
        self.inner.chunk_end(s, e, state, dt_s, render_every)
    }
}

fn budget() -> &'static Budget {
    static B: OnceLock<Budget> = OnceLock::new();
    B.get_or_init(|| {
        let pr = prep(scenario("base"));
        let mut sim = pr.sim();
        let mut sink = RssSink { inner: MemSink::default(), peak_kb: 0, frame_bytes: 0, peak_alive: 0 };
        sim.run(&mut sink).expect("기본 실행");
        let (w0, w1) = pr.w.scn.window_s();
        Budget { peak_rss_kb: sink.peak_kb.max(rss_kb()), frame_bytes: sink.frame_bytes, hours: (w1 - w0) / 3600.0, peak_alive: sink.peak_alive }
    })
}

/// 기록 중 최대 메모리가 예산(3GB, memguard) 안이다.
#[test]
#[ignore = "예산 시험: 기본 실행(3시간, 목표 동시 2,000명)을 끝까지 돈다"]
fn e226_2_memory_budget() {
    let b = budget();
    println!("최대 RSS {:.1}MB, 최대 동시 {}명", b.peak_rss_kb as f64 / 1024.0, b.peak_alive);
    assert!(b.peak_rss_kb > 0, "ps로 RSS를 읽지 못함");
    assert!(b.peak_rss_kb <= 3 * 1024 * 1024, "최대 RSS {}KB > 3GB", b.peak_rss_kb);
}

/// 시뮬레이션 1시간당 렌더 프레임 크기가 Q-F1의 예산 안이다: 기본 실행(A안, 3시간·동시 약 2,000명)의 기록 최대
/// 약 260MB(압축 전) → 1시간당 260/3MB.
#[test]
#[ignore = "예산 시험: 기본 실행(3시간, 목표 동시 2,000명)을 끝까지 돈다"]
fn e226_6_size_budget() {
    let b = budget();
    let per_hour = b.frame_bytes as f64 / b.hours;
    println!("렌더 프레임 {:.1}MB (1시간당 {:.1}MB), 최대 동시 {}명", b.frame_bytes as f64 / 1e6, per_hour / 1e6, b.peak_alive);
    assert!(per_hour <= 260e6 / 3.0, "1시간당 {:.1}MB > {:.1}MB", per_hour / 1e6, 260.0 / 3.0);
}
