//! 이동과 상호 회피 (C-223, 02d I-223-r, S-F5).
//!
//! 시야 기반 방향 선택(Moussaïd 외 2011)과 겹침 없는 속도 제한(Tordeux 외 2016).
//! 이웃은 틱 t의 위치만 읽는다(동시 갱신, E223-4). 몸마다 구한 희망 이동은 `limit_steps`가 이웃 간격 예산으로
//! 한 번 더 깎아 같은 틱에 서로를 향해 움직여도 겹치지 않게 한다(S-E43).

use crate::collective::Collective;
use crate::env::EnvState;
use crate::params::{tobler, LocoParams};
use crate::space::{Space, CROSS, SIDEWALK};
use crate::state::{Body, RoutePlan};
use mdsim_core::geom::{point_at, project_on_polyline, V2};
use rayon::prelude::*;

pub struct MoveIn<'a> {
    pub space: &'a Space,
    pub env: &'a EnvState,
    pub coll: &'a Collective,
    pub p: &'a LocoParams,
    pub interactions: bool,
    pub dt: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Drive {
    pub browse: bool,
    pub speed_mult: f64,
    pub fatigue: f64,
    pub peripheral_deg: f64,
    pub peripheral_range: f64,
    pub gaze_off_path: bool,
    /// 경로 대신 바로 갈 점(줄 자리 등).
    pub goal: Option<V2>,
}

#[derive(Clone, Debug)]
pub struct MoveOut {
    pub pos: V2,
    pub vel: V2,
    pub heading: f64,
    pub moved: f64,
    pub route_idx: usize,
    pub side_assign: Option<(usize, i8)>,
    pub arrived: bool,
    pub cur_edge: Option<u32>,
    pub grade_pct: f64,
    pub on_steps: bool,
    pub blocked: bool,
    /// 희망 속도. 이웃 간격 예산이 이동을 깎은 뒤 막힘을 다시 판정하는 데 쓴다.
    pub v0: f64,
    /// 이 몸이 가는 최종 위치. 깎은 뒤 도착을 다시 판정하는 데 쓴다.
    pub final_pos: V2,
}

/// 간선 a→b 기준 왼쪽 수직 벡터(그 지점의 선분 방향).
fn edge_frame(space: &Space, e: u32, s_ab: f64) -> (V2, V2) {
    let es = &space.edges[e as usize];
    let (p, dir) = point_at(&es.poly, &es.cum, s_ab);
    (p, dir.perp())
}

/// 경로의 간선 idx에서 몸이 설 옆 오프셋(간선 a→b 왼쪽이 +).
/// 차량이 다니는 혼재 간선에서는 차로 밖 양쪽 띠 안으로 옮긴다(C-210 ②).
fn lateral_offset(space: &Space, env: &EnvState, route: &RoutePlan, idx: usize, side: i8, p: &LocoParams) -> f64 {
    let (e, _) = route.edges[idx];
    let es = &space.edges[e as usize];
    if es.sidewalks {
        side as f64 * (es.width / 2.0 - es.sw / 2.0)
    } else {
        let u = route.lateral.get(idx).copied().unwrap_or(0.0);
        if es.vehicle_mixed && !es.is_crossing && es.vehicle_now(env) {
            let inner = space.lane_w / 2.0 + p.body_radius_m + 0.05;
            let outer = (es.width / 2.0 - p.wall_clearance_m - p.body_radius_m).max(inner);
            let sgn = if u >= 0.0 { 1.0 } else { -1.0 };
            return sgn * (inner + (outer - inner) * (u.abs() * 2.0).min(1.0));
        }
        u * (es.width - 2.0 * p.wall_clearance_m - 2.0 * p.body_radius_m).max(0.0) * 0.8
    }
}

