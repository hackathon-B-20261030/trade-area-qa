//! E220 (Rust 쪽): 연속 공간의 완료 기준.
mod common;

use common::fx;
use mdsim_core::dims::Dim;
use mdsim_core::geom::V2;
use mdsim_runtime::env::env_at;
use mdsim_runtime::space::{PlaceKind, Space, BUILDING, CORE, CROSS};

#[test]
fn e220_1_edge_attrs_present() {
    let f = fx();
    assert_eq!(f.space.edges.len(), f.bundle.edges.len());
    for (e, row) in f.space.edges.iter().zip(&f.bundle.edges) {
        assert!(e.len > 0.0 && e.width > 0.0, "간선 {}", e.id);
        assert!(e.grade_ab.is_finite() && e.grade_smooth_ab.is_finite());
        assert!(!row.label_width.is_empty(), "폭 출처 라벨");
        assert_eq!(f.defs.classes[e.class_ix].id, row.class);
    }
}

#[test]
fn e220_2_portals_reachable() {
    let f = fx();
    for po in &f.space.portals {
        let seen = f.space.graph.reachable(po.node);
        for pl in &f.space.places {
            assert!(seen[pl.node_near as usize], "출입구 {}에서 장소 {}에 닿지 못함", po.id, pl.id);
        }
        for q in &f.space.portals {
            assert!(seen[q.node as usize]);
        }
    }
}

#[test]
fn e220_3_no_walk_in_buildings() {
    let f = fx();
    let env = env_at(&common::scenario("base"), &f.space, 0.6, 15.0 * 3600.0);
    let mut checked = 0;
    for r in (0..f.space.h).step_by(3) {
        for c in (0..f.space.w).step_by(3) {
            let flags = f.space.flags[r * f.space.w + c];
            if flags & BUILDING != 0 && flags & CORE == 0 {
                assert!(!f.space.walkable(f.space.cell_center(c, r), &env));
                checked += 1;
            }
        }
    }
    assert!(checked > 1000, "건물 칸 {checked}");
    for b in &f.bundle.buildings {
        let mut c = V2::ZERO;
        for p in &b.footprint {
            c = c + *p * (1.0 / b.footprint.len() as f64);
        }
        assert!(!f.space.walkable_static(c), "건물 {} 가운데", b.id);
    }
}

#[test]
fn e220_4_places_classified_or_excluded() {
    let f = fx();
    assert_eq!(f.space.places.len() + f.space.excluded_places.len(), f.bundle.places.len());
    for p in &f.space.places {
        let class = &f.defs.classes[p.class_ix].id;
        // 02c I-203-a의 v1 잎 클래스 → 런타임 장소 종류(affords로 정한다)
        let expect = match class.as_str() {
            "stall" => PlaceKind::Stall,
            "event" => PlaceKind::Event,
            "rest_facility" => PlaceKind::Bench,
            "landmark" => PlaceKind::Visit,
            "lodging" => PlaceKind::Lodging,
            "store" | "cafe" => PlaceKind::Shop,
            other => panic!("모르는 클래스 {other}"),
        };
        assert_eq!(p.kind, expect, "장소 {} ({class})", p.id);
        assert!(f.space.walkable_static(p.anchor), "장소 {} 앵커가 걷기 가능 칸", p.id);
    }
}

#[test]
fn e220_5_space_deterministic() {
    let f = fx();
    let again = Space::build(&f.defs, &f.bundle).unwrap();
    assert_eq!(again.flags, f.space.flags);
    for (a, b) in again.edges.iter().zip(&f.space.edges) {
        assert_eq!(a.demand, b.demand);
    }
    assert_eq!(again.places.iter().map(|p| p.anchor).collect::<Vec<_>>(), f.space.places.iter().map(|p| p.anchor).collect::<Vec<_>>());
}

#[test]
fn e220_6_coordinates_frame() {
    let f = fx();
    for (n, row) in f.space.graph.node_pos.iter().zip(&f.bundle.nodes) {
        assert_eq!(*n, row.pos, "노드 좌표는 계약 좌표 그대로");
    }
    for (p, row) in f.space.places.iter().zip(f.bundle.places.iter().filter(|r| !f.space.excluded_places.iter().any(|(id, _)| *id == r.id))) {
        assert_eq!(p.pos, row.pos);
        assert!(p.anchor.dist(row.anchor) <= 3.0 + 1e-9, "장소 {} 앵커 이동 {}", p.id, p.anchor.dist(row.anchor));
    }
    for (po, row) in f.space.portals.iter().zip(&f.bundle.portals) {
        assert!(po.pos.dist(row.pos) <= 5.0);
    }
    // 칸 중심 ↔ 좌표 왕복 오차
    let q = V2::new(12.34, 56.78);
    let (c, r) = f.space.cell_of(q);
    assert!(f.space.cell_center(c, r).dist(q) <= 0.5 * f.space.cell * 2f64.sqrt() + 1e-9);
}

