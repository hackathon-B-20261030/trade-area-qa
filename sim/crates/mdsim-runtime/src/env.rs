//! 환경 동역학 (C-224, 02d I-224-r). 환경 상태(t)는 시나리오와 t만의 함수다.

use crate::space::{PlaceKind, Space};
use mdsim_core::scenario::{parse_hhmm, Scenario};
use serde::{Deserialize, Serialize};

pub const LAT: f64 = 37.563;
pub const LON: f64 = 126.98525;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvState {
    pub t_s: f64,
    /// 장소별 영업 여부(행사는 진행 중일 때만 참).
    pub open: Vec<bool>,
    /// 장소별 노점 운영(노점이 아니면 거짓). 운영 중인 노점 칸은 걷지 못한다.
    pub stall_active: Vec<bool>,
    pub event_active: Vec<bool>,
    /// 차량 통행 시간 안인가(`segment.vehicle_hours_default`, 기본 하루 종일).
    pub vehicle_hours: bool,
    /// 보행 전용 시간(#1) 안인가. ped_zone 간선에만 적용된다.
    pub ped_only: bool,
    /// 조도 0~1.
    pub light: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvChange {
    pub kind: String,
    pub place: Option<usize>,
    pub value: f64,
}

/// [open, close) 분 안인가. close는 2880까지(자정 넘김은 1440 이상, 02c §2.4 (5)).
pub fn in_hours(open: f64, close: f64, m: f64) -> bool {
    if close >= open {
        (m >= open && m < close) || (m + 1440.0 >= open && m + 1440.0 < close)
    } else {
        m >= open || m < close
    }
}

/// 장소가 그 분에 열려 있는가(시나리오의 행사 일정을 반영).
pub fn place_open_at(scn: &Scenario, space: &Space, ix: usize, minute: f64) -> bool {
    let p = &space.places[ix];
    if !p.day_types.is_empty() && !p.day_types.contains(&scn.day()) {
        return false; // 그 하루 유형에는 없는 장소
    }
    if p.kind == PlaceKind::Event {
        scn.events.iter().any(|e| e.place_id == p.id && minute >= parse_hhmm(&e.start).unwrap_or(f64::MAX) && minute < parse_hhmm(&e.end).unwrap_or(f64::MIN))
    } else {
        in_hours(p.open_min, p.close_min, minute)
    }
}

pub fn env_at(scn: &Scenario, space: &Space, night_light: f64, t_s: f64) -> EnvState {
    env_at_hours(scn, space, night_light, &space.vehicle_hours, t_s)
}

pub fn env_at_hours(scn: &Scenario, space: &Space, night_light: f64, vehicle_hours: &[(f64, f64)], t_s: f64) -> EnvState {
    let minute = t_s / 60.0;
    let n = space.places.len();
    let mut open = vec![false; n];
    let mut stall_active = vec![false; n];
    let mut event_active = vec![false; n];
    for i in 0..n {
        let o = place_open_at(scn, space, i, minute);
        open[i] = o;
        match space.places[i].kind {
            PlaceKind::Stall => stall_active[i] = o,
            PlaceKind::Event => event_active[i] = o,
            _ => {}
        }
    }
    let ped_only = scn.pedestrian_only_s().iter().any(|&(a, b)| t_s >= a && t_s < b);
    let vh = vehicle_hours.is_empty() || vehicle_hours.iter().any(|&(a, b)| in_hours(a, b, minute));
    let (y, m, d) = scn.date_ymd().unwrap_or((2026, 11, 7));
    let alt = sun_altitude_deg((y, m, d), t_s, LAT, LON);
    let light = if alt >= 0.0 { 1.0 } else if alt <= -6.0 { night_light } else { night_light + (1.0 - night_light) * (alt + 6.0) / 6.0 };
    EnvState { t_s, open, stall_active, event_active, vehicle_hours: vh, ped_only, light }
}

/// 상태가 바뀐 것마다 사건 하나(C-224 ②).
pub fn env_changes(prev: &EnvState, next: &EnvState) -> Vec<EnvChange> {
    let mut out = Vec::new();
    for i in 0..next.open.len() {
        if prev.open[i] != next.open[i] {
            out.push(EnvChange { kind: if next.open[i] { "open".into() } else { "close".into() }, place: Some(i), value: 0.0 });
        }
    }
    if prev.ped_only != next.ped_only {
        out.push(EnvChange { kind: if next.ped_only { "ped_only_start".into() } else { "ped_only_end".into() }, place: None, value: 0.0 });
    }
    if prev.vehicle_hours != next.vehicle_hours {
        out.push(EnvChange { kind: if next.vehicle_hours { "vehicles_on".into() } else { "vehicles_off".into() }, place: None, value: 0.0 });
    }
    let dark = |l: f64| l < 0.999;
    if dark(prev.light) != dark(next.light) {
        out.push(EnvChange { kind: if dark(next.light) { "dusk".into() } else { "dawn".into() }, place: None, value: next.light });
    }
    out
}

fn day_of_year(y: i32, m: u32, d: u32) -> u32 {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    days[..(m - 1) as usize].iter().sum::<u32>() + d
}

/// 태양 고도(도). NOAA 근사식, 시각은 KST(UTC+9) 초.
pub fn sun_altitude_deg(date: (i32, u32, u32), t_s: f64, lat: f64, lon: f64) -> f64 {
    let (y, m, d) = date;
    let n = day_of_year(y, m, d) as f64;
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let ylen = if leap { 366.0 } else { 365.0 };
    let hour = t_s / 3600.0;
    let tz = 9.0;
    let g = 2.0 * std::f64::consts::PI / ylen * (n - 1.0 + (hour - tz - 12.0) / 24.0);
    let eqtime = 229.18 * (0.000075 + 0.001868 * g.cos() - 0.032077 * g.sin() - 0.014615 * (2.0 * g).cos() - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos() + 0.000907 * (2.0 * g).sin() - 0.002697 * (3.0 * g).cos() + 0.00148 * (3.0 * g).sin();
    let offset = eqtime + 4.0 * lon - 60.0 * tz;
    let tst = hour * 60.0 + offset;
    let ha = (tst / 4.0 - 180.0).to_radians();
    let latr = lat.to_radians();
    let cosz = (latr.sin() * decl.sin() + latr.cos() * decl.cos() * ha.cos()).clamp(-1.0, 1.0);
    90.0 - cosz.acos().to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_altitude_reference_values() {
        // 2026-11-07 서울: 정오 무렵 고도 약 35°, 일몰은 17시 25~35분 무렵.
        let noon = sun_altitude_deg((2026, 11, 7), 12.5 * 3600.0, LAT, LON);
        assert!((30.0..40.0).contains(&noon), "정오 고도 {noon}");
        let s1700 = sun_altitude_deg((2026, 11, 7), 17.0 * 3600.0, LAT, LON);
        let s1800 = sun_altitude_deg((2026, 11, 7), 18.0 * 3600.0, LAT, LON);
        assert!(s1700 > 0.0 && s1800 < 0.0, "17시 {s1700}, 18시 {s1800}");
        let midnight = sun_altitude_deg((2026, 11, 7), 0.0, LAT, LON);
        assert!(midnight < -40.0);
    }

    #[test]
    fn hours_half_open_and_overnight() {
        assert!(in_hours(600.0, 900.0, 600.0));
        assert!(in_hours(600.0, 900.0, 899.99));
        assert!(!in_hours(600.0, 900.0, 900.0));
        assert!(in_hours(1200.0, 120.0, 60.0));
        assert!(!in_hours(1200.0, 120.0, 600.0));
        // 02c 형식의 자정 넘김(close > 1440)
        assert!(in_hours(1080.0, 1560.0, 60.0));
        assert!(in_hours(1080.0, 1560.0, 1100.0));
        assert!(!in_hours(1080.0, 1560.0, 130.0));
    }
}
