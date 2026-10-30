//! E206: 개인 상태와 시간(C-206)의 완료 기준 ①~⑥.
mod common;

use common::{ctx, fx, mid_quick, observed_quick, prep, prep_empty, quiet_walker, route_plan, scenario, with_window, Mid};
use mdsim_core::geom::V2;
use mdsim_runtime::behavior::{effective_caps, passive_recovery, person};
use mdsim_runtime::react::react;
use mdsim_runtime::record::{EventRow, MemSink};
use mdsim_runtime::route::Path;
use mdsim_runtime::situation::{rest_situation, route_situation, Action, AltTarget};
use mdsim_runtime::space::PlaceKind;
use mdsim_runtime::state::{Activity, Aware, Body, GazeTarget, Target};
use mdsim_runtime::step::{Sim, World};
use std::collections::BTreeMap;

/// 상태 변화는 반응 결과와 등록된 규칙으로만 정해진다(C-206 ①, E206-1). 엔진 소스에서 몸의 상태 필드를 직접 쓰는 곳은
/// 등록된 모듈(state.rs 갱신 함수, behavior.rs 결정 적용, step.rs 틱 단계)뿐이다(02d I-206-r, S-E50). 필드는 통합 시험이
/// 상황을 꾸미도록 `pub`이라 컴파일 대신 소스를 읽어 거절한다. state.rs의 등록 함수 호출은 어디서나 된다.
/// (숨은 상태가 없음 — 스냅샷만으로 이어 돌린 실행이 끊김 없는 실행과 같음 — 은 e225_3이 본다.)
#[test]
fn e206_1_state_writes_registered() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("src").map(|e| e.expect("항목").path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    files.sort();
    assert!(files.len() >= 15, "엔진 소스 {}개", files.len());
    let registered = ["state.rs", "behavior.rs", "step.rs"];
    let mut found = Vec::new();
    let mut n_registered = 0;
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(f).expect("읽기");
        let writes: Vec<String> = src.lines().enumerate().filter(|(_, l)| state_write(l)).map(|(i, l)| format!("{name}:{}: {}", i + 1, l.trim())).collect();
        if registered.contains(&name.as_str()) {
            n_registered += writes.len();
        } else {
            found.extend(writes);
        }
    }
    assert!(found.is_empty(), "등록 모듈 밖에서 몸 상태를 직접 씀: {found:#?}");
    // 검사기가 실제 쓰기를 찾는다(등록 모듈의 쓰기, 꾸민 위반 예).
    assert!(n_registered > 20, "등록 모듈의 상태 쓰기 {n_registered}");
    for bad in ["b.fatigue = 0.5;", "b.sat[2] += 0.1;", "self.state.bodies[i].pos = p;", "body.aware.push(a);", "b.route = None;", "b.alive=false;"] {
        assert!(state_write(bad), "위반 예를 찾지 못함: {bad}");
    }
    for ok in ["let x = b.fatigue;", "if b.alive == other.alive {", "o.pos = np;", "b.set_activity(Activity::Walk)", "// b.fatigue = 1.0;", "out.route = r;"] {
        assert!(!state_write(ok), "쓰기가 아닌 줄을 위반으로 봄: {ok}");
    }
}