/// 보도 간선에 처음 들어설 때 쪽을 정한다(횡단 구역이면 목적지 쪽, 아니면 지금 서 있는 쪽).
fn choose_side(space: &Space, route: &RoutePlan, idx: usize, pos: V2) -> i8 {
    let (e, _) = route.edges[idx];
    let es = &space.edges[e as usize];
    let (s_ab, _) = project_on_polyline(&es.poly, &es.cum, pos);
    let (c, left) = edge_frame(space, e, s_ab);
    let here = (pos - c).dot(left);
    let flag = space.flag_at(pos);
    if flag & CROSS != 0 || flag & SIDEWALK == 0 || here.abs() < 0.5 {
        // 건널 수 있는 곳: 다음 비보도 간선이나 목적지 쪽
        let mut goal = route.final_pos;
        for j in idx + 1..route.edges.len() {
            let (ne, nfwd) = route.edges[j];
            let nes = &space.edges[ne as usize];
            if !nes.sidewalks {
                goal = if nfwd { nes.poly[0] } else { *nes.poly.last().unwrap() };
                break;
            }
        }
        let (gs, _) = project_on_polyline(&es.poly, &es.cum, goal);
        let (gc, gl) = edge_frame(space, e, gs);
        let v = (goal - gc).dot(gl);
        if v.abs() > 0.5 {
            return if v > 0.0 { 1 } else { -1 };
        }
    }
    if here >= 0.0 {
        1
    } else {
        -1
    }
}

/// 다음 목표점, 진행한 경로 인덱스, 현재 간선과 경사.
fn steer_target(space: &Space, env: &EnvState, route: &RoutePlan, pos: V2, p: &LocoParams) -> (V2, usize, Option<(usize, i8)>, Option<u32>, f64, bool) {
    let mut idx = route.idx;
    let mut side_assign = None;
    loop {
        if idx >= route.edges.len() {
            return (route.final_pos, idx, side_assign, None, 0.0, false);
        }
        let (e, fwd) = route.edges[idx];
        let es = &space.edges[e as usize];
        let (s_ab, _) = project_on_polyline(&es.poly, &es.cum, pos);
        let s = if fwd { s_ab } else { es.len - s_ab };
        let last = idx + 1 >= route.edges.len();
        if !last && s >= es.len - 0.75 {
            idx += 1;
            continue;
        }
        let side = route.side.get(idx).copied().unwrap_or(0);
        let side = if es.sidewalks && side == 0 {
            let sd = choose_side(space, route, idx, pos);
            side_assign = Some((idx, sd));
            sd
        } else {
            side
        };
        let grade = es.grade(fwd);
        // 마지막 간선: 목적지 투영을 지났거나 가까우면 목적지로 바로
        if last {
            let (t_ab, _) = project_on_polyline(&es.poly, &es.cum, route.final_pos);
            let st = if fwd { t_ab } else { es.len - t_ab };
            if s >= st - p.lookahead_m || pos.dist(route.final_pos) <= p.lookahead_m {
                return (route.final_pos, idx, side_assign, Some(e), grade, es.is_steps);
            }
        }
        let want = s + p.lookahead_m;
        let lat = lateral_offset(space, env, route, idx, side, p);
        let point_on = |edge: u32, forward: bool, s_travel: f64, lat: f64| -> V2 {
            let ee = &space.edges[edge as usize];
            let s_ab = if forward { s_travel } else { ee.len - s_travel };
            let (c, left) = edge_frame(space, edge, s_ab.clamp(0.0, ee.len));
            c + left * lat
        };
        let target = if want <= es.len {
            point_on(e, fwd, want, lat)
        } else if !last {
            let (ne, nfwd) = route.edges[idx + 1];
            let nside = route.side.get(idx + 1).copied().unwrap_or(0);
            let nside = if space.edges[ne as usize].sidewalks && nside == 0 { side.clamp(-1, 1) } else { nside };
            let nlat = lateral_offset(space, env, route, idx + 1, if nside == 0 { 1 } else { nside }, p);
            let nlat = if space.edges[ne as usize].sidewalks { nlat } else { lateral_offset(space, env, route, idx + 1, 0, p) };
            let nt = point_on(ne, nfwd, (want - es.len).min(space.edges[ne as usize].len), nlat);
            // 다음 간선의 점이 지금 진행 방향으로 몸보다 뒤면(넓은 보도 간선의 옆 오프셋 등) 그 틱에 다음 간선으로
            // 넘긴다. 넘긴 인덱스는 돌아가지 않아 두 목표 사이를 오가지 않는다(S-E52).
            let (_, dir_ab) = point_at(&es.poly, &es.cum, s_ab);
            let dir = if fwd { dir_ab } else { -dir_ab };
            if (nt - pos).dot(dir) < 0.0 {
                idx += 1;
                continue;
            }
            nt
        } else {
            route.final_pos
        };
        return (target, idx, side_assign, Some(e), grade, es.is_steps);
    }
}

