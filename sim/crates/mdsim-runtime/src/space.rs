//! 연속 공간 (C-220의 Rust 쪽, 02d I-220-r, S-E5·S-E6).
//!
//! 0.5m 격자에 칸 표지를 찍어 걷기·가림 판정을 O(1)로 한다. 간선 정적 요구는 정의의 클래스 식으로
//! 방향별(a→b, b→a)·차량 통행 여부별로 미리 계산한다.

use crate::env::EnvState;
use crate::route::{EdgeIx, Graph, NodeIx};
use mdsim_core::bundle::{Bundle, Group, NodeKind, PortalKind, WalkMode};
use mdsim_core::defs::{AttrType, Defs};
use mdsim_core::scenario::DayType;
use mdsim_core::dims::{DimVec, IntVec};
use mdsim_core::geom::{cumulative, dist_point_segment, point_in_polygon, BBox, V2};
use mdsim_core::{Result, SimError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WALK: u8 = 1;
pub const SIDEWALK: u8 = 2;
pub const CROSS: u8 = 4;
pub const BUILDING: u8 = 8;
pub const STALL: u8 = 16;
pub const LANE: u8 = 32;
pub const CORE: u8 = 64;
/// 보행 전용 시간(#1)에 비는 차로(ped_zone 간선).
pub const LANE_PZ: u8 = 128;

pub const CELL_M: f64 = 0.5;
pub const STALL_RADIUS_M: f64 = 1.0;

#[derive(Clone, Debug)]
pub struct EdgeStatic {
    pub id: String,
    pub class_ix: usize,
    pub a: NodeIx,
    pub b: NodeIx,
    pub poly: Vec<V2>,
    pub cum: Vec<f64>,
    pub len: f64,
    pub width: f64,
    pub sidewalks: bool,
    pub sw: f64,
    pub vehicle_mixed: bool,
    /// 변수 #1(보행 전용 시간)의 대상 간선.
    pub ped_zone: bool,
    /// 횡단 간선: 차량이 늘 다닌다.
    pub is_crossing: bool,
    /// 차량 통행 중 잃는 보행 면적 비율.
    pub carriage_share: f64,
    pub is_steps: bool,
    /// 행동에 쓰는 경사(번들의 grade_pct, a→b 부호, %). 스무딩 값은 기록용.
    pub grade_ab: f64,
    pub grade_smooth_ab: f64,
    pub ad: f64,
    /// [방향(0: a→b, 1: b→a)][차량 통행(0: 없음, 1: 있음)]
    pub demand: [[DimVec; 2]; 2],
    /// 노점이 운영될 때 빠지는 면적(㎡).
    pub stall_area: f64,
    /// 유효 면적 하한(`segment.min_effective_area_m2`, 없으면 1㎡).
    pub min_area_m2: f64,
}

impl EdgeStatic {
    /// 지금 이 간선에 차량이 다니는가(런타임 변수 vehicle_active, 02c §2.3·§2.4 (3)).
    pub fn vehicle_now(&self, env: &EnvState) -> bool {
        self.is_crossing || (self.vehicle_mixed && env.vehicle_hours && !(self.ped_zone && env.ped_only))
    }

    /// 유효 보행 면적(㎡, 02c §2.3): 길이 × 보행 폭(full = width_m, sidewalks = 2 × sidewalk_width_m)
    /// × (1 − vehicle_active·carriage_share) − 열린 노점 점유, 하한 `segment.min_effective_area_m2`.
    pub fn walk_area(&self, env: &EnvState, stalls_active_area: f64) -> f64 {
        let w = if self.sidewalks { 2.0 * self.sw } else { self.width };
        let veh = if self.vehicle_now(env) && !self.sidewalks { self.carriage_share } else { 0.0 };
        (self.len * w * (1.0 - veh) - stalls_active_area).max(self.min_area_m2)
    }

    pub fn grade(&self, forward: bool) -> f64 {
        if forward {
            self.grade_ab
        } else {
            -self.grade_ab
        }
    }

    pub fn demand_of(&self, forward: bool, env: &EnvState) -> &DimVec {
        &self.demand[if forward { 0 } else { 1 }][if self.vehicle_now(env) { 1 } else { 0 }]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaceKind {
    Shop,
    Visit,
    Bench,
    Stall,
    Event,
    /// 숙박: 목적지가 아니라 떠나는 곳(affords LEAVE).
    Lodging,
}

#[derive(Clone, Debug)]
pub struct PlaceInst {
    pub id: String,
    pub class_ix: usize,
    pub kind: PlaceKind,
    pub category: String,
    pub name: String,
    pub pos: V2,
    pub anchor: V2,
    pub edge: EdgeIx,
    pub node_near: NodeIx,
    pub offer: IntVec,
    pub demand: DimVec,
    pub base_price: f64,
    pub discount: f64,
    /// 실제 가격 = 가격 × (1 − 할인율)
    pub price: f64,
    pub open_min: f64,
    pub close_min: f64,
    /// 존재·영업하는 하루 유형(비면 모든 날).
    pub day_types: Vec<DayType>,
    pub visibility: f64,
    pub fame: f64,
    pub service_rate: f64,
    /// 동시 수용(널이면 제한 없음 = u32::MAX).
    pub capacity: u32,
    pub can_buy: bool,
    /// 쉴 수 있다(REST_SEAT 또는 REST_PAID).
    pub can_rest: bool,
    /// 무료로 앉아 쉴 수 있다(affords REST_SEAT: 벤치·휴식 시설).
    pub rest_seat: bool,
    /// 돈을 내고 앉아 쉴 수 있다(affords REST_PAID: 카페, S-E53).
    pub rest_paid: bool,
    /// NEXT의 GO_TO 대상이 될 수 있는가(affords GO_TO).
    pub go_to: bool,
    pub foreign_label: bool,
    pub ad: f64,
}

#[derive(Clone, Debug)]
pub struct PortalInst {
    pub id: String,
    pub kind: PortalKind,
    pub pos: V2,
    pub node: NodeIx,
    pub name: String,
    pub headway_min: f64,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug)]
pub struct Space {
    pub origin: V2,
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    pub flags: Vec<u8>,
    pub stall_of_cell: BTreeMap<u32, u32>,
    pub graph: Graph,
    pub node_kind: Vec<NodeKind>,
    pub node_signal: Vec<bool>,
    pub node_id: Vec<String>,
    pub edges: Vec<EdgeStatic>,
    pub places: Vec<PlaceInst>,
    pub portals: Vec<PortalInst>,
    pub buildings: Vec<Vec<V2>>,
    pub excluded_places: Vec<(String, String)>,
    pub bbox: BBox,
    place_grid_cell: f64,
    place_grid_w: usize,
    place_grid_h: usize,
    place_grid: Vec<Vec<u32>>,
    pub lane_w: f64,
    /// 차량 혼재 간선의 차량 통행 시간 [시작, 끝) 분(`segment.vehicle_hours_default`, 없으면 하루 종일).
    pub vehicle_hours: Vec<(f64, f64)>,
    /// 갈 수 있는 장소(GO_TO)의 업종별 평균 제공(EXPLORE의 기대 제공).
    pub mean_offer: IntVec,
}

struct Raster<'a> {
    origin: V2,
    cell: f64,
    w: usize,
    h: usize,
    flags: &'a mut Vec<u8>,
}

impl Raster<'_> {
    fn stamp_disk(&mut self, c: V2, r: f64, bits: u8) {
        self.stamp_where(c - V2::new(r, r), c + V2::new(r, r), |p| p.dist(c) <= r, bits);
    }

    fn clear_disk(&mut self, c: V2, r: f64, bits: u8) {
        let (lo, hi) = (c - V2::new(r, r), c + V2::new(r, r));
        let c0 = (((lo.x - self.origin.x) / self.cell).floor().max(0.0)) as usize;
        let r0 = (((lo.z - self.origin.z) / self.cell).floor().max(0.0)) as usize;
        let c1 = ((((hi.x - self.origin.x) / self.cell).ceil()) as isize).clamp(0, self.w as isize) as usize;
        let r1 = ((((hi.z - self.origin.z) / self.cell).ceil()) as isize).clamp(0, self.h as isize) as usize;
        for row in r0..r1 {
            for col in c0..c1 {
                let p = V2::new(self.origin.x + (col as f64 + 0.5) * self.cell, self.origin.z + (row as f64 + 0.5) * self.cell);
                if p.dist(c) <= r {
                    self.flags[row * self.w + col] &= !bits;
                }
            }
        }
    }

    fn stamp_where(&mut self, lo: V2, hi: V2, f: impl Fn(V2) -> bool, bits: u8) {
        let c0 = (((lo.x - self.origin.x) / self.cell).floor().max(0.0)) as usize;
        let r0 = (((lo.z - self.origin.z) / self.cell).floor().max(0.0)) as usize;
        let c1 = ((((hi.x - self.origin.x) / self.cell).ceil()) as isize).clamp(0, self.w as isize) as usize;
        let r1 = ((((hi.z - self.origin.z) / self.cell).ceil()) as isize).clamp(0, self.h as isize) as usize;
        for r in r0..r1 {
            for c in c0..c1 {
                let p = V2::new(self.origin.x + (c as f64 + 0.5) * self.cell, self.origin.z + (r as f64 + 0.5) * self.cell);
                if f(p) {
                    self.flags[r * self.w + c] |= bits;
                }
            }
        }
    }
}

