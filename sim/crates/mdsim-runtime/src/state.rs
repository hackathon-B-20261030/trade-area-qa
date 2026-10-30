//! 몸과 개인 상태 (C-206, 02d I-206-r).
//!
//! 갱신은 이 모듈의 함수와 등록된 단계(behavior.rs 결정 적용, step.rs 틱 단계)만 한다(E206-1). 필드는 통합 시험이
//! 상황을 꾸미도록 `pub`이고, 등록 모듈 밖의 직접 쓰기는 e206_1이 소스를 읽어 거절한다(S-E50).

use crate::route::{EdgeIx, NodeIx};
use mdsim_core::bundle::Mode;
use mdsim_core::dims::{IntVec, NI};
use mdsim_core::geom::V2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Activity {
    Walk,
    Browse,
    Queue,
    Inside,
    Rest,
    Watch,
    Exit,
    Gone,
}

impl Activity {
    pub fn code(self) -> u8 {
        match self {
            Activity::Walk => 1,
            Activity::Browse => 2,
            Activity::Queue => 3,
            Activity::Inside => 4,
            Activity::Rest => 5,
            Activity::Watch => 6,
            Activity::Exit => 7,
            Activity::Gone => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Activity::Walk => "걷기",
            Activity::Browse => "둘러보기",
            Activity::Queue => "줄서기",
            Activity::Inside => "장소 안",
            Activity::Rest => "쉬기",
            Activity::Watch => "관람",
            Activity::Exit => "떠나는 중",
            Activity::Gone => "사라짐",
        }
    }

    /// 01b §3.3 상태도의 허용 전이(E222-3).
    pub fn allowed(self, to: Activity) -> bool {
        use Activity::*;
        if self == to {
            return true;
        }
        matches!(
            (self, to),
            (Walk, Browse) | (Browse, Walk) | (Walk, Queue) | (Browse, Queue) | (Queue, Inside) | (Queue, Walk) | (Walk, Inside) | (Browse, Inside) | (Inside, Walk) | (Walk, Rest) | (Rest, Walk) | (Walk, Watch) | (Watch, Walk) | (Walk, Exit) | (Browse, Exit) | (Rest, Exit) | (Queue, Exit) | (Inside, Exit) | (Watch, Exit) | (Exit, Gone)
        )
    }

    pub fn moving(self) -> bool {
        matches!(self, Activity::Walk | Activity::Browse | Activity::Exit)
    }

    /// 보행 공간에 몸이 있는가(장소 안·사라짐 제외, S-F9).
    pub fn on_street(self) -> bool {
        !matches!(self, Activity::Inside | Activity::Gone)
    }
}

/// 결정 계기(01b §3.3 표, E222-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    Spawned,
    ActivityEnd,
    Arrived,
    Deadline,
    Fatigue,
    ObservedChange,
    Notice,
    StartRoute,
    Admitted,
    NoTarget,
    EnvChange,
    DwellEnd,
    Collision,
    /// 연기(DEFER)한 시간이 끝남.
    DeferEnd,
}