/// 차량이 다니는 차로로 들어서는 방향의 자유 거리 배율(S-E27).
pub const LANE_ENTRY_FACTOR: f64 = 0.5;

/// 이 틈(m)보다 가까운 이웃은 맞닿은 것으로 본다(S-E44).
pub const CONTACT_M: f64 = 0.02;

/// 한 몸의 다음 위치(틱 t의 이웃 위치만 읽는다).
pub fn move_body(m: &MoveIn, b: &Body, d: &Drive) -> MoveOut {
    let p = m.p;
    let (target, route_idx, side_assign, cur_edge, grade, on_steps) = match (&d.goal, &b.route) {
        (Some(g), _) => (*g, b.route.as_ref().map(|r| r.idx).unwrap_or(0), None, b.cur_edge, 0.0, false),
        (None, Some(r)) => steer_target(m.space, m.env, r, b.pos, p),
        (None, None) => (b.pos, 0, None, b.cur_edge, 0.0, false),
    };
    let final_pos = match (&d.goal, &b.route) {
        (Some(g), _) => *g,
        (None, Some(r)) => r.final_pos,
        (None, None) => b.pos,
    };
    let to_target = target - b.pos;
    let dist_final = b.pos.dist(final_pos);
    let still = MoveOut { pos: b.pos, vel: V2::ZERO, heading: b.heading, moved: 0.0, route_idx, side_assign, arrived: dist_final <= p.arrive_m, cur_edge, grade_pct: grade, on_steps, blocked: false, v0: 0.0, final_pos };
    if to_target.len() < 1e-6 || dist_final <= p.arrive_m * 0.5 {
        return still;
    }
    // 희망 속도
    let mut v0 = b.base_speed * tobler(p, grade / 100.0) * d.speed_mult.max(0.05);
    if on_steps {
        v0 *= p.steps_factor;
    }
    if d.browse {
        v0 *= p.browse_factor;
    }
    v0 *= (1.0 - p.fatigue_factor * d.fatigue.clamp(0.0, 1.0)).max(0.2);
    let mut horizon = d.peripheral_range.max(1.0);
    if d.gaze_off_path {
        horizon *= 0.6;
        v0 *= 0.85;
    }
    // 시야 안 방향 훑기: 목표 방향을 가운데에 둔다(목표가 등 뒤일 때 갇히지 않게).
    let a0 = to_target.angle();
    let half = (d.peripheral_deg.to_radians() / 2.0).clamp(0.2, 1.4);
    let n = p.scan_dirs.max(3);
    let k_max = (n - 1) / 2;
    let mut neigh = Vec::new();
    if m.interactions {
        m.coll.neighbors(b.pos, horizon + 1.0, &mut neigh);
    }
    let mut best: Option<(f64, f64, f64)> = None; // (d², 방향, f)
    let in_lane = m.space.lane_active_at(b.pos, m.env);
    for step in 0..=(2 * k_max) {
        let k = if step == 0 { 0i64 } else if step % 2 == 1 { step.div_ceil(2) as i64 } else { -((step / 2) as i64) };
        let a = a0 + k as f64 * half / k_max as f64;
        let u = V2::from_angle(a);
        let fw = (m.space.free_distance(b.pos, u, horizon, m.env) - p.wall_clearance_m).max(0.0);
        let mut f = fw.min(horizon);
        // 차가 다니는 차로로 들어서는 방향은 덜 자유롭게 본다(가장자리 띠를 걷고, 필요할 때만 건넌다).
        if !in_lane && m.space.lane_active_at(b.pos + u * f.clamp(0.5, 1.5), m.env) {
            f *= LANE_ENTRY_FACTOR;
        }
        for &j in &neigh {
            if j == b.id {
                continue;
            }
            if !m.coll.on_street[j as usize] {
                continue;
            }
            let w = m.coll.pos[j as usize] - b.pos;
            let r = b.radius + m.coll.radius[j as usize];
            // 맞닿았거나 겹친 이웃(틈 CONTACT_M 미만)은 광선이 이웃 가장자리에서 시작해 모든 방향의 자유 거리가 0이 되므로
            // 여기서는 뺀다. 서로를 향한 접근은 `limit_steps`가 막고 옆으로 미끄러지게 한다(S-E44).
            if w.dot(w) < (r + CONTACT_M) * (r + CONTACT_M) {
                continue;
            }
            let proj = w.dot(u);
            if proj <= 0.0 {
                continue;
            }
            let perp2 = w.dot(w) - proj * proj;
            if perp2 >= r * r {
                continue;
            }
            let t = (proj - (r * r - perp2).sqrt()).max(0.0);
            f = f.min(t);
        }
        let d2 = horizon * horizon + f * f - 2.0 * horizon * f * (a0 - a).cos();
        if best.map(|(bd, _, _)| d2 < bd - 1e-12).unwrap_or(true) {
            best = Some((d2, a, f));
        }
    }
    let (_, a_star, f_star) = best.unwrap();
    let mut v = v0.min(f_star / p.time_gap_s.max(m.dt));
    // 목적지를 지나치지 않게
    if dist_final < v * m.dt + p.arrive_m && target == final_pos {
        v = v.min((dist_final - 0.5 * p.arrive_m).max(0.0) / m.dt);
    }
    let u = V2::from_angle(a_star);
    let mut np = b.pos + u * (v * m.dt);
    if v > 0.0 && !m.space.walkable(np, m.env) {
        v *= 0.5;
        np = b.pos + u * (v * m.dt);
        if !m.space.walkable(np, m.env) {
            v = 0.0;
            np = b.pos;
        }
    }
    let moved = np.dist(b.pos);
    let heading = if moved > 1e-3 { a_star } else { b.heading };
    MoveOut {
        pos: np,
        vel: u * v,
        heading,
        moved,
        route_idx,
        side_assign,
        arrived: np.dist(final_pos) <= p.arrive_m,
        cur_edge,
        grade_pct: grade,
        on_steps,
        blocked: v0 > 0.05 && v < 0.05,
        v0,
        final_pos,
    }
}

