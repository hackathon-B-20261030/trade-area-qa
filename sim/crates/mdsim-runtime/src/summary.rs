//! plot용 요약 누적 (02d I-228-a). 정본 지표(C-212)는 Python 후처리가 기록에서 다시 계산한다.

use crate::env::EnvState;
use crate::space::Space;
use crate::state::{Activity, Body, GazeTarget};
use mdsim_core::pq::{ColData, Table};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameAcc {
    pub start_min: f64,
    pub arrivals: u32,
    pub departures: u32,
    pub conc_sum: f64,
    pub conc_max: u32,
    pub samples: u32,
    pub speed_sum: f64,
    pub speed_n: u32,
    pub notices: u32,
    pub purchases: u32,
    pub revenue: f64,
    pub enters: u32,
    pub edge_density_sum: Vec<f64>,
    pub edge_speed_sum: Vec<f64>,
    pub edge_speed_n: Vec<u32>,
    /// 렌더 표본마다 더한 간선 유효 면적(㎡, 02c §2.3).
    pub edge_area_sum: Vec<f64>,
    /// 간선 길이의 50% 이상을 지나고 떠난 몸 수.
    pub edge_pass: Vec<u32>,
    pub place_queue_sum: Vec<f64>,
    pub place_queue_max: Vec<u32>,
    pub place_inside_sum: Vec<f64>,
    pub revenue_by_cat: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub body: u32,
    pub background: bool,
    pub pts: Vec<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapGlyph {
    pub body: u32,
    pub x: f64,
    pub z: f64,
    pub heading: f64,
    pub gaze: f64,
    pub focal_range: f64,
    pub focal_angle: f64,
    pub background: bool,
    pub looking_at: Option<(f64, f64)>,
    pub activity: u8,
    pub noticed_now: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapCapture {
    pub tick: u64,
    pub t_s: f64,
    pub glyphs: Vec<SnapGlyph>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SummaryAcc {
    pub frames: Vec<FrameAcc>,
    pub tracks: BTreeMap<u32, Track>,
    pub snaps: Vec<SnapCapture>,
    pub snap_center: Option<(f64, f64)>,
    pub snap_ticks: Vec<u64>,
    /// 실행 창 시작(초). 10분 프레임 번호는 창이 아니라 시각 눈금 floor(t_s / 600)을 따른다(02d §1.2, DR-17).
    #[serde(default)]
    pub w0_s: f64,
}

/// 시각 t_s가 속한 10분 눈금 번호(반열린 [10k, 10k + 10)분).
fn grid10(t_s: f64) -> usize {
    (t_s / 600.0).floor().max(0.0) as usize
}

pub const TRACK_EVERY_TICKS: u64 = 10;
pub const SNAP_RADIUS_M: f64 = 22.0;

impl SummaryAcc {
    pub fn new(window_start_s: f64, n_ticks: u64, dt: f64, n_edges: usize, n_places: usize, render_every: u64) -> SummaryAcc {
        let base = grid10(window_start_s);
        let last = window_start_s + n_ticks.saturating_sub(1) as f64 * dt;
        let n_frames = grid10(last) - base + 1;
        let frames = (0..n_frames)
            .map(|k| FrameAcc {
                start_min: 10.0 * (base + k) as f64,
                edge_density_sum: vec![0.0; n_edges],
                edge_speed_sum: vec![0.0; n_edges],
                edge_speed_n: vec![0; n_edges],
                edge_area_sum: vec![0.0; n_edges],
                edge_pass: vec![0; n_edges],
                place_queue_sum: vec![0.0; n_places],
                place_queue_max: vec![0; n_places],
                place_inside_sum: vec![0.0; n_places],
                ..Default::default()
            })
            .collect();
        // 스냅샷: 창 가운데부터 렌더 프레임 4장(2초 간격)
        let mid = (n_ticks / 2) / render_every * render_every;
        let gap = (2.0 / dt).round().max(1.0) as u64;
        let snap_ticks = (0..4).map(|k| mid + k * gap).filter(|&t| t < n_ticks).collect();
        SummaryAcc { frames, tracks: BTreeMap::new(), snaps: Vec::new(), snap_center: None, snap_ticks, w0_s: window_start_s }
    }

    /// 틱이 속한 프레임(첫 프레임은 창 시작이 든 10분 눈금).
    pub fn frame_ix(&self, tick: u64, dt: f64) -> usize {
        (grid10(self.w0_s + tick as f64 * dt) - grid10(self.w0_s)).min(self.frames.len().saturating_sub(1))
    }

    /// 렌더 프레임마다 부른다(틱 t의 상태).
    #[allow(clippy::too_many_arguments)]
    pub fn on_render(&mut self, tick: u64, t_s: f64, dt: f64, bodies: &[Body], space: &Space, env: &EnvState, queues: &[crate::collective::PlaceQueue], focal: &dyn Fn(&Body) -> (f64, f64)) {
        let fi = self.frame_ix(tick, dt);
        let f = &mut self.frames[fi];
        let alive = bodies.iter().filter(|b| b.alive).count() as u32;
        f.conc_sum += alive as f64;
        f.conc_max = f.conc_max.max(alive);
        f.samples += 1;
        let mut cnt = vec![0u32; space.edges.len()];
        for b in bodies.iter().filter(|b| b.alive && b.activity.moving()) {
            let s = b.vel.len();
            f.speed_sum += s;
            f.speed_n += 1;
            if let Some(e) = b.cur_edge {
                cnt[e as usize] += 1;
                f.edge_speed_sum[e as usize] += s;
                f.edge_speed_n[e as usize] += 1;
            }
        }
        for b in bodies.iter().filter(|b| b.alive && !b.activity.moving() && b.activity.on_street()) {
            if let Some(e) = b.cur_edge {
                cnt[e as usize] += 1;
            }
        }
        let stall = stall_area_by_edge(space, env);
        for (e, es) in space.edges.iter().enumerate() {
            let area = es.walk_area(env, stall[e]);
            f.edge_area_sum[e] += area;
            if cnt[e] > 0 {
                f.edge_density_sum[e] += cnt[e] as f64 / area;
            }
        }
        for (i, q) in queues.iter().enumerate().take(f.place_queue_sum.len()) {
            f.place_queue_sum[i] += q.waiting.len() as f64;
            f.place_queue_max[i] = f.place_queue_max[i].max(q.waiting.len() as u32);
            f.place_inside_sum[i] += q.inside.len() as f64;
        }
        // 궤적: 몸 id 순 처음 60명(내국인)과 20명(배경)
        if tick.is_multiple_of(TRACK_EVERY_TICKS) {
            for b in bodies.iter().filter(|b| b.alive && b.activity.on_street()) {
                let take = self.tracks.contains_key(&b.id) || {
                    let n_dom = self.tracks.values().filter(|t| !t.background).count();
                    let n_bg = self.tracks.values().filter(|t| t.background).count();
                    if b.is_background() { n_bg < 20 } else { n_dom < 60 }
                };
                if take {
                    self.tracks.entry(b.id).or_insert_with(|| Track { body: b.id, background: b.is_background(), pts: Vec::new() }).pts.push((b.pos.x as f32, b.pos.z as f32));
                }
            }
        }
        // 스냅샷
        if self.snap_ticks.contains(&tick) {
            if self.snap_center.is_none() {
                self.snap_center = densest_point(bodies);
            }
            if let Some((cx, cz)) = self.snap_center {
                let c = mdsim_core::geom::V2::new(cx, cz);
                let glyphs = bodies
                    .iter()
                    .filter(|b| b.alive && b.activity.on_street() && b.pos.dist(c) <= SNAP_RADIUS_M)
                    .map(|b| {
                        let (fr, fa) = focal(b);
                        let looking_at = match b.gaze_target {
                            GazeTarget::Place(p) => Some((space.places[p as usize].anchor.x, space.places[p as usize].anchor.z)),
                            _ => None,
                        };
                        SnapGlyph { body: b.id, x: b.pos.x, z: b.pos.z, heading: b.heading, gaze: b.gaze, focal_range: fr, focal_angle: fa, background: b.is_background(), looking_at, activity: b.activity.code(), noticed_now: !b.new_notices.is_empty() }
                    })
                    .collect();
                self.snaps.push(SnapCapture { tick, t_s, glyphs });
            }
        }
    }

    /// 10분 집계를 긴 형식 표로(metrics_10min.parquet, 02c §2.6). Python이 읽는 metric은
    /// edge_density_mean·edge_speed_mean·edge_effective_area_m2·edge_pass_count(key = edge_id),
    /// place_queue_len_mean·place_queue_len_max·place_inside_mean(key = place_id)이고 나머지는 plot용이다.
    pub fn metrics_table(&self, space: &Space) -> Table {
        let (mut fm, mut metric, mut key, mut value) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut push = |f: f64, m: &str, k: String, v: f64| {
            fm.push(f);
            metric.push(m.to_string());
            key.push(k);
            value.push(v);
        };
        for f in &self.frames {
            let s = f.samples.max(1) as f64;
            push(f.start_min, "active_bodies_mean", String::new(), f.conc_sum / s);
            push(f.start_min, "active_bodies_max", String::new(), f.conc_max as f64);
            push(f.start_min, "arrivals", String::new(), f.arrivals as f64);
            push(f.start_min, "departures", String::new(), f.departures as f64);
            push(f.start_min, "mean_speed_mps", String::new(), if f.speed_n > 0 { f.speed_sum / f.speed_n as f64 } else { 0.0 });
            push(f.start_min, "notices", String::new(), f.notices as f64);
            push(f.start_min, "enters", String::new(), f.enters as f64);
            push(f.start_min, "purchases", String::new(), f.purchases as f64);
            push(f.start_min, "revenue_krw", String::new(), f.revenue);
            for (cat, v) in &f.revenue_by_cat {
                push(f.start_min, "revenue_krw_by_category", cat.clone(), *v);
            }
            for (e, es) in space.edges.iter().enumerate() {
                if f.edge_density_sum[e] > 0.0 {
                    push(f.start_min, "edge_density_mean", es.id.to_string(), f.edge_density_sum[e] / s);
                }
                if f.edge_speed_n[e] > 0 {
                    push(f.start_min, "edge_speed_mean", es.id.to_string(), f.edge_speed_sum[e] / f.edge_speed_n[e] as f64);
                }
                if f.samples > 0 {
                    push(f.start_min, "edge_effective_area_m2", es.id.to_string(), f.edge_area_sum[e] / s);
                }
                if f.edge_pass[e] > 0 {
                    push(f.start_min, "edge_pass_count", es.id.to_string(), f.edge_pass[e] as f64);
                }
            }
            for (i, pl) in space.places.iter().enumerate().take(f.place_queue_sum.len()) {
                if f.place_queue_max[i] > 0 {
                    push(f.start_min, "place_queue_len_mean", pl.id.clone(), f.place_queue_sum[i] / s);
                    push(f.start_min, "place_queue_len_max", pl.id.clone(), f.place_queue_max[i] as f64);
                }
                if f.place_inside_sum[i] > 0.0 {
                    push(f.start_min, "place_inside_mean", pl.id.clone(), f.place_inside_sum[i] / s);
                }
            }
        }
        Table::new()
            .col("frame_start_min", ColData::I32(fm.iter().map(|v| v.round() as i32).collect()))
            .col("metric", ColData::Str(metric))
            .col("key", ColData::Str(key))
            .col("value", ColData::F64(value))
    }
}

/// 간선별 열린 노점 점유 면적(㎡).
pub fn stall_area_by_edge(space: &Space, env: &EnvState) -> Vec<f64> {
    let mut stall = vec![0.0f64; space.edges.len()];
    for (i, p) in space.places.iter().enumerate() {
        if env.stall_active[i] {
            stall[p.edge as usize] += std::f64::consts::PI * crate::space::STALL_RADIUS_M.powi(2);
        }
    }
    stall
}

/// 몸이 가장 많이 모인 지점(2m 칸, 반경 안 인원 최대).
fn densest_point(bodies: &[Body]) -> Option<(f64, f64)> {
    let pts: Vec<_> = bodies.iter().filter(|b| b.alive && b.activity.on_street() && b.activity != Activity::Gone).map(|b| b.pos).collect();
    let mut best: Option<(usize, f64, f64)> = None;
    for p in &pts {
        let n = pts.iter().filter(|q| q.dist(*p) <= 12.0).count();
        if best.map(|(bn, bx, bz)| n > bn || (n == bn && (p.x, p.z) < (bx, bz))).unwrap_or(true) {
            best = Some((n, p.x, p.z));
        }
    }
    best.map(|(_, x, z)| (x, z))
}