impl Trigger {
    pub fn name(self) -> &'static str {
        match self {
            Trigger::Spawned => "도착",
            Trigger::ActivityEnd => "활동 종료",
            Trigger::Arrived => "목표 도착",
            Trigger::Deadline => "마감",
            Trigger::Fatigue => "피로",
            Trigger::ObservedChange => "관찰 값 변화",
            Trigger::Notice => "알아차림",
            Trigger::StartRoute => "이동 시작",
            Trigger::Admitted => "입장",
            Trigger::NoTarget => "목표 없음",
            Trigger::EnvChange => "환경 사건",
            Trigger::DwellEnd => "시선 유지 종료",
            Trigger::DeferEnd => "연기 끝",
            Trigger::Collision => "충돌 위험",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Target {
    None,
    Place(u32),
    Portal(u32),
    /// EXPLORE의 목표 교차점(노드 인덱스).
    Node(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum GazeTarget {
    Path,
    Footing,
    Place(u32),
}

impl GazeTarget {
    pub fn code(self) -> u32 {
        match self {
            GazeTarget::Path => 0xFFFF_FFFF,
            GazeTarget::Footing => 0xFFFF_FFFE,
            GazeTarget::Place(p) => p,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutePlan {
    pub nodes: Vec<NodeIx>,
    pub edges: Vec<(EdgeIx, bool)>,
    pub idx: usize,
    pub final_pos: V2,
    pub speed_factor: f64,
    /// 경로를 정할 때 기대한 간선 밀도(관찰 값 변화 계기, route.replan_margin).
    pub expected_density: f64,
    /// 간선마다 옆 오프셋 비율(−0.5~0.5, 키 LATERAL, S-E14).
    pub lateral: Vec<f64>,
    /// 보도 간선에서 설 쪽(간선 a→b 왼쪽 +1, 오른쪽 −1, 미정 0).
    pub side: Vec<i8>,
}

impl RoutePlan {
    pub fn current(&self) -> Option<(EdgeIx, bool)> {
        self.edges.get(self.idx).copied()
    }

    pub fn next(&self) -> Option<(EdgeIx, bool)> {
        self.edges.get(self.idx + 1).copied()
    }

    pub fn on_last_edge(&self) -> bool {
        self.idx + 1 >= self.edges.len()
    }
}

/// 인지 집합의 한 항목(C-214)과 관찰 값(DR-221 ②).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aware {
    pub place: u32,
    pub first_tick: u64,
    pub last_seen_tick: Option<u64>,
    pub seen_open: Option<bool>,
    pub seen_queue: Option<u32>,
    /// 목적·익숙함으로 처음부터 알던 장소(C-214 ⑤).
    pub known_before: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservedEdge {
    pub edge: u32,
    pub density: f64,
    pub tick: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyStats {
    pub distance_m: f64,
    pub visits: u32,
    pub purchases: u32,
    pub spent_krw: f64,
    pub notices: u32,
    pub gaze_path_s: f64,
    pub gaze_footing_s: f64,
    pub gaze_place_s: f64,
    pub max_fatigue: f64,
    pub sense_over_s: f64,
    pub way_over: bool,
    pub queue_wait_s: f64,
    pub inside_s: f64,
    pub decisions: u32,
    /// 지금 장소 활동(가게 안·행사 보기·벤치 쉬기)을 시작한 틱. exit·rest_end의 머문 초를 잰다(S-E55).
    #[serde(default)]
    pub act_since: u64,
}

/// 배경 보행자(관광객 합성 집단 전, S-F18)의 일정.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BgItinerary {
    pub stops: Vec<u32>,
    pub dwell_s: Vec<f64>,
    pub next: usize,
    pub exit_portal: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: u32,
    /// 프로필 인덱스(배경 보행자는 None).
    pub profile: Option<u32>,
    pub replicate: u16,
    pub entity: String,
    pub arrival_tick: u64,
    pub deadline_tick: u64,
    pub portal_in: u32,
    pub mode: Mode,
    pub alive: bool,
    pub departed_tick: Option<u64>,
    pub portal_out: Option<u32>,
    pub pos: V2,
    pub vel: V2,
    pub heading: f64,
    pub radius: f64,
    pub base_speed: f64,
    pub gaze: f64,
    pub gaze_target: GazeTarget,
    pub gaze_since: u64,
    pub activity: Activity,
    pub act_until: u64,
    pub act_place: Option<u32>,
    pub target: Target,
    pub purpose_value: f64,
    pub route: Option<RoutePlan>,
    pub aware: Vec<Aware>,
    pub obs_edges: Vec<ObservedEdge>,
    pub fatigue: f64,
    pub sat: IntVec,
    /// 들어간 적이 있는 장소(업종과 무관한 목적의 충족 판정).
    pub entered: Vec<u32>,
    pub spent: f64,
    pub stroller: bool,
    pub luggage: bool,
    pub child: bool,
    /// 맥락별 마지막 결정 틱(VISIT 제외 5개: NEXT, ROUTE, REST, PURCHASE, LOOK).
    pub last_decision: [Option<u64>; 5],
    pub last_notice_trigger: Option<u64>,
    pub new_notices: Vec<u32>,
    pub cur_edge: Option<u32>,
    /// 지난 이동에서 목표에 닿았는가(다음 틱의 '목표 도착' 계기).
    pub arrived: bool,
    /// 막혀 서 있던 틱 수(교착 감지).
    pub blocked_ticks: u32,
    /// 교착을 풀려고 이웃을 무시하며 움직이는 동안의 끝 틱(이 틱 전까지, 0이면 아님, S-E56).
    #[serde(default)]
    pub jam_until: u64,
    /// 막혔을 때 잠시 따라가는 국소 경유점과 만료 틱(S-E22).
    pub detour: Option<(V2, u64)>,
    /// EXPLORE로 가 본 교차점.
    pub explored: Vec<u32>,
    /// 지금 간선에 들어선 뒤 걸은 거리(m).
    pub edge_run_m: f64,
    /// 이번 틱에 길이 50% 이상을 지나고 떠난 간선(edge_pass_count).
    pub passed: Option<u32>,
    /// 마지막 NEXT에서 고르지 않은 가능한 대안 중 최고 효용(ROUTE의 ABANDON 기준, S-E23).
    pub fallback_u: f64,
    /// ROUTE에서 DEFER를 골라 그 자리에 서 있는 마감 틱(S-E36).
    pub defer_until: Option<u64>,
    /// 고른 대안이 차선보다 효용이 낮았던 만큼(관측되지 않은 선호로 이긴 몫). 경로를 다시 볼 때 유지한다.
    pub choice_margin: f64,
    /// 다음 틱에 처리할 계기(입장 뒤 구매, 행선지 뒤 경로 등).
    pub pending: Option<Trigger>,
    /// 잠시 피할 장소(포기한 목표)와 그 기한 틱.
    pub avoid: Option<(u32, u64)>,
    pub rest_seated: bool,
    pub last_place: Option<u32>,
    /// 마지막으로 결정한 행선지의 목적 가치(경로 결정에 쓴다).
    pub target_purpose: f64,
    pub stats: BodyStats,
    pub bg: Option<BgItinerary>,
}

impl Body {
    pub fn is_background(&self) -> bool {
        self.profile.is_none()
    }

    pub(crate) fn set_activity(&mut self, to: Activity) -> Result<(), String> {
        if !self.activity.allowed(to) {
            return Err(format!("몸 {}: 허용되지 않은 활동 전이 {:?} → {:?}", self.id, self.activity, to));
        }
        self.activity = to;
        Ok(())
    }

    pub fn aware_of(&self, place: u32) -> Option<&Aware> {
        self.aware.binary_search_by_key(&place, |a| a.place).ok().map(|i| &self.aware[i])
    }

    pub(crate) fn aware_mut(&mut self, place: u32) -> Option<&mut Aware> {
        match self.aware.binary_search_by_key(&place, |a| a.place) {
            Ok(i) => Some(&mut self.aware[i]),
            Err(_) => None,
        }
    }

    /// 인지 집합에 넣는다(이미 있으면 그대로).
    pub(crate) fn add_aware(&mut self, place: u32, tick: u64, known_before: bool) -> bool {
        match self.aware.binary_search_by_key(&place, |a| a.place) {
            Ok(_) => false,
            Err(i) => {
                self.aware.insert(i, Aware { place, first_tick: tick, last_seen_tick: None, seen_open: None, seen_queue: None, known_before });
                true
            }
        }
    }

    pub(crate) fn observe_edge(&mut self, edge: u32, density: f64, tick: u64) {
        if let Some(o) = self.obs_edges.iter_mut().find(|o| o.edge == edge) {
            o.density = density;
            o.tick = tick;
        } else {
            self.obs_edges.push(ObservedEdge { edge, density, tick });
            if self.obs_edges.len() > 32 {
                self.obs_edges.sort_by_key(|o| std::cmp::Reverse(o.tick));
                self.obs_edges.truncate(24);
                self.obs_edges.sort_by_key(|o| o.edge);
            }
        }
    }

    /// 본 간선 밀도(신선도 기한 안이면 그 값, 아니면 None → 사전 기대, DR-221 ②).
    pub fn seen_density(&self, edge: u32, tick: u64, fresh_ticks: u64) -> Option<f64> {
        self.obs_edges.iter().find(|o| o.edge == edge && tick.saturating_sub(o.tick) <= fresh_ticks).map(|o| o.density)
    }

    pub fn minutes_left(&self, tick: u64, dt_s: f64) -> f64 {
        (self.deadline_tick.saturating_sub(tick)) as f64 * dt_s / 60.0
    }
}

/// 피로 갱신(I-206-r): 이동 거리 × (1 + 오르막·계단 가중) ÷ 편안 보행 거리.
pub(crate) fn add_walk_fatigue(b: &mut Body, dist_m: f64, grade_pct: f64, is_steps: bool, walk_comfort_m: f64, climb_coef: f64, steps_coef: f64) {
    let load = dist_m * (1.0 + climb_coef * grade_pct.max(0.0) / 10.0 + if is_steps { steps_coef } else { 0.0 });
    b.fatigue += load / walk_comfort_m.max(100.0);
    b.stats.max_fatigue = b.stats.max_fatigue.max(b.fatigue);
}

/// 쉬기 회복(앉으면 더 크게, E206-3).
pub(crate) fn recover(b: &mut Body, minutes: f64, seated: bool, seated_rate: f64, standing_rate: f64) {
    let rate = if seated { seated_rate } else { standing_rate };
    b.fatigue = (b.fatigue - rate * minutes).max(0.0);
}

/// 충족도: offer × gain × (1 − 충족도)만큼 오른다(S-R21).
pub(crate) fn satisfy(b: &mut Body, offer: &IntVec, gain: f64) {
    for k in 0..NI {
        let add = (offer[k] * gain).clamp(0.0, 1.0) * (1.0 - b.sat[k]);
        b.sat[k] = (b.sat[k] + add).clamp(0.0, 1.0);
    }
}

pub(crate) fn spend(b: &mut Body, krw: f64) {
    b.spent += krw.max(0.0);
    b.stats.spent_krw += krw.max(0.0);
}