#[test]
fn e220_7_steps_press() {
    let f = fx();
    let steps: Vec<_> = f.space.edges.iter().filter(|e| e.is_steps).collect();
    assert_eq!(steps.len(), 1);
    let s = steps[0];
    let mut env = env_at(&common::scenario("base"), &f.space, 0.6, 15.0 * 3600.0);
    env.vehicle_hours = false;
    let up = if s.grade_ab > 0.0 { s.demand_of(true, &env) } else { s.demand_of(false, &env) };
    let down = if s.grade_ab > 0.0 { s.demand_of(false, &env) } else { s.demand_of(true, &env) };
    assert!(up[Dim::Step.ix()] >= 15.0 && down[Dim::Step.ix()] >= 15.0, "계단 단차");
    assert!(up[Dim::Climb.ix()] > down[Dim::Climb.ix()]);
    assert!(down[Dim::Fall.ix()] > up[Dim::Fall.ix()]);
    for e in f.space.edges.iter().filter(|e| !e.is_steps) {
        assert_eq!(e.demand_of(true, &env)[Dim::Step.ix()], 0.0);
    }
}

#[test]
fn sidewalks_crossings_sight_and_stalls() {
    let f = fx();
    let scn = common::scenario("base");
    let env = env_at(&scn, &f.space, 0.6, 15.0 * 3600.0);
    // 간선도로(x=210) 차도는 걷지 못하고 보도(x≈198.5)는 걷는다
    assert!(!f.space.walkable(V2::new(210.0, 50.0), &env));
    assert!(f.space.walkable(V2::new(198.5, 50.0), &env));
    assert!(f.space.walkable(V2::new(221.5, 50.0), &env));
    // 횡단 노드(210,102)와 교차 노드(210,68)에서는 차도를 건넌다
    assert!(f.space.walkable(V2::new(210.0, 102.0), &env));
    assert_ne!(f.space.flag_at(V2::new(210.0, 68.0)) & CROSS, 0);
    // 시선: 길을 따라서는 보이고 건물을 가로지르면 막힌다
    assert!(f.space.line_of_sight(V2::new(10.0, 0.0), V2::new(60.0, 0.0)));
    assert!(!f.space.line_of_sight(V2::new(34.0, 0.0), V2::new(34.0, 68.0)));
    // 노점: 16시 전에는 그 칸을 걷고 운영 중에는 못 걷는다
    let stall = f.space.places.iter().position(|p| p.kind == PlaceKind::Stall).unwrap();
    let pos = f.space.places[stall].pos;
    let before = env_at(&scn, &f.space, 0.6, 15.5 * 3600.0);
    let during = env_at(&scn, &f.space, 0.6, 16.5 * 3600.0);
    assert!(f.space.walkable(pos, &before));
    assert!(!f.space.walkable(pos, &during));
    // 생활도로 차로(S-E27): 걷기는 늘 가능하고, 차가 다니는지는 lane_active_at으로 본다.
    // 보행 전용 구역(ped_zone)은 보행 전용 시간에 차가 없고, 그 밖 차량 혼재 간선은 차량 시간 내내 차가 다닌다.
    let lane = V2::new(34.0, 68.0);
    let mut veh = before.clone();
    veh.ped_only = false;
    assert!(before.ped_only, "기본 시나리오는 12~20시 보행 전용");
    assert!(f.space.walkable(lane, &before) && f.space.walkable(lane, &veh), "차로도 걸을 수 있다");
    assert!(!f.space.lane_active_at(lane, &before) && f.space.lane_active_at(lane, &veh));
    assert!(f.space.lane_active_at(V2::new(34.0, 204.0), &before), "ped_zone이 아닌 차로는 보행 전용 시간에도 차가 다닌다");
    let mut off = veh.clone();
    off.vehicle_hours = false;
    assert!(!f.space.lane_active_at(lane, &off), "차량 통행 시간 밖");
    // 교차점(차수 3 이상)에는 차로 표지가 없다
    assert!(!f.space.lane_active_at(V2::new(68.0, 68.0), &veh));
}