/// 쌍의 틈 `g`를 두 몸의 접근 희망 `ci`·`cj`에 나눠 주는 몫(앞 몸 i의 몫). 희망이 틈을 넘으면 희망에 비례해
/// 틈 전체를, 아니면 희망에 남는 틈의 절반씩을 더해 준다. 두 몫의 합은 언제나 `g`다.
fn budget_share(ci: f64, cj: f64, g: f64) -> f64 {
    let s = ci + cj;
    if s >= g {
        if s > 0.0 {
            g * ci / s
        } else {
            0.0
        }
    } else {
        ci + (g - s) / 2.0
    }
}

/// 희망 이동 `d0`을 이웃마다의 접근 예산 `d·e ≤ b`(e = 이웃 쪽 단위 벡터)에 가장 가깝게 맞춘다. 예산을 넘는 접근 성분만
/// 덜어 내므로 이웃을 따라 미끄러지는 이동은 남는다(S-E43). 규칙 순서가 몸 번호에 따르지 않게 기하 값으로 정렬하고,
/// 반복으로 다 못 푼 위반은 마지막에 이동을 줄여 없앤다(보장은 언제나 성립).
fn clip_to_budgets(d0: V2, cons: &mut [(V2, f64)], sidestep: bool) -> V2 {
    cons.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.z.total_cmp(&b.0.z)).then(a.1.total_cmp(&b.1)));
    let fit = |start: V2| -> V2 {
        let mut d = start;
        for _ in 0..CLIP_ITERS {
            let mut changed = false;
            for &(e, b) in cons.iter() {
                let s = d.dot(e) - b;
                if s > 1e-12 {
                    d = d - e * s;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut k = 1.0f64;
        for &(e, b) in cons.iter() {
            let a = d.dot(e);
            if a > b + 1e-12 {
                k = k.min(b / a);
            }
        }
        d * k
    };
    let d1 = fit(d0);
    // 한동안 거의 막혀 있으면(`sidestep`, 남은 이동이 희망의 30% 미만) 가장 크게 막은 이웃을 같은 쪽(반시계)으로 돌아 지나가는
    // 옆걸음을 더해 한 번 더 맞춘다. 마주 선 두 몸이 서로 반대쪽으로 비켜 지나가게 하는 장치다.
    if sidestep && d1.len() < SIDESTEP_BELOW * d0.len() {
        if let Some(&(e, _)) = cons.iter().max_by(|a, b| (d0.dot(a.0) - a.1).total_cmp(&(d0.dot(b.0) - b.1))) {
            let d2 = fit(d0 + e.perp() * (SIDESTEP_FRAC * d0.len()));
            if d2.len() > d1.len() {
                return d2;
            }
        }
    }
    d1
}

/// 예산을 맞춘 이동이 희망 이동의 이 비율 아래면 옆걸음을 더해 본다.
const SIDESTEP_BELOW: f64 = 0.3;
/// 옆걸음의 크기(희망 이동 길이에 대한 비율).
const SIDESTEP_FRAC: f64 = 0.5;
/// 이만큼(틱) 막혀 있어야 옆걸음을 더한다(잠깐 앞 몸을 따라가는 경우는 뺀다).
const SIDESTEP_AFTER_TICKS: u32 = 4;

/// 예산을 맞추는 반복 횟수.
const CLIP_ITERS: usize = 6;

/// 이웃 간격 예산(02d I-223-r 5, S-E43). `outs`는 몸 번호로 찾는 희망 이동이고 움직이지 않는 몸은 `None`이다.
/// 거리 위 몸 쌍마다 두 몸의 접근 성분 합이 틈을 넘지 않게 이동을 덜어 낸다. 틱 t의 위치(`m.coll`)와 희망 이동만
/// 읽으므로 처리 순서·분할과 무관하다. `bodies`는 `m.coll`을 만든 몸 목록이다(몸 번호 = 목록 위치).
pub fn limit_steps(m: &MoveIn, bodies: &[Body], outs: &mut [Option<MoveOut>]) {
    if !m.interactions {
        return;
    }
    let coll = m.coll;
    let n = outs.len();
    debug_assert_eq!(bodies.len(), n);
    debug_assert_eq!(coll.pos.len(), n);
    let d: Vec<V2> = (0..n).map(|i| outs[i].as_ref().map_or(V2::ZERO, |o| o.pos - coll.pos[i])).collect();
    let mmax = d.iter().map(|v| v.len()).fold(0.0, f64::max);
    if mmax <= 0.0 {
        return;
    }
    let rmax = coll.radius.iter().copied().fold(0.0, f64::max);
    // 몸마다 최종 이동을 따로 구한다(서로 독립이라 병렬로 구해도 결과가 같다).
    let fin: Vec<V2> = (0..n)
        .into_par_iter()
        .map_init(
            || (Vec::<u32>::new(), Vec::<(V2, f64)>::new()),
            |(buf, cons), i| {
                let di = d[i];
                if di.len() < 1e-12 {
                    return di;
                }
                coll.neighbors(coll.pos[i], di.len() + mmax + 2.0 * rmax, buf);
                cons.clear();
                let mut violated = false;
                for &j in buf.iter() {
                    let j = j as usize;
                    if j == i {
                        continue;
                    }
                    let delta = coll.pos[j] - coll.pos[i];
                    let dist = delta.len();
                    if dist < 1e-9 {
                        continue;
                    }
                    let e = delta * (1.0 / dist);
                    let ci = di.dot(e).max(0.0);
                    let cj = (-d[j].dot(e)).max(0.0);
                    let g = (dist - coll.radius[i] - coll.radius[j]).max(0.0);
                    let b = budget_share(ci, cj, g);
                    violated |= ci > b + 1e-12;
                    cons.push((e, b));
                }
                if violated {
                    clip_to_budgets(di, cons, bodies[i].blocked_ticks >= SIDESTEP_AFTER_TICKS)
                } else {
                    di
                }
            },
        )
        .collect();
    for i in 0..n {
        if fin[i] == d[i] {
            continue;
        }
        let Some(o) = outs[i].as_mut() else { continue };
        let p0 = coll.pos[i];
        let mut step = fin[i];
        let mut np = p0 + step;
        if !m.space.walkable(np, m.env) {
            step = step * 0.5;
            np = p0 + step;
            if !m.space.walkable(np, m.env) {
                step = V2::ZERO;
                np = p0;
            }
        }
        o.pos = np;
        o.vel = step * (1.0 / m.dt);
        o.moved = step.len();
        o.heading = if o.moved > 1e-3 { step.angle() } else { bodies[i].heading };
        o.arrived = np.dist(o.final_pos) <= m.p.arrive_m;
        o.blocked = o.v0 > 0.05 && o.vel.len() < 0.05;
    }
}