/// 번들 행의 같은 이름 열로 클래스 식을 평가한다(02c §2.4: 표에 없는 속성은 클래스 default).
/// 범주 속성은 values 안의 인덱스, 방향 속성은 `reverse`면 부호를 바꾼다.
pub fn eval_row(defs: &Defs, class_ix: usize, nums: &BTreeMap<String, f64>, cats: &BTreeMap<String, String>, reverse: bool, rt: &dyn Fn(&str) -> Option<f64>) -> std::result::Result<(DimVec, IntVec), String> {
    let rc = &defs.classes[class_ix];
    let attr = |n: &str| -> Option<f64> {
        let a = rc.attrs.get(n)?;
        match &a.ty {
            AttrType::Categorical(_) => cats.get(n).and_then(|v| defs.categorical_index(class_ix, n, v)),
            _ => nums.get(n).map(|v| if reverse && a.directional { -v } else { *v }),
        }
    };
    defs.eval_instance(class_ix, &attr, rt)
}

impl Space {
    pub fn build(defs: &Defs, b: &Bundle) -> Result<Space> {
        let p = &defs.params;
        let lane_w = p.f("loco.lane_width_m")?;
        let core_w = p.f("loco.core_strip_m")?;
        // 그래프
        let node_ix: BTreeMap<&str, NodeIx> = b.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i as NodeIx)).collect();
        let node_pos: Vec<V2> = b.nodes.iter().map(|n| n.pos).collect();
        let gedges: Vec<(NodeIx, NodeIx, f64)> = b.edges.iter().map(|e| (node_ix[e.a.as_str()], node_ix[e.b.as_str()], e.length_m)).collect();
        let graph = Graph::new(node_pos, &gedges);
        // 범위
        let mut bbox = BBox::empty();
        for n in &b.nodes {
            bbox.add(n.pos);
        }
        for e in &b.edges {
            for q in &e.polyline {
                bbox.add(*q + V2::new(e.width_m, e.width_m));
                bbox.add(*q - V2::new(e.width_m, e.width_m));
            }
        }
        for bl in &b.buildings {
            for q in &bl.footprint {
                bbox.add(*q);
            }
        }
        let bbox = bbox.grow(20.0);
        let origin = bbox.min;
        let w = ((bbox.max.x - bbox.min.x) / CELL_M).ceil() as usize;
        let h = ((bbox.max.z - bbox.min.z) / CELL_M).ceil() as usize;
        if w == 0 || h == 0 || w * h > 60_000_000 {
            return Err(SimError::Invariant(format!("보행 격자 크기 {w}×{h}가 범위 밖")));
        }
        let mut flags = vec![0u8; w * h];
        {
            let mut ras = Raster { origin, cell: CELL_M, w, h, flags: &mut flags };
            for e in &b.edges {
                if e.walk_mode == WalkMode::NoWalk {
                    continue;
                }
                for seg in e.polyline.windows(2) {
                    let (p0, p1) = (seg[0], seg[1]);
                    let half = e.width_m / 2.0;
                    let lo = V2::new(p0.x.min(p1.x) - half, p0.z.min(p1.z) - half);
                    let hi = V2::new(p0.x.max(p1.x) + half, p0.z.max(p1.z) + half);
                    match e.walk_mode {
                        WalkMode::Full => {
                            ras.stamp_where(lo, hi, |q| dist_point_segment(q, p0, p1).0 <= half, WALK);
                            ras.stamp_where(lo, hi, |q| dist_point_segment(q, p0, p1).0 <= core_w / 2.0, CORE);
                            // 차량과 폭을 나눠 쓰는 간선만 차로를 표시한다. 횡단 간선(is_crossing)은 걷는 곳이고
                            // vehicle_active = 1은 D-SAFE 노출에만 쓴다.
                            if e.vehicle_mixed && !e.is_crossing {
                                let bit = if e.ped_zone { LANE_PZ } else { LANE };
                                ras.stamp_where(lo, hi, |q| dist_point_segment(q, p0, p1).0 <= lane_w / 2.0, bit);
                            }
                        }
                        WalkMode::Sidewalks => {
                            let inner = half - e.sidewalk_width_m;
                            ras.stamp_where(
                                lo,
                                hi,
                                |q| {
                                    let d = dist_point_segment(q, p0, p1).0;
                                    d >= inner && d <= half
                                },
                                WALK | SIDEWALK,
                            );
                        }
                        WalkMode::NoWalk => {}
                    }
                }
            }
            // 보도 간선에 닿은 횡단·교차 노드의 횡단 구역(DR-222)
            for (ni, n) in b.nodes.iter().enumerate() {
                if !matches!(n.kind, NodeKind::Crossing | NodeKind::Junction) {
                    continue;
                }
                let mut r = 0.0f64;
                for &(e, _) in graph.neighbors(ni as NodeIx) {
                    let er = &b.edges[e as usize];
                    if er.walk_mode == WalkMode::Sidewalks {
                        r = r.max(er.width_m / 2.0);
                    }
                }
                if r > 0.0 {
                    ras.stamp_disk(n.pos, r, WALK | CROSS);
                }
            }
            // 차량 혼재 간선(full 폭 생활도로)의 차로는 차수 3 이상 교차점에서 건널 수 있다(S-E27).
            // 간선도로(sidewalks)의 차도는 처음부터 걷기 칸이 아니라 DR-222(횡단 구역에서만 건넘)와 무관하다.
            for (ni, n) in b.nodes.iter().enumerate() {
                let inc = graph.neighbors(ni as NodeIx);
                if inc.len() < 3 {
                    continue;
                }
                let r = inc.iter().map(|&(e, _)| b.edges[e as usize].width_m / 2.0).fold(0.0, f64::max);
                ras.clear_disk(n.pos, r, LANE | LANE_PZ);
            }
            for bl in &b.buildings {
                let mut bb = BBox::empty();
                for q in &bl.footprint {
                    bb.add(*q);
                }
                let fp = &bl.footprint;
                ras.stamp_where(bb.min, bb.max, |q| point_in_polygon(q, fp), BUILDING);
            }
        }
        // 간선 정적 요구: 방향별·차량 통행 여부별(02c §2.3, 같은 이름 열 규칙)
        let mut edges = Vec::with_capacity(b.edges.len());
        for (i, e) in b.edges.iter().enumerate() {
            let class_ix = *defs.class_ix.get(&e.class).ok_or_else(|| SimError::contract("edges", format!("간선 {}의 class '{}'가 정의에 없음", e.id, e.class)))?;
            let mut demand = [[[0.0; mdsim_core::dims::ND]; 2]; 2];
            for (di, reverse) in [(0usize, false), (1, true)] {
                for (vi, veh) in [(0usize, 0.0f64), (1, 1.0)] {
                    let rt = |n: &str| -> Option<f64> {
                        Some(match n {
                            "vehicle_active" => veh,
                            "stall_active" | "event_active" | "open" | "light" | "ad_multiplier" => 1.0,
                            _ => return None,
                        })
                    };
                    let (press, _) = eval_row(defs, class_ix, &e.attrs, &e.cats, reverse, &rt).map_err(|m| SimError::contract("edges", format!("간선 {}: {m}", e.id)))?;
                    demand[di][vi] = press;
                }
            }
            let poly = e.polyline.clone();
            let cum = cumulative(&poly);
            edges.push(EdgeStatic {
                id: e.id.clone(),
                class_ix,
                a: gedges[i].0,
                b: gedges[i].1,
                len: *cum.last().unwrap(),
                poly,
                cum,
                width: e.width_m,
                sidewalks: e.walk_mode == WalkMode::Sidewalks,
                sw: e.sidewalk_width_m,
                vehicle_mixed: e.vehicle_mixed,
                ped_zone: e.ped_zone,
                is_crossing: e.is_crossing,
                carriage_share: e.carriage_share,
                is_steps: e.is_steps,
                grade_ab: e.grade_pct,
                grade_smooth_ab: e.grade_pct_smooth,
                ad: e.ad_intensity,
                demand,
                stall_area: 0.0,
                min_area_m2: p.f("segment.min_effective_area_m2").unwrap_or(1.0).max(1e-3),
            });
        }
        let edge_ix: BTreeMap<&str, EdgeIx> = b.edges.iter().enumerate().map(|(i, e)| (e.id.as_str(), i as EdgeIx)).collect();
        let vh = p.v("segment.vehicle_hours_default").unwrap_or_default();
        let vehicle_hours: Vec<(f64, f64)> = vh.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        let mut space = Space {
            origin,
            cell: CELL_M,
            w,
            h,
            flags,
            stall_of_cell: BTreeMap::new(),
            graph,
            node_kind: b.nodes.iter().map(|n| n.kind).collect(),
            node_signal: b.nodes.iter().map(|n| n.signal).collect(),
            node_id: b.nodes.iter().map(|n| n.id.clone()).collect(),
            edges,
            places: Vec::new(),
            portals: Vec::new(),
            buildings: b.buildings.iter().map(|bl| bl.footprint.clone()).collect(),
            excluded_places: Vec::new(),
            bbox,
            place_grid_cell: 10.0,
            place_grid_w: 0,
            place_grid_h: 0,
            place_grid: Vec::new(),
            lane_w,
            vehicle_hours,
            mean_offer: [0.0; mdsim_core::dims::NI],
        };
        // 장소
        for pr in &b.places {
            let class_ix = *defs.class_ix.get(&pr.class).ok_or_else(|| SimError::contract("places", format!("장소 {}의 class '{}'가 정의에 없음", pr.id, pr.class)))?;
            let rc = &defs.classes[class_ix];
            let rt = |n: &str| -> Option<f64> {
                Some(match n {
                    "vehicle_active" => 0.0,
                    "stall_active" | "event_active" | "open" | "light" | "ad_multiplier" => 1.0,
                    _ => return None,
                })
            };
            let (demand, offer) = eval_row(defs, class_ix, &pr.attrs, &pr.cats, false, &rt).map_err(|m| SimError::contract("places", format!("장소 {}: {m}", pr.id)))?;
            let can_buy = rc.affords("BUY");
            let rest_seat = rc.affords("REST_SEAT");
            let rest_paid = rc.affords("REST_PAID");
            let can_rest = rest_seat || rest_paid;
            let go_to = rc.affords("GO_TO");
            let occupies = pr.footprint_m2.unwrap_or(0.0) > 0.0 || rc.attrs.contains_key("footprint_m2");
            let kind = if rc.affords("WATCH") {
                PlaceKind::Event
            } else if occupies {
                PlaceKind::Stall
            } else if rc.affords("REST_SEAT") && !go_to {
                PlaceKind::Bench
            } else if rc.affords("LEAVE") && !go_to {
                PlaceKind::Lodging
            } else if can_buy {
                PlaceKind::Shop
            } else {
                PlaceKind::Visit
            };
            let edge = *edge_ix.get(pr.edge_id.as_str()).ok_or_else(|| SimError::contract("places", format!("장소 {}의 edge_id {}가 없음", pr.id, pr.edge_id)))?;
            let anchor = if kind == PlaceKind::Stall {
                // 노점은 길 위에 놓인다. 손님은 노점 칸 바로 옆에 선다.
                space.snap_static(pr.anchor, 0.0).unwrap_or(pr.anchor)
            } else {
                match space.snap_static(pr.anchor, 3.0) {
                    Some(a) => a,
                    None => {
                        space.excluded_places.push((pr.id.clone(), "앵커가 3m 안에서 걷기 가능 칸에 닿지 않음".into()));
                        continue;
                    }
                }
            };
            let es = &space.edges[edge as usize];
            let (pa, pb) = (space.graph.node_pos[es.a as usize], space.graph.node_pos[es.b as usize]);
            // 간선 위 위치(02c B5): offset이 있으면 그 값으로 가까운 끝 노드를 고른다.
            let node_near = match pr.offset_m {
                Some(off) if es.len > 0.0 => {
                    if off <= es.len / 2.0 {
                        es.a
                    } else {
                        es.b
                    }
                }
                _ => {
                    if anchor.dist(pa) <= anchor.dist(pb) {
                        es.a
                    } else {
                        es.b
                    }
                }
            };
            let ad = es.ad;
            let capacity = match (kind, pr.capacity, pr.seats) {
                (PlaceKind::Bench, _, Some(s)) => s.round().max(1.0) as u32,
                (_, Some(c), _) => c.round().max(0.0) as u32,
                _ => u32::MAX,
            };
            space.places.push(PlaceInst {
                id: pr.id.clone(),
                class_ix,
                kind,
                category: pr.category.clone(),
                name: pr.name.clone(),
                pos: pr.pos,
                anchor,
                edge,
                node_near,
                offer,
                demand,
                base_price: pr.price_level_krw,
                discount: pr.discount_pct,
                price: pr.price_level_krw * (1.0 - pr.discount_pct / 100.0),
                open_min: pr.open_min,
                close_min: pr.close_min,
                day_types: pr.day_types.clone(),
                visibility: pr.visibility,
                fame: pr.fame,
                service_rate: pr.service_rate_per_min,
                capacity,
                can_buy,
                can_rest,
                rest_seat,
                rest_paid,
                go_to,
                foreign_label: pr.foreign_label,
                ad,
            });
        }
        // 노점 칸과 간선의 노점 면적
        let stall_ix: Vec<usize> = space.places.iter().enumerate().filter(|(_, p)| p.kind == PlaceKind::Stall).map(|(i, _)| i).collect();
        for &pi in &stall_ix {
            let c = space.places[pi].pos;
            let r = STALL_RADIUS_M;
            let (c0, r0) = space.cell_of(c - V2::new(r, r));
            let (c1, r1) = space.cell_of(c + V2::new(r, r));
            for rr in r0..=r1 {
                for cc in c0..=c1 {
                    if rr >= space.h || cc >= space.w {
                        continue;
                    }
                    let q = space.cell_center(cc, rr);
                    if q.dist(c) <= r {
                        let k = rr * space.w + cc;
                        space.flags[k] |= STALL;
                        space.stall_of_cell.insert(k as u32, pi as u32);
                    }
                }
            }
            let e = space.places[pi].edge as usize;
            let foot = b.places.iter().find(|p| p.id == space.places[pi].id).and_then(|p| p.footprint_m2).unwrap_or(std::f64::consts::PI * r * r);
            space.edges[e].stall_area += foot;
            // 손님 자리: 노점 옆 걷기 가능 칸
            let a = space.places[pi].anchor;
            let side = space.snap_static_avoiding_stall(a, 3.0).unwrap_or(a);
            space.places[pi].anchor = side;
        }
        // 출입구
        for po in &b.portals {
            let pos = space.snap_static(po.pos, 5.0).ok_or_else(|| SimError::contract("portals", format!("출입구 {}가 5m 안에서 걷기 가능 칸에 닿지 않음", po.id)))?;
            let node = *node_ix.get(po.node_id.as_str()).ok_or_else(|| SimError::contract("portals", format!("출입구 {}의 node_id {}가 없음", po.id, po.node_id)))?;
            space.portals.push(PortalInst { id: po.id.clone(), kind: po.kind, pos, node, name: po.name.clone(), headway_min: po.headway_min, groups: po.groups.clone() });
        }
        let goto: Vec<&PlaceInst> = space.places.iter().filter(|p| p.go_to).collect();
        if !goto.is_empty() {
            for k in 0..mdsim_core::dims::NI {
                space.mean_offer[k] = goto.iter().map(|p| p.offer[k]).sum::<f64>() / goto.len() as f64;
            }
        }
        // 장소 격자
        let gc = space.place_grid_cell;
        space.place_grid_w = ((space.bbox.max.x - space.bbox.min.x) / gc).ceil() as usize + 1;
        space.place_grid_h = ((space.bbox.max.z - space.bbox.min.z) / gc).ceil() as usize + 1;
        space.place_grid = vec![Vec::new(); space.place_grid_w * space.place_grid_h];
        for (i, pl) in space.places.iter().enumerate() {
            let gx = ((pl.anchor.x - space.bbox.min.x) / gc).floor().max(0.0) as usize;
            let gz = ((pl.anchor.z - space.bbox.min.z) / gc).floor().max(0.0) as usize;
            let k = gz.min(space.place_grid_h - 1) * space.place_grid_w + gx.min(space.place_grid_w - 1);
            space.place_grid[k].push(i as u32);
        }
        Ok(space)
    }

    pub fn cell_of(&self, p: V2) -> (usize, usize) {
        let c = ((p.x - self.origin.x) / self.cell).floor();
        let r = ((p.z - self.origin.z) / self.cell).floor();
        (c.max(0.0) as usize, r.max(0.0) as usize)
    }

    pub fn cell_center(&self, c: usize, r: usize) -> V2 {
        V2::new(self.origin.x + (c as f64 + 0.5) * self.cell, self.origin.z + (r as f64 + 0.5) * self.cell)
    }

    fn cell_index(&self, p: V2) -> Option<usize> {
        let c = ((p.x - self.origin.x) / self.cell).floor();
        let r = ((p.z - self.origin.z) / self.cell).floor();
        if c < 0.0 || r < 0.0 || c >= self.w as f64 || r >= self.h as f64 {
            return None;
        }
        Some(r as usize * self.w + c as usize)
    }

    /// 걷기 판정. 차량 혼재 간선의 차로는 막지 않는다: 이면도로는 차 사이로 걷고 건넌다.
    /// 차로는 이동 방향을 고를 때의 감점(`lane_active_at`)과 유효 면적(carriage_share)으로만 들어간다(S-E27).
    fn walkable_flags(&self, k: usize, _veh: (bool, bool), stall_active: Option<&[bool]>) -> bool {
        let f = self.flags[k];
        if f & WALK == 0 {
            return false;
        }
        if f & BUILDING != 0 && f & CORE == 0 {
            return false;
        }
        if f & STALL != 0 {
            match stall_active {
                Some(sa) => {
                    if let Some(&pi) = self.stall_of_cell.get(&(k as u32)) {
                        if sa.get(pi as usize).copied().unwrap_or(false) {
                            return false;
                        }
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 지금 걸을 수 있는가(C-223 ①).
    pub fn walkable(&self, p: V2, env: &EnvState) -> bool {
        match self.cell_index(p) {
            Some(k) => self.walkable_flags(k, (env.vehicle_hours, env.ped_only), Some(&env.stall_active)),
            None => false,
        }
    }

    /// 동적 층을 끈 걷기 판정(앵커·출입구 붙이기).
    pub fn walkable_static(&self, p: V2) -> bool {
        match self.cell_index(p) {
            Some(k) => self.walkable_flags(k, (false, false), Some(&[])),
            None => false,
        }
    }

    /// 지금 차량이 다니는 차로 칸인가(보행 전용 시간의 ped_zone 차로는 아님).
    pub fn lane_active_at(&self, p: V2, env: &EnvState) -> bool {
        match self.cell_index(p) {
            Some(k) => {
                let f = self.flags[k];
                env.vehicle_hours && (f & LANE != 0 || (f & LANE_PZ != 0 && !env.ped_only))
            }
            None => false,
        }
    }

    pub fn flag_at(&self, p: V2) -> u8 {
        self.cell_index(p).map(|k| self.flags[k]).unwrap_or(0)
    }

    /// 시선을 막는 칸(건물, 단 중심선 통로 제외).
    pub fn blocks_sight(&self, p: V2) -> bool {
        match self.cell_index(p) {
            Some(k) => self.flags[k] & BUILDING != 0 && self.flags[k] & CORE == 0,
            None => false,
        }
    }

    /// from에서 dir 방향으로 걷기 불가 칸까지의 거리(최대 max).
    pub fn free_distance(&self, from: V2, dir: V2, max: f64, env: &EnvState) -> f64 {
        let step = self.cell * 0.5;
        let mut s = step;
        while s <= max {
            if !self.walkable(from + dir * s, env) {
                return (s - step).max(0.0);
            }
            s += step;
        }
        max
    }

    /// 두 점 사이 시선이 건물에 막히지 않는가. 양 끝 0.75m는 보지 않는다.
    pub fn line_of_sight(&self, a: V2, b: V2) -> bool {
        let d = a.dist(b);
        if d < 1.5 {
            return true;
        }
        let dir = (b - a).norm();
        let step = self.cell * 0.5;
        let mut s = 0.75;
        while s < d - 0.75 {
            if self.blocks_sight(a + dir * s) {
                return false;
            }
            s += step;
        }
        true
    }

    fn snap_with(&self, p: V2, max_r: f64, ok: impl Fn(usize) -> bool) -> Option<V2> {
        if let Some(k) = self.cell_index(p) {
            if ok(k) {
                return Some(p);
            }
        }
        let (pc, pr) = self.cell_of(p);
        let rmax = (max_r / self.cell).ceil() as isize;
        let mut best: Option<(f64, usize, V2)> = None;
        for dr in -rmax..=rmax {
            for dc in -rmax..=rmax {
                let (c, r) = (pc as isize + dc, pr as isize + dr);
                if c < 0 || r < 0 || c >= self.w as isize || r >= self.h as isize {
                    continue;
                }
                let k = r as usize * self.w + c as usize;
                if !ok(k) {
                    continue;
                }
                let q = self.cell_center(c as usize, r as usize);
                let d = q.dist(p);
                if d <= max_r + 1e-9 && best.map(|(bd, bk, _)| d < bd || (d == bd && k < bk)).unwrap_or(true) {
                    best = Some((d, k, q));
                }
            }
        }
        best.map(|(_, _, q)| q)
    }

    pub fn snap_static(&self, p: V2, max_r: f64) -> Option<V2> {
        self.snap_with(p, max_r, |k| self.walkable_flags(k, (false, false), Some(&[])))
    }

    fn snap_static_avoiding_stall(&self, p: V2, max_r: f64) -> Option<V2> {
        self.snap_with(p, max_r, |k| self.flags[k] & STALL == 0 && self.walkable_flags(k, (true, false), Some(&[])))
    }

    /// 지금 걸을 수 있는 가장 가까운 점.
    pub fn snap(&self, p: V2, max_r: f64, env: &EnvState) -> Option<V2> {
        self.snap_with(p, max_r, |k| self.walkable_flags(k, (env.vehicle_hours, env.ped_only), Some(&env.stall_active)))
    }

    /// 막힌 몸의 국소 경유점: from에서 to까지 걷기 칸 위 너비 우선 탐색(8방향, max_r 안)을 하고
    /// 경로를 따라 step_m 앞의 점을 돌려준다. 칸 순서가 고정이라 결정적이다.
    pub fn local_waypoint(&self, from: V2, to: V2, max_r: f64, step_m: f64, env: &EnvState) -> Option<V2> {
        let (fc, fr) = self.cell_of(from);
        let (tc, tr) = self.cell_of(to);
        let rad = (max_r / self.cell).ceil() as isize;
        let (c0, r0) = (fc as isize - rad, fr as isize - rad);
        let side = (2 * rad + 1) as usize;
        let idx = |c: isize, r: isize| -> Option<usize> {
            let (lc, lr) = (c - c0, r - r0);
            if lc < 0 || lr < 0 || lc >= side as isize || lr >= side as isize || c < 0 || r < 0 || c >= self.w as isize || r >= self.h as isize {
                None
            } else {
                Some(lr as usize * side + lc as usize)
            }
        };
        let mut prev = vec![u32::MAX; side * side];
        let start = idx(fc as isize, fr as isize)?;
        let goal_local = idx(tc as isize, tr as isize);
        let mut queue = std::collections::VecDeque::new();
        prev[start] = start as u32;
        queue.push_back((fc as isize, fr as isize));
        let mut found: Option<(isize, isize)> = None;
        // 목표 칸이 반경 밖이면 목표에 가장 가까운 도달 칸으로 간다
        let mut best: (f64, isize, isize) = (from.dist(to), fc as isize, fr as isize);
        while let Some((c, r)) = queue.pop_front() {
            if Some(idx(c, r).unwrap()) == goal_local {
                found = Some((c, r));
                break;
            }
            let here = self.cell_center(c as usize, r as usize);
            let d = here.dist(to);
            if d < best.0 {
                best = (d, c, r);
            }
            for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (nc, nr) = (c + dc, r + dr);
                let Some(k) = idx(nc, nr) else { continue };
                if prev[k] != u32::MAX {
                    continue;
                }
                let q = self.cell_center(nc as usize, nr as usize);
                if !self.walkable(q, env) {
                    continue;
                }
                // 대각선은 양옆 칸도 걸을 수 있을 때만
                if dc != 0 && dr != 0 && (!self.walkable(self.cell_center((c + dc) as usize, r as usize), env) || !self.walkable(self.cell_center(c as usize, (r + dr) as usize), env)) {
                    continue;
                }
                prev[k] = idx(c, r).unwrap() as u32;
                queue.push_back((nc, nr));
            }
        }
        let (gc, gr) = found.unwrap_or((best.1, best.2));
        // 목표 칸에서 거꾸로 따라가 출발 칸 쪽 경로를 만든다
        let mut path = vec![(gc, gr)];
        let mut cur = idx(gc, gr)?;
        while cur != start {
            let p = prev[cur] as usize;
            let (lc, lr) = ((p % side) as isize, (p / side) as isize);
            path.push((lc + c0, lr + r0));
            cur = p;
        }
        path.reverse();
        let mut acc = 0.0;
        let mut last = from;
        for &(c, r) in path.iter().skip(1) {
            let q = self.cell_center(c as usize, r as usize);
            acc += q.dist(last);
            last = q;
            if acc >= step_m {
                return Some(q);
            }
        }
        if path.len() > 1 {
            Some(last)
        } else {
            None
        }
    }

    /// 반경 r 안 앵커를 가진 장소(격자 순서 → 결정적).
    pub fn places_near(&self, p: V2, r: f64, out: &mut Vec<u32>) {
        out.clear();
        let gc = self.place_grid_cell;
        let x0 = (((p.x - r - self.bbox.min.x) / gc).floor().max(0.0)) as usize;
        let z0 = (((p.z - r - self.bbox.min.z) / gc).floor().max(0.0)) as usize;
        let x1 = ((((p.x + r - self.bbox.min.x) / gc).floor()) as isize).clamp(0, self.place_grid_w as isize - 1) as usize;
        let z1 = ((((p.z + r - self.bbox.min.z) / gc).floor()) as isize).clamp(0, self.place_grid_h as isize - 1) as usize;
        for gz in z0..=z1.max(z0) {
            for gx in x0..=x1.max(x0) {
                if gz >= self.place_grid_h || gx >= self.place_grid_w {
                    continue;
                }
                for &i in &self.place_grid[gz * self.place_grid_w + gx] {
                    if self.places[i as usize].anchor.dist(p) <= r {
                        out.push(i);
                    }
                }
            }
        }
        out.sort_unstable();
    }

    /// 사람 걸음 기준 간선 통과 시간(초)과 횡단 대기를 더한 경로 비용.
    pub fn walk_cost(&self, e: EdgeIx, speed: f64, crossing_wait_s: f64) -> f64 {
        let es = &self.edges[e as usize];
        let wait = if self.node_signal[es.a as usize] || self.node_signal[es.b as usize] { crossing_wait_s * 0.5 } else { 0.0 };
        es.len / speed.max(0.1) + wait
    }

    pub fn portal_ix(&self, id: &str) -> Option<usize> {
        self.portals.iter().position(|p| p.id == id)
    }

    pub fn place_ix(&self, id: &str) -> Option<usize> {
        self.places.iter().position(|p| p.id == id)
    }

    /// 걷기 격자를 n칸마다 내려 뽑은 그림용 자료(0 없음, 1 걷기, 2 보도·횡단).
    pub fn raster_view(&self, step: usize) -> (V2, f64, usize, usize, Vec<u8>) {
        let step = step.max(1);
        let (w, h) = (self.w.div_ceil(step), self.h.div_ceil(step));
        let mut out = vec![0u8; w * h];
        for r in 0..h {
            for c in 0..w {
                let f = self.flags[(r * step).min(self.h - 1) * self.w + (c * step).min(self.w - 1)];
                out[r * w + c] = if f & WALK == 0 || (f & BUILDING != 0 && f & CORE == 0) { 0 } else if f & (SIDEWALK | CROSS) != 0 { 2 } else { 1 };
            }
        }
        (self.origin, self.cell * step as f64, w, h, out)
    }
}
