//! 시나리오 (02d §2.2). 동적 변수만 담고, 정적 변수는 Python이 번들 파일 값에 반영한다.

use crate::error::{Result, SimError};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DayType {
    Weekday,
    Weekend,
    EventDay,
}

impl DayType {
    pub fn parse(s: &str) -> Option<DayType> {
        match s {
            "weekday" => Some(DayType::Weekday),
            "weekend" => Some(DayType::Weekend),
            "event_day" => Some(DayType::EventDay),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DayType::Weekday => "weekday",
            DayType::Weekend => "weekend",
            DayType::EventDay => "event_day",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventSpec {
    pub place_id: String,
    pub start: String,
    pub end: String,
}

fn d_render() -> u32 {
    2
}
fn d_one() -> f64 {
    1.0
}
fn d_true() -> bool {
    true
}
fn d_trace() -> u32 {
    50
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub date: String,
    pub day_type: String,
    pub window: Window,
    pub dt_s: f64,
    #[serde(default = "d_render")]
    pub render_every_ticks: u32,
    pub seed: u64,
    pub target_peak_concurrency: f64,
    #[serde(default)]
    pub representation_ratio: Option<f64>,
    #[serde(default)]
    pub pedestrian_only: Vec<Window>,
    #[serde(default)]
    pub rent_support_pct: f64,
    #[serde(default)]
    pub events: Vec<EventSpec>,
    #[serde(default = "d_one")]
    pub headway_multiplier: f64,
    #[serde(default = "d_one")]
    pub demand_multiplier: f64,
    #[serde(default = "d_one")]
    pub foreign_inflow_multiplier: f64,
    #[serde(default = "d_one")]
    pub labor_cost_index: f64,
    #[serde(default = "d_true")]
    pub interactions: bool,
    #[serde(default = "d_trace")]
    pub trace_bodies: u32,
    /// 상점 모형 구성: static_stores · policy_experiment · store_adaptation (02c B10).
    #[serde(default)]
    pub configuration: Option<String>,
    #[serde(default)]
    pub months: Option<u32>,
    #[serde(default)]
    pub event_days_per_month: Option<f64>,
    /// Python 덧붙임(02c §2.4 (13)). Rust는 기록만 한다.
    #[serde(default)]
    pub schema_version: Option<serde_norway::Value>,
    /// Python이 파일 값에 이미 반영한 정적 변수(기록용).
    #[serde(default)]
    pub static_applied: Option<serde_norway::Value>,
}

/// "HH:MM" → 분.
pub fn parse_hhmm(s: &str) -> Option<f64> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h > 24 || m > 59 || (h == 24 && m > 0) {
        return None;
    }
    Some((h * 60 + m) as f64)
}

pub fn fmt_hhmm(min: f64) -> String {
    let m = min.round() as i64;
    format!("{:02}:{:02}", m.div_euclid(60), m.rem_euclid(60))
}

impl Scenario {
    pub fn load(path: &Path) -> Result<Scenario> {
        let text = std::fs::read_to_string(path).map_err(|e| SimError::contract(path.display().to_string(), e.to_string()))?;
        let s: Scenario = serde_norway::from_str(&text).map_err(|e| SimError::contract(path.display().to_string(), e.to_string()))?;
        s.validate().map_err(|m| SimError::contract(path.display().to_string(), m))?;
        Ok(s)
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        let (a, b) = self.window_min().ok_or("window의 시각 형식은 HH:MM")?;
        if b <= a {
            return Err("window.end는 window.start보다 늦어야 한다".into());
        }
        if !(self.dt_s > 0.0 && self.dt_s <= 2.0) {
            return Err("dt_s는 (0, 2] 범위".into());
        }
        if self.render_every_ticks == 0 {
            return Err("render_every_ticks는 1 이상".into());
        }
        if !((600.0 / self.dt_s) as u64).is_multiple_of(self.render_every_ticks as u64) {
            return Err("10분 묶음의 틱 수가 render_every_ticks로 나누어떨어져야 한다".into());
        }
        DayType::parse(&self.day_type).ok_or("day_type은 weekday·weekend·event_day")?;
        self.date_ymd().ok_or("date 형식은 YYYY-MM-DD")?;
        if self.target_peak_concurrency < 0.0 {
            return Err("target_peak_concurrency는 0 이상".into());
        }
        if let Some(r) = self.representation_ratio {
            if !(r > 0.0) {
                return Err("representation_ratio는 양수".into());
            }
        }
        for (name, v) in [
            ("headway_multiplier", self.headway_multiplier),
            ("demand_multiplier", self.demand_multiplier),
            ("foreign_inflow_multiplier", self.foreign_inflow_multiplier),
            ("labor_cost_index", self.labor_cost_index),
        ] {
            if !(v >= 0.0 && v.is_finite()) {
                return Err(format!("{name}는 0 이상의 유한한 값"));
            }
        }
        if !(0.0..=100.0).contains(&self.rent_support_pct) {
            return Err("rent_support_pct는 0~100".into());
        }
        for w in &self.pedestrian_only {
            let (s, e) = (parse_hhmm(&w.start).ok_or("pedestrian_only 시각 형식")?, parse_hhmm(&w.end).ok_or("pedestrian_only 시각 형식")?);
            if e <= s {
                return Err("pedestrian_only 구간의 끝은 시작보다 늦어야 한다".into());
            }
        }
        for ev in &self.events {
            let (s, e) = (parse_hhmm(&ev.start).ok_or("events 시각 형식")?, parse_hhmm(&ev.end).ok_or("events 시각 형식")?);
            if e <= s {
                return Err("events 구간의 끝은 시작보다 늦어야 한다".into());
            }
        }
        Ok(())
    }

    pub fn window_min(&self) -> Option<(f64, f64)> {
        Some((parse_hhmm(&self.window.start)?, parse_hhmm(&self.window.end)?))
    }

    /// 창의 (시작, 끝) 초.
    pub fn window_s(&self) -> (f64, f64) {
        let (a, b) = self.window_min().expect("validated");
        (a * 60.0, b * 60.0)
    }

    pub fn day(&self) -> DayType {
        DayType::parse(&self.day_type).expect("validated")
    }

    pub fn date_ymd(&self) -> Option<(i32, u32, u32)> {
        let mut it = self.date.split('-');
        let y = it.next()?.parse().ok()?;
        let m = it.next()?.parse().ok()?;
        let d = it.next()?.parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        Some((y, m, d))
    }

    pub fn n_ticks(&self) -> u64 {
        let (a, b) = self.window_s();
        ((b - a) / self.dt_s).round() as u64
    }

    pub fn ticks_per_chunk(&self) -> u64 {
        (600.0 / self.dt_s).round() as u64
    }

    pub fn pedestrian_only_s(&self) -> Vec<(f64, f64)> {
        self.pedestrian_only.iter().map(|w| (parse_hhmm(&w.start).unwrap() * 60.0, parse_hhmm(&w.end).unwrap() * 60.0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Scenario {
        Scenario {
            id: "t".into(),
            date: "2026-11-07".into(),
            day_type: "weekend".into(),
            window: Window { start: "14:00".into(), end: "17:00".into() },
            dt_s: 0.5,
            render_every_ticks: 2,
            seed: 1,
            target_peak_concurrency: 2000.0,
            representation_ratio: None,
            pedestrian_only: vec![],
            rent_support_pct: 0.0,
            events: vec![],
            headway_multiplier: 1.0,
            demand_multiplier: 1.0,
            foreign_inflow_multiplier: 1.0,
            labor_cost_index: 1.0,
            interactions: true,
            trace_bodies: 50,
            configuration: None,
            months: None,
            event_days_per_month: None,
            schema_version: None,
            static_applied: None,
        }
    }

    #[test]
    fn valid_and_ticks() {
        let s = base();
        assert!(s.validate().is_ok());
        assert_eq!(s.n_ticks(), 21_600);
        assert_eq!(s.ticks_per_chunk(), 1200);
        assert_eq!(fmt_hhmm(14.0 * 60.0 + 5.0), "14:05");
    }

    /// 02c §2.4 (13)의 예시 파일을 그대로 읽는다(큰 u64 시드, 따옴표 없는 날짜, Python 덧붙임 필드).
    #[test]
    fn reads_contract_example() {
        let text = r#"id: discount_high_weekday_r0
schema_version: 1
date: 2026-11-02
day_type: weekday
window: {start: "14:00", end: "17:00"}
dt_s: 0.5
render_every_ticks: 2
seed: 9599481731617930288
target_peak_concurrency: 2000
representation_ratio: null
pedestrian_only: []
rent_support_pct: 0
events: []
headway_multiplier: 1.0
demand_multiplier: 1.0
foreign_inflow_multiplier: 1.0
labor_cost_index: 1.0
interactions: true
trace_bodies: 50
configuration: static_stores
months: 0
event_days_per_month: 0
static_applied: {preset: null, v02_width_delta_m: 0.0, v03_stall_scale: 1.0, v04_added_benches: 0,
                 v05_discount_pct: null, v06_hours_extension_h: 0, v12_income_multiplier: 1.0,
                 v13_competitor_access_scale: 1.0, v15_price_scale: 1.0, v16_visibility_level: base}
"#;
        let s: Scenario = serde_norway::from_str(text).unwrap();
        assert!(s.validate().is_ok());
        assert_eq!(s.seed, 9599481731617930288);
        assert_eq!(s.date, "2026-11-02");
        assert_eq!(s.configuration.as_deref(), Some("static_stores"));
        assert_eq!(s.months, Some(0));
        assert!(s.static_applied.is_some());
        let ev: Scenario = serde_norway::from_str(&text.replace("events: []", "events: [{place_id: \"V:0001\", start: \"15:00\", end: \"17:00\"}]").replace("weekday\n", "event_day\n")).unwrap();
        assert_eq!(ev.events[0].place_id, "V:0001");
        assert_eq!(ev.day(), DayType::EventDay);
    }

    #[test]
    fn invalid_cases() {
        let mut s = base();
        s.window.end = "13:00".into();
        assert!(s.validate().is_err());
        let mut s = base();
        s.day_type = "holiday".into();
        assert!(s.validate().is_err());
        let mut s = base();
        s.render_every_ticks = 7;
        assert!(s.validate().is_err());
        let mut s = base();
        s.rent_support_pct = 120.0;
        assert!(s.validate().is_err());
    }
}
