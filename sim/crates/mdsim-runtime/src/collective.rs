//! 집단 상태 (C-210, 02d I-210-r): 틱 t의 위치로 공간 색인·국소 밀도·간선 밀도를 만든다.
//! 결과는 같은 틱의 지각·결정·이동이 읽고 다음 틱에 다시 만든다(E210-1, E210-6).

use crate::env::EnvState;
use crate::space::{PlaceKind, Space};
use crate::state::Body;
use mdsim_core::geom::V2;
use serde::{Deserialize, Serialize};

pub const GRID_M: f64 = 2.0;

fn cell(p: V2) -> (i64, i64) {
    ((p.x / GRID_M).floor() as i64, (p.z / GRID_M).floor() as i64)
}

fn key(cx: i64, cz: i64) -> i64 {
    (cx + 1_000_000) * 2_000_003 + (cz + 1_000_000)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Collective {
    /// (칸 키, 몸 id) 정렬 배열.
    pub keys: Vec<(i64, u32)>,
    /// 틱 t의 위치·반지름·거리 위 여부(몸 id별). 이웃 계산은 이 사본만 읽는다(동시 갱신).
    pub pos: Vec<V2>,
    pub radius: Vec<f64>,
    pub on_street: Vec<bool>,
    /// 몸 id별 국소 밀도(인/㎡). 거리에 없는 몸은 0.
    pub local_density: Vec<f64>,
    pub edge_count: Vec<u32>,
    pub edge_density: Vec<f64>,
}

impl Collective {
    pub fn build(bodies: &[Body], space: &Space, env: &EnvState, radius: f64) -> Collective {
        let mut keys: Vec<(i64, u32)> = bodies
            .iter()
            .filter(|b| b.alive && b.activity.on_street())
            .map(|b| {
                let (cx, cz) = cell(b.pos);
                (key(cx, cz), b.id)
            })
            .collect();
        keys.sort_unstable();
        let mut c = Collective {
            keys,
            pos: bodies.iter().map(|b| b.pos).collect(),
            radius: bodies.iter().map(|b| b.radius).collect(),
            on_street: bodies.iter().map(|b| b.alive && b.activity.on_street()).collect(),
            local_density: vec![0.0; bodies.len()],
            edge_count: vec![0; space.edges.len()],
            edge_density: vec![0.0; space.edges.len()],
        };
        let area = std::f64::consts::PI * radius * radius;
        let mut buf = Vec::new();
        for b in bodies.iter().filter(|b| b.alive && b.activity.on_street()) {
            c.neighbors(b.pos, radius, &mut buf);
            c.local_density[b.id as usize] = buf.len() as f64 / area;
            if let Some(e) = b.cur_edge {
                c.edge_count[e as usize] += 1;
            }
        }
        let mut stall_area = vec![0.0; space.edges.len()];
        for (i, p) in space.places.iter().enumerate() {
            if p.kind == PlaceKind::Stall && env.stall_active[i] {
                stall_area[p.edge as usize] += std::f64::consts::PI * crate::space::STALL_RADIUS_M.powi(2);
            }
        }
        for (e, es) in space.edges.iter().enumerate() {
            let a = es.walk_area(env, stall_area[e]);
            c.edge_density[e] = c.edge_count[e] as f64 / a;
        }
        c
    }

    /// 위치 색인만 만든다(국소 밀도·간선 밀도는 채우지 않는다). 이동 단계가 그 틱 결정 뒤의 활동으로 이웃을 찾고(S-E43),
    /// 도착·퇴장 자리를 고를 때(S-E45) 쓴다.
    pub fn positions(bodies: &[Body]) -> Collective {
        Collective::positions_where(bodies, |_| true)
    }

    /// `positions`에서 `keep`이 거짓인 몸은 색인에서 뺀다(이웃에게 보이지 않는다). 교착을 풀려고 이웃을 무시하는 몸에 쓴다(S-E56).
    pub fn positions_where(bodies: &[Body], keep: impl Fn(&Body) -> bool) -> Collective {
        let seen = |b: &Body| b.alive && b.activity.on_street() && keep(b);
        let mut keys: Vec<(i64, u32)> = bodies
            .iter()
            .filter(|b| seen(b))
            .map(|b| {
                let (cx, cz) = cell(b.pos);
                (key(cx, cz), b.id)
            })
            .collect();
        keys.sort_unstable();
        Collective {
            keys,
            pos: bodies.iter().map(|b| b.pos).collect(),
            radius: bodies.iter().map(|b| b.radius).collect(),
            on_street: bodies.iter().map(seen).collect(),
            ..Collective::default()
        }
    }

    /// p에서 r 안에 있는 몸(id 오름차순). 틱 t의 위치를 읽는다.
    pub fn neighbors(&self, p: V2, r: f64, out: &mut Vec<u32>) {
        out.clear();
        let (cx0, cz0) = cell(p - V2::new(r, r));
        let (cx1, cz1) = cell(p + V2::new(r, r));
        for cx in cx0..=cx1 {
            let lo = key(cx, cz0);
            let hi = key(cx, cz1);
            let start = self.keys.partition_point(|&(k, _)| k < lo);
            for &(k, id) in &self.keys[start..] {
                if k > hi {
                    break;
                }
                if self.pos[id as usize].dist(p) <= r {
                    out.push(id);
                }
            }
        }
        out.sort_unstable();
    }
}

/// 장소의 줄과 안 인원(C-210 ⑤, C-223 ⑦).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaceQueue {
    pub waiting: Vec<u32>,
    pub inside: Vec<u32>,
    pub credit: f64,
    pub served: u64,
    pub max_len: u32,
    pub wait_ticks_sum: u64,
}

impl PlaceQueue {
    /// 처리율(인/분)만큼 입장 기회를 쌓는다. 쌓이는 상한은 한 명(줄이 있으면 바로 쓴다).
    pub fn accrue(&mut self, rate_per_min: f64, dt_s: f64) {
        if rate_per_min <= 0.0 {
            self.credit = 1.0;
            return;
        }
        self.credit = (self.credit + rate_per_min * dt_s / 60.0).min(1.0 + rate_per_min * dt_s / 60.0);
    }

    pub fn can_enter_now(&self, capacity: u32) -> bool {
        (self.inside.len() as u32) < capacity && self.credit >= 1.0 && self.waiting.is_empty()
    }
}
