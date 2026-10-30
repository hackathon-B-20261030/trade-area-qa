//! E224: 환경 동역학의 완료 기준(①~④. ⑤ 스냅샷의 시나리오 해시는 e225 쪽에서 본다).
mod common;

use common::{fx, scenario};
use mdsim_core::scenario::Scenario;
use mdsim_runtime::env::{env_at, env_changes};
use mdsim_runtime::space::PlaceKind;

#[test]
fn e224_1_pure_env() {
    let f = fx();
    let scn = scenario("variant");
    for t in [15.5 * 3600.0, 15.75 * 3600.0, 16.0 * 3600.0, 16.25 * 3600.0] {
        assert_eq!(env_at(&scn, &f.space, 0.6, t), env_at(&scn, &f.space, 0.6, t));
    }
}

#[test]
fn e224_2_one_event_per_change() {
    let f = fx();
    let scn = scenario("variant");
    let (a, b) = scn.window_s();
    let mut prev = env_at(&scn, &f.space, 0.6, a);
    let mut n_events = 0usize;
    let mut expected = 0usize;
    let mut t = a + scn.dt_s;
    while t < b {
        let next = env_at(&scn, &f.space, 0.6, t);
        let ch = env_changes(&prev, &next);
        let diff = (0..next.open.len()).filter(|&i| prev.open[i] != next.open[i]).count() + usize::from(prev.ped_only != next.ped_only) + usize::from(prev.vehicle_hours != next.vehicle_hours) + usize::from((prev.light < 0.999) != (next.light < 0.999));
        assert_eq!(ch.len(), diff);
        n_events += ch.len();
        expected += diff;
        prev = next;
        t += scn.dt_s;
    }
    // 16:00 노점 6곳 운영 시작, 15:45/16:15 행사, 16:00 보행자 전용 시작(차량 끝)
    let stalls = f.space.places.iter().filter(|p| p.kind == PlaceKind::Stall).count();
    assert_eq!(n_events, expected);
    assert!(n_events >= stalls + 3, "사건 {n_events}");
}

#[test]
fn e224_3_half_open() {
    let f = fx();
    let scn = scenario("base");
    // 고정 자료에서 15:00에 닫는 장소
    let ix = f.space.places.iter().position(|p| p.close_min == 900.0).expect("15시 마감 장소");
    assert!(env_at(&scn, &f.space, 0.6, 899.99 * 60.0).open[ix]);
    assert!(!env_at(&scn, &f.space, 0.6, 900.0 * 60.0).open[ix]);
    // 행사: [15:45, 16:15)
    let v = scenario("variant");
    let ev = f.space.places.iter().position(|p| p.kind == PlaceKind::Event).unwrap();
    assert!(!env_at(&v, &f.space, 0.6, (15.0 * 60.0 + 44.99) * 60.0).event_active[ev]);
    assert!(env_at(&v, &f.space, 0.6, (15.0 * 60.0 + 45.0) * 60.0).event_active[ev]);
    assert!(!env_at(&v, &f.space, 0.6, (16.0 * 60.0 + 15.0) * 60.0).event_active[ev]);
}

#[test]
fn e224_4_reject_out_of_range() {
    let base = scenario("base");
    let mut s = base.clone();
    s.headway_multiplier = -1.0;
    assert!(s.validate().is_err());
    let mut s = base.clone();
    s.events.push(mdsim_core::scenario::EventSpec { place_id: "V:0001".into(), start: "16:00".into(), end: "15:00".into() });
    assert!(s.validate().is_err());
    let dir = std::env::temp_dir().join(format!("mdsim-e224-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("bad.yaml");
    std::fs::write(&p, "id: x\ndate: 2026-13-40\n").unwrap();
    let err = Scenario::load(&p).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    let _ = std::fs::remove_dir_all(dir);
}