/// 몸(`b`·`body`·`bodies[…]`)의 C-206 상태 필드를 직접 쓰는 줄인가(주석 뒤는 보지 않는다).
fn state_write(line: &str) -> bool {
    const FIELDS: [&str; 14] = ["activity", "fatigue", "sat", "spent", "aware", "pos", "deadline_tick", "entered", "stroller", "luggage", "child", "alive", "target", "route"];
    const MUTATORS: [&str; 13] = ["push(", "insert(", "clear(", "retain(", "remove(", "truncate(", "as_mut(", "take(", "get_or_insert", "iter_mut(", "extend(", "sort", "dedup"];
    let code = line.split("//").next().unwrap_or("");
    let bytes = code.as_bytes();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    for f in FIELDS {
        let pat = format!(".{f}");
        let mut from = 0;
        while let Some(k) = code[from..].find(&pat) {
            let at = from + k;
            from = at + pat.len();
            let after = at + pat.len();
            if after < bytes.len() && is_ident(bytes[after]) {
                continue; // .position 같은 다른 이름
            }
            // 받는 쪽: 점 앞의 식별자, 또는 `이름[…]`의 이름
            let mut e = at;
            if e > 0 && bytes[e - 1] == b']' {
                let mut depth = 0;
                while e > 0 {
                    e -= 1;
                    match bytes[e] {
                        b']' => depth += 1,
                        b'[' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            let mut s0 = e;
            while s0 > 0 && is_ident(bytes[s0 - 1]) {
                s0 -= 1;
            }
            if !matches!(&code[s0..e], "b" | "body" | "bodies") {
                continue;
            }
            // 필드 뒤: 선택적 첨자 다음 대입 연산자 또는 변경 메서드
            let mut rest = code[after..].trim_start();
            if rest.starts_with('[') {
                if let Some(close) = rest.find(']') {
                    rest = rest[close + 1..].trim_start();
                }
            }
            let assign = (rest.starts_with('=') && !rest.starts_with("==")) || ["+=", "-=", "*=", "/="].iter().any(|op| rest.starts_with(op));
            let mutate = rest.strip_prefix('.').is_some_and(|m| MUTATORS.iter().any(|x| m.starts_with(x)));
            if assign || mutate {
                return true;
            }
        }
    }
    false
}

/// 빈 실행에 걷는 몸 하나를 넣는다(모든 장소를 이미 알아 새 알아차림 계기가 없고, 마감이 멀다).
fn inject_walker(sim: &mut Sim, m: &Mid, profile: u32, pos: V2, path: &Path, final_pos: V2, target: Target) -> u32 {
    let w = sim.w;
    let id = sim.state.bodies.len() as u32;
    let mut b = quiet_walker(m);
    b.id = id;
    b.profile = Some(profile);
    b.entity = format!("crafted#{id}");
    b.base_speed = w.bundle.profiles[profile as usize].base_speed_mps;
    b.arrival_tick = sim.state.tick;
    b.deadline_tick = sim.state.tick + 100_000;
    b.pos = pos;
    b.heading = (w.space.graph.node_pos[path.nodes[1] as usize] - pos).angle();
    b.gaze = b.heading;
    b.gaze_target = GazeTarget::Path;
    b.gaze_since = sim.state.tick;
    b.activity = Activity::Walk;
    b.act_place = None;
    b.act_until = 0;
    b.target = target;
    b.route = Some(route_plan(path, final_pos));
    b.cur_edge = Some(path.edges[0].0);
    b.pending = None;
    b.arrived = false;
    b.fatigue = 0.0;
    b.spent = 0.0;
    b.sat = [0.0; 7];
    b.stroller = false;
    b.luggage = false;
    b.obs_edges.clear();
    b.new_notices.clear();
    b.last_decision = [None; 5];
    b.last_notice_trigger = None;
    b.aware = (0..w.space.places.len() as u32).map(|p| Aware { place: p, first_tick: sim.state.tick, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: true }).collect();
    b.stats = Default::default();
    sim.state.bodies.push(b);
    id
}

/// ROUTE 결정의 대안별 확률(엔진의 route() 와 같은 함수: 경로 상황 → 유효 능력 → 반응).
fn route_probs(w: &World, b: &Body, tick: u64, place: u32) -> Vec<(Action, AltTarget, f64, Option<Path>)> {
    let env = mdsim_runtime::env::env_at(w.scn, w.space, w.p.perception.night_light, w.t_s(tick));
    let c = ctx(w, &env, tick);
    let pl = &w.space.places[place as usize];
    let prof = &w.bundle.profiles[b.profile.unwrap() as usize];
    let ch = route_situation(&c, b, pl.node_near, pl.anchor, b.target_purpose + b.choice_margin, pl.offer, b.fallback_u);
    let alts: Vec<_> = ch.iter().map(|x| x.alt.clone()).collect();
    let caps = effective_caps(w.defs, prof, b, b.minutes_left(tick, w.dt()));
    let r = react(&person(prof), &caps, &b.sat, b.minutes_left(tick, w.dt()), &alts, &w.defs.dims, &w.p.react, 0.5);
    ch.into_iter().zip(r.probs).map(|(x, p)| (x.action, x.target, p, x.path)).collect()
}

/// 같은 길(계단을 오르는 최단 경로)을 연속으로 두 번 가면 두 번째의 회피 쪽(최단 경로 그대로 가기 밖의 대안: 우회·천천히·
/// 연기·포기) 확률이 첫 번째보다 작지 않다. 첫 번째 길을 실제로 걸어 쌓인 상태(피로 등)를 그대로 지닌 채 출발점에서 다시 고른다.
#[test]
fn e206_2_repeat_route_avoidance() {
    let m = mid_quick();
    let pr = prep_empty(with_window(scenario("quick"), "14:00", "15:00"));
    let w = pr.w;
    let f = fx();
    let start = common::node_at(w.space, V2::new(102.0, 68.0));
    let steps_edge = w.space.edges.iter().position(|e| e.is_steps).expect("계단 간선") as u32;
    // 오르막 부담이 드러나도록 걷기·오르기 능력이 낮은 내국인
    let profile = (0..400u32).min_by(|&a, &b| f.bundle.profiles[a as usize].comfort[3].total_cmp(&f.bundle.profiles[b as usize].comfort[3])).unwrap();
    let speed = f.bundle.profiles[profile as usize].base_speed_mps.max(0.3);
    // 계단을 오르는 최단 경로로 닿는 장소
    let (place, path) = (0..w.space.places.len() as u32)
        .filter(|&p| w.space.places[p as usize].kind == PlaceKind::Shop)
        .find_map(|p| {
            let pl = &w.space.places[p as usize];
            let path = w.space.graph.shortest(start, pl.node_near, &|e, _| w.space.walk_cost(e, speed, w.p.route.crossing_wait_s))?;
            path.edges.iter().any(|&(e, _)| e == steps_edge).then_some((p, path))
        })
        .expect("계단을 지나는 목적지");
    let mut sim = Sim::new(w, &[]);
    let mut sink = MemSink::default();
    let anchor = w.space.places[place as usize].anchor;
    let s0 = w.space.graph.node_pos[start as usize];
    let id = inject_walker(&mut sim, m, profile, s0, &path, anchor, Target::Place(place));
    sim.state.bodies[id as usize].target_purpose = 1.0;
    let avoid = |probs: &[(Action, AltTarget, f64, Option<Path>)]| -> f64 {
        let first = probs.iter().find(|x| x.0 == Action::Proceed).expect("PROCEED");
        assert_eq!(first.3.as_ref(), Some(&path), "PROCEED는 계단 최단 경로");
        1.0 - first.2
    };
    let p1 = avoid(&route_probs(w, &sim.state.bodies[id as usize], sim.state.tick, place));
    let fatigue0 = sim.state.bodies[id as usize].fatigue;
    // 첫 번째로 실제로 걷는다(계단을 오를 때까지가 아니라 목적지에 닿을 때까지)
    let mut reached = false;
    for _ in 0..3000 {
        let t = sim.state.tick;
        sim.run_until(t + 1, &mut sink).expect("step");
        let b = &sim.state.bodies[id as usize];
        if b.arrived || b.target != Target::Place(place) {
            reached = b.arrived;
            break;
        }
    }
    assert!(reached, "첫 번째 길을 끝까지 걸음");
    let walked = sim.state.bodies[id as usize].clone();
    assert!(walked.fatigue > fatigue0, "걸은 만큼 피로가 쌓임");
    assert!(sink.decisions.iter().all(|d| d.context == "LOOK"), "걷는 동안 다른 결정이 끼지 않음");
    // 같은 출발점에서 같은 길을 다시: 걸어서 바뀐 상태는 그대로 두고 자리·경로만 처음으로
    let mut again = walked.clone();
    again.pos = s0;
    again.route = Some(route_plan(&path, anchor));
    again.cur_edge = Some(path.edges[0].0);
    again.arrived = false;
    again.target = Target::Place(place);
    let p2 = avoid(&route_probs(w, &again, sim.state.tick, place));
    assert!(p2 >= p1 - 1e-12, "두 번째 회피 확률 {p2:.6} < 첫 번째 {p1:.6} (피로 {:.4} → {:.4})", fatigue0, walked.fatigue);
}

/// 앉아 쉬기는 서서 쉬기보다 피로를 더 줄이고 시간을 쓴다: 회복 규칙(분당 앉음·서 있음), 쉬기 맥락의 앉아 쉬기 대안의
/// 추가 시간, 그리고 실행에서 벤치(3석)에 넷이 함께 닿아 셋은 앉고 하나는 서서 쉰 결과.
#[test]
fn e206_3_seated_vs_standing() {
    let m = mid_quick();
    let w = m.w();
    let dt = w.dt();
    let mut seated = quiet_walker(m);
    seated.activity = Activity::Rest;
    seated.fatigue = 0.6;
    seated.rest_seated = true;
    let mut standing = seated.clone();
    standing.rest_seated = false;
    for _ in 0..(4.0 * 60.0 / dt) as usize {
        passive_recovery(&mut seated, dt, &w.p);
        passive_recovery(&mut standing, dt, &w.p);
    }
    assert!((seated.fatigue - (0.6 - 4.0 * w.p.state.recover_seated_per_min)).abs() < 1e-9);
    assert!((standing.fatigue - (0.6 - 4.0 * w.p.state.recover_standing_per_min)).abs() < 1e-9);
    assert!(seated.fatigue < standing.fatigue, "앉아 쉬기가 더 회복");
    // 쉬기 맥락: 앉아 쉬기는 rest.seat_min을 쓰는 대안(서서 계속 가기는 추가 시간 없음)
    let bench = w.space.places.iter().position(|p| p.kind == PlaceKind::Bench).unwrap() as u32;
    let mut b = quiet_walker(m);
    b.pos = w.space.places[bench as usize].anchor + V2::new(6.0, 0.0);
    b.cur_edge = Some(w.space.places[bench as usize].edge);
    if b.aware_of(bench).is_none() {
        let i = b.aware.partition_point(|a| a.place < bench);
        b.aware.insert(i, Aware { place: bench, first_tick: 0, last_seen_tick: None, seen_open: None, seen_queue: None, known_before: true });
    }
    let c = ctx(w, &m.state.env, m.state.tick);
    let ch = rest_situation(&c, common::prof_of(w, &b), &b);
    let seat = ch.iter().find(|x| x.action == Action::RestSeat).expect("앉아 쉬기 대안");
    let cont = ch.iter().find(|x| x.action == Action::Continue).unwrap();
    assert!(seat.alt.dt_min >= w.p.rest.seat_min && seat.dwell_min == w.p.rest.seat_min, "앉아 쉬기는 시간을 쓴다");
    assert_eq!(cont.alt.dt_min, 0.0);
    // 실행: 3석 벤치에 피로한 넷이 동시에 닿는다
    let pr = prep_empty(with_window(scenario("quick"), "14:00", "14:30"));
    let w2 = pr.w;
    let mut sim = Sim::new(w2, &[]);
    let mut sink = MemSink::default();
    let anchor = w2.space.places[bench as usize].anchor;
    let node = w2.space.places[bench as usize].node_near;
    let (e, other) = w2.space.graph.neighbors(node)[0];
    let path = Path { nodes: vec![other, node], edges: vec![(e, w2.space.graph.edge_a[e as usize] == other)], length: w2.space.edges[e as usize].len };
    let ids: Vec<u32> = (0..4)
        .map(|k| {
            let id = inject_walker(&mut sim, m, k, anchor + V2::new(0.3 * k as f64, 0.0), &path, anchor, Target::Place(bench));
            let b = &mut sim.state.bodies[id as usize];
            b.fatigue = 0.9;
            b.arrived = true;
            id
        })
        .collect();
    sim.run_until(1, &mut sink).expect("step");
    assert!(ids.iter().all(|&i| sim.state.bodies[i as usize].activity == Activity::Rest), "벤치에 닿으면 쉰다");
    let seated: Vec<bool> = ids.iter().map(|&i| sim.state.bodies[i as usize].rest_seated).collect();
    assert_eq!(seated.iter().filter(|&&s| s).count(), 3, "3석: 셋은 앉고 하나는 선다 {seated:?}");
    let until = sim.state.bodies[ids[0] as usize].act_until;
    assert!(until as f64 * dt >= w2.p.rest.seat_min * 60.0 - 1e-9, "쉬는 시간 rest.seat_min");
    sim.run_until(1 + (4.0 * 60.0 / dt) as u64, &mut sink).expect("run");
    let fat: Vec<f64> = ids.iter().map(|&i| sim.state.bodies[i as usize].fatigue).collect();
    let max_seated = (0..4).filter(|&k| seated[k]).map(|k| fat[k]).fold(0.0, f64::max);
    let standing_f = (0..4).find(|&k| !seated[k]).map(|k| fat[k]).unwrap();
    assert!(max_seated < standing_f, "4분 뒤 피로: 앉은 몸 ≤ {max_seated:.3}, 선 몸 {standing_f:.3}");
    assert!(ids.iter().all(|&i| sim.state.bodies[i as usize].activity == Activity::Rest), "쉬는 시간 동안 쉰다(시간을 쓴다)");
}

/// 몸 하나가 받은 NEXT 결정(틱, 계기).
fn next_decisions(sink: &MemSink, id: u32) -> Vec<(u64, String)> {
    sink.decisions.iter().filter(|d| d.body == id as i64 && d.context == "NEXT").map(|d| (d.tick, d.trigger.clone())).collect()
}

/// 영업 종료가 재평가를 일으킨다: 15시에 닫는 가게로 가던 몸은 닫힌 것을 보거나 닿은 뒤 곧 NEXT를 다시 고르고
/// 그 가게를 목표에서 뺀다(그 전에는 다시 고르지 않는다).
#[test]
fn e206_4_env_change_reeval() {
    let m = mid_quick();
    let pr = prep_empty(with_window(scenario("base"), "14:58", "15:06"));
    let w = pr.w;
    let place = w.space.places.iter().position(|p| p.close_min == 900.0).expect("15시 마감") as u32;
    let pl = &w.space.places[place as usize];
    let close_tick = ((900.0 * 60.0 - w.t_s(0)) / w.dt()).round() as u64;
    // 목적지까지 그래프 거리 150~260m의 걷기 가능한 출발 노드
    let (start, path) = (0..w.space.graph.n_nodes() as u32)
        .filter(|&n| w.space.walkable_static(w.space.graph.node_pos[n as usize]))
        .filter_map(|n| w.space.graph.shortest(n, pl.node_near, &|e, _| w.space.edges[e as usize].len).map(|p| (n, p)))
        .find(|(_, p)| (150.0..260.0).contains(&p.length) && p.edges.len() >= 2 && p.edges.iter().all(|&(e, _)| !w.space.edges[e as usize].sidewalks))
        .expect("출발 노드");
    let mut sim = Sim::new(w, &[]);
    let mut sink = MemSink::default();
    let id = inject_walker(&mut sim, m, 0, w.space.graph.node_pos[start as usize], &path, pl.anchor, Target::Place(place));
    let a = sim.state.bodies[id as usize].aware.iter_mut().find(|a| a.place == place).unwrap();
    (a.seen_open, a.last_seen_tick) = (Some(true), Some(0));
    sim.run_until(w.scn.n_ticks(), &mut sink).expect("run");
    let nexts = next_decisions(&sink, id);
    assert!(nexts.iter().all(|(t, _)| *t >= close_tick), "닫히기 전 재평가 {nexts:?}");
    let re = nexts.iter().find(|(_, tr)| tr == "관찰 값 변화").unwrap_or_else(|| panic!("영업 종료 뒤 재평가 없음: {nexts:?}"));
    assert!(re.0 - close_tick <= (180.0 / w.dt()) as u64, "닫힌 뒤 3분 안에 재평가({}틱 뒤)", re.0 - close_tick);
    assert_ne!(sim.state.bodies[id as usize].target, Target::Place(place), "닫힌 가게를 목표에서 뺌");
    assert!(sink.events.iter().any(|e: &EventRow| e.kind == "env" && e.info == "close" && e.place == Some(place) && e.tick == close_tick), "영업 종료 환경 사건");
}

/// 행사 시작이 재평가를 일으킨다: 행사 장소를 이미 알고(시작 전에 닫힌 것을 본) 그 앞을 지나는 몸이 행사가 시작한 것을
/// 보면 NEXT를 다시 고른다(01b §3.3 계기 표의 '환경 사건: 행사 시작'). (비는 엔진·시나리오에 날씨 변수가 없어 다루지 않는다.)
#[test]
fn e206_4_event_start_reeval() {
    let m = mid_quick();
    let pr = prep_empty(with_window(scenario("variant"), "15:44", "15:48"));
    let w = pr.w;
    let ev = w.space.places.iter().position(|p| p.kind == PlaceKind::Event).unwrap() as u32;
    let start_tick = ((945.0 * 60.0 - w.t_s(0)) / w.dt()).round() as u64;
    // 행사 장소(80,133) 서쪽에서 동쪽 먼 가게로 z=136 길을 따라 걷는다
    let from = common::node_at(w.space, V2::new(68.0, 136.0));
    let far = w.space.places.iter().position(|p| p.kind == PlaceKind::Shop && p.anchor.x > 150.0 && p.anchor.z > 130.0 && p.anchor.z < 142.0).unwrap() as u32;
    let path = w.space.graph.shortest(from, w.space.places[far as usize].node_near, &|e, _| w.space.edges[e as usize].len).unwrap();
    let mut sim = Sim::new(w, &[]);
    let mut sink = MemSink::default();
    let id = inject_walker(&mut sim, m, 0, w.space.graph.node_pos[from as usize] + V2::new(-30.0, 0.0), &path, w.space.places[far as usize].anchor, Target::Place(far));
    sim.state.bodies[id as usize].base_speed = 0.3; // 천천히 걸어 행사가 시작한 뒤에 그 앞을 지난다
    let a = sim.state.bodies[id as usize].aware.iter_mut().find(|a| a.place == ev).unwrap();
    (a.seen_open, a.last_seen_tick) = (Some(false), Some(0));
    sim.run_until(w.scn.n_ticks(), &mut sink).expect("run");
    let saw_open = sim.state.bodies[id as usize].aware_of(ev).and_then(|a| a.seen_open) == Some(true);
    assert!(saw_open, "몸이 행사가 열린 것을 봄");
    let nexts = next_decisions(&sink, id);
    assert!(nexts.iter().any(|(t, _)| *t >= start_tick), "행사 시작을 본 뒤 재평가 없음: {nexts:?}");
}

/// 유료 쉼터(카페)에 쉬러 가면 줄에 설 때 앉을 자리로 정해지고 안에서는 앉아 회복한다(S-E53). 같은 몸이 서서
/// 회복하면 피로가 덜 준다. 나오면(활동 종료) 앉음 표시가 지워진다.
#[test]
fn e206_3_paid_rest_seated_inside() {
    let m = mid_quick();
    let w = m.w();
    let cafe = w.space.places.iter().position(|p| p.rest_paid && !p.rest_seat && p.go_to).expect("카페") as u32;
    let mut b = quiet_walker(m);
    b.fatigue = 0.8;
    b.target = Target::Place(cafe);
    b.arrived = true;
    b.route = None;
    let sh = common::shared_state(w, &m.state);
    let c = common::dctx(w, &m.state.env, &sh, m.state.tick);
    let mut cafe_env = m.state.env.clone();
    cafe_env.open[cafe as usize] = true;
    let c = mdsim_runtime::behavior::DCtx { env: &cafe_env, ..c };
    let _ = mdsim_runtime::behavior::decide_domestic(&c, common::prof_of(w, &b), &mut b);
    assert_eq!(b.activity, Activity::Queue, "카페 줄에 섬");
    assert!(b.rest_seated, "유료 쉼터 안은 앉을 자리");
    let mut seated = b.clone();
    seated.activity = Activity::Inside;
    let mut standing = seated.clone();
    standing.rest_seated = false;
    for _ in 0..240 {
        passive_recovery(&mut seated, w.dt(), &w.p);
        passive_recovery(&mut standing, w.dt(), &w.p);
    }
    assert!(seated.fatigue < standing.fatigue - 1e-9, "앉아 {} vs 서서 {}", seated.fatigue, standing.fatigue);
    seated.act_place = Some(cafe);
    seated.act_until = m.state.tick;
    let _ = mdsim_runtime::behavior::decide_domestic(&c, common::prof_of(w, &seated), &mut seated);
    assert!(!seated.rest_seated, "나오면 앉음 표시를 지움");
}

/// 10분 프레임은 DR-17의 규약(KST 10분 눈금, [시작, 끝) 반열린 구간, 프레임 번호 = floor(t_s / 600))을 지킨다.
#[test]
fn e206_5_frame_convention() {
    let o = observed_quick();
    let w = o.prep.w;
    let acc = &o.state.acc;
    let (w0, _) = w.scn.window_s();
    let base = (w0 / 600.0).floor() as usize;
    for (k, fr) in acc.frames.iter().enumerate() {
        assert_eq!(fr.start_min, 10.0 * (base + k) as f64, "프레임 {k} 시작");
    }
    let n = o.snaps.len() as u64 - 1;
    for t in 0..n {
        let want = (w.t_s(t) / 600.0).floor() as usize - base;
        assert_eq!(acc.frame_ix(t, w.dt()), want, "틱 {t}");
    }
    // 반열린 구간: 14:10:00 정각의 틱은 다음 프레임
    let t1010 = ((600.0) / w.dt()) as u64;
    assert_eq!((acc.frame_ix(t1010 - 1, w.dt()), acc.frame_ix(t1010, w.dt())), (0, 1));
    // 도착 사건은 그 틱의 프레임에 세어진다
    let mut per: BTreeMap<usize, u32> = BTreeMap::new();
    for e in o.sink.events.iter().filter(|e| e.kind == "spawn") {
        *per.entry((e.t_s / 600.0).floor() as usize - base).or_default() += 1;
        assert_eq!(e.t_s, w.t_s(e.tick), "사건 시각 = 창 시작 + 틱 × Δt");
    }
    for (k, fr) in acc.frames.iter().enumerate() {
        assert_eq!(fr.arrivals, per.get(&k).copied().unwrap_or(0), "프레임 {k} 도착 수");
    }
    // 렌더 묶음도 10분 눈금 [s, s + 1200틱)
    for (i, (s, _)) in o.sink.chunk_hashes.iter().enumerate() {
        assert_eq!(*s, i as u64 * w.scn.ticks_per_chunk());
    }
}

/// 실행 창이 10분 눈금에 맞지 않으면(14:05~14:35) 시나리오가 거부되거나, 프레임이 여전히 10분 눈금(14:00·14:10…)에 맞는다.
#[test]
fn e206_5_frame_convention_unaligned_window() {
    let scn = with_window(scenario("quick"), "14:05", "14:35");
    if scn.validate().is_err() {
        return;
    }
    let pr = prep(scn);
    let mut sim = pr.sim();
    let mut sink = MemSink::default();
    sim.run_until(10, &mut sink).expect("run");
    let acc = &sim.state.acc;
    assert_eq!(acc.frames[0].start_min, 840.0, "첫 프레임은 14:00 눈금");
    let t1410 = (300.0 / pr.w.dt()) as u64; // 14:10:00
    assert_eq!((acc.frame_ix(t1410 - 1, pr.w.dt()), acc.frame_ix(t1410, pr.w.dt())), (0, 1), "14:10에서 다음 프레임");
}

/// 개체 생성·소멸은 규칙(도착 계획, 출구 도달)으로만 일어나고 사건으로 남는다.
#[test]
fn e206_6_spawn_despawn_logged() {
    let o = observed_quick();
    let w = o.prep.w;
    let n = o.snaps.len() as u64 - 1;
    let planned: BTreeMap<&str, &mdsim_runtime::visit::Spawn> = o.prep.spawns.iter().filter(|s| s.tick < n).map(|s| (s.entity.as_str(), s)).collect();
    assert_eq!(planned.len(), o.state.bodies.len(), "계획한 도착 = 생긴 몸");
    let mut spawn_ev: BTreeMap<i64, Vec<&EventRow>> = BTreeMap::new();
    let mut despawn_ev: BTreeMap<i64, Vec<&EventRow>> = BTreeMap::new();
    for e in &o.sink.events {
        match e.kind.as_str() {
            "spawn" => spawn_ev.entry(e.body).or_default().push(e),
            "despawn" => despawn_ev.entry(e.body).or_default().push(e),
            _ => {}
        }
    }
    for b in &o.state.bodies {
        let s = planned[b.entity.as_str()];
        assert_eq!((b.arrival_tick, b.portal_in), (s.tick, s.portal), "몸 {}: 계획대로 생김", b.id);
        let ev = &spawn_ev[&(b.id as i64)];
        assert_eq!(ev.len(), 1);
        assert_eq!((ev[0].tick, ev[0].info.as_str()), (b.arrival_tick, w.space.portals[b.portal_in as usize].id.as_str()));
        match (b.alive, despawn_ev.get(&(b.id as i64))) {
            (true, None) => {}
            (false, Some(d)) => {
                assert_eq!(d.len(), 1, "몸 {} 소멸 사건 하나", b.id);
                let po = &w.space.portals[b.portal_out.expect("나간 출입구") as usize];
                assert_eq!((d[0].tick, d[0].info.as_str()), (b.departed_tick.unwrap(), po.id.as_str()));
                assert!(b.pos.dist(po.pos) <= w.p.loco.arrive_m + 1e-9, "몸 {}: 출구에 닿아 사라짐", b.id);
            }
            (alive, d) => panic!("몸 {}: 살아 있음 {alive}, 소멸 사건 {:?}", b.id, d.map(|v| v.len())),
        }
    }
    // 틱마다 살아 있는 몸 수 = 그때까지 생성 − 소멸
    let mut alive = 0i64;
    for t in 0..n {
        alive += spawn_ev.values().flatten().filter(|e| e.tick == t).count() as i64;
        alive -= despawn_ev.values().flatten().filter(|e| e.tick == t).count() as i64;
        let seen = o.snaps[t as usize + 1].iter().filter(|s| s.alive).count() as i64;
        assert_eq!(seen, alive, "틱 {t} 살아 있는 몸");
    }
}
