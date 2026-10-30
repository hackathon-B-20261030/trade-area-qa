//! 결정 하나 평가 (02d I-PROBE, 02c §2.5).
//!
//! 검증(Python I-208-b)이 반복 실행의 잡음 없이 한 결정의 대안별 확률을 얻는 통로다.
//! 노출·제공은 클래스 식으로 계산하고 차원 집계 규칙으로 모아 반응 함수를 부른다.
//! 확률은 정확한 값이라 키드 난수를 쓰지 않는다(선택 표시는 u = 0.5).

use crate::behavior::{caps_from, person};
use crate::params::RtParams;
use crate::react::{burden, react, Alt};
use crate::situation::{aggregate, Exposure};
use crate::space::eval_row;
use mdsim_core::bundle::{Avail, Group, Perception, Profile, Purpose};
use mdsim_core::defs::Defs;
use mdsim_core::dims::{Dim, Interest, ND, NI};
use mdsim_core::scenario::DayType;
use mdsim_core::{Result, SimError};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

fn bad(msg: impl Into<String>) -> SimError {
    SimError::contract("probe", msg)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// profiles.parquet 한 행과 같은 키의 JSON에서 프로필을 만든다(02c §2.4 (1)).
pub fn profile_from_json(v: &Value) -> Result<Profile> {
    let o = v.as_object().ok_or_else(|| bad("profile은 객체여야 한다"))?;
    let f = |k: &str| -> Result<f64> { o.get(k).and_then(num).ok_or_else(|| bad(format!("profile.{k}가 없거나 수가 아님"))) };
    let fd = |k: &str, d: f64| o.get(k).and_then(num).unwrap_or(d);
    let mut comfort = [0.0; ND];
    let mut limit = [0.0; ND];
    let mut sens = [1.0; ND];
    for d in Dim::ALL {
        let k = d.key();
        sens[d.ix()] = fd(&format!("sens_{k}"), 1.0);
        if d == Dim::Time {
            continue;
        }
        comfort[d.ix()] = f(&format!("cap_{k}_comfort"))?;
        limit[d.ix()] = f(&format!("cap_{k}_limit"))?;
        if !(comfort[d.ix()] < limit[d.ix()]) {
            return Err(bad(format!("profile: cap_{k}_comfort ≥ cap_{k}_limit")));
        }
    }
    let mut interest = [0.0; NI];
    for it in Interest::ALL {
        interest[it.ix()] = fd(&format!("interest_{}", it.key()), 0.0);
    }
    let purposes = o
        .get("purposes")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .map(|p| Purpose {
                    purpose: p.get("purpose").and_then(|x| x.as_str()).unwrap_or("other").to_string(),
                    interest: p.get("interest").and_then(|x| x.as_str()).and_then(Interest::parse),
                    value: p.get("value").and_then(num).unwrap_or(1.0),
                    target_place_id: p.get("target_place_id").and_then(|x| x.as_str()).map(|s| s.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();
    let availability = o
        .get("availability")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| Some(Avail { day_type: DayType::parse(a.get("day_type")?.as_str()?)?, start_min: a.get("start_min").and_then(num)?, end_min: a.get("end_min").and_then(num)? }))
                .collect()
        })
        .unwrap_or_default();
    Ok(Profile {
        persona_id: o.get("persona_id").and_then(|x| x.as_str()).unwrap_or("probe").to_string(),
        group: if o.get("group").and_then(|x| x.as_str()) == Some("tourist") { Group::Tourist } else { Group::Domestic },
        origin_id: o.get("origin_id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        comfort,
        limit,
        uncertainty: [0.0; ND],
        cap_label: vec![String::new(); ND],
        sens,
        interest,
        risk: fd("risk", 0.5),
        punctuality: fd("punctuality", 0.5),
        exploration: fd("exploration", 0.5),
        attention: fd("attention", 0.5),
        tau: fd("tau", 0.5),
        base_speed_mps: fd("base_speed_mps", 1.34),
        child_prob: fd("child_prob", 0.0),
        stroller_prob: fd("stroller_prob", 0.0),
        luggage_prob: fd("luggage_prob", 0.0),
        perc: Perception { focal_range_m: fd("focal_range_m", 25.0), focal_angle_deg: fd("focal_angle_deg", 60.0), peripheral_range_m: fd("peripheral_range_m", 10.0), peripheral_angle_deg: fd("peripheral_angle_deg", 150.0), head_turn_deg: fd("head_turn_deg", 90.0), min_dwell_s: fd("min_dwell_s", 1.0) },
        purposes,
        availability,
        evidence: Vec::new(),
    })
}

/// 노출·제공 항목 하나의 속성(수·범주)과 런타임 변수.
fn item_values(item: &Map<String, Value>) -> (BTreeMap<String, f64>, BTreeMap<String, String>, BTreeMap<String, f64>) {
    let mut nums = BTreeMap::new();
    let mut cats = BTreeMap::new();
    if let Some(attrs) = item.get("attrs").and_then(|a| a.as_object()) {
        for (k, v) in attrs {
            match v {
                Value::String(s) => {
                    cats.insert(k.clone(), s.clone());
                }
                other => {
                    if let Some(x) = num(other) {
                        nums.insert(k.clone(), x);
                    }
                }
            }
        }
    }
    let mut rt = BTreeMap::new();
    if let Some(r) = item.get("runtime").and_then(|a| a.as_object()) {
        for (k, v) in r {
            if let Some(x) = num(v) {
                rt.insert(k.clone(), x);
            }
        }
    }
    (nums, cats, rt)
}

fn class_ix(defs: &Defs, item: &Map<String, Value>) -> Result<usize> {
    let c = item.get("class").and_then(|c| c.as_str()).ok_or_else(|| bad("노출·제공 항목에 class가 없음"))?;
    defs.class_ix.get(c).copied().ok_or_else(|| bad(format!("class '{c}'가 정의에 없음")))
}

fn eval_item(defs: &Defs, item: &Map<String, Value>, reverse: bool) -> Result<(mdsim_core::dims::DimVec, mdsim_core::dims::IntVec, Option<f64>)> {
    let ci = class_ix(defs, item)?;
    let (nums, cats, rtv) = item_values(item);
    let rt = |n: &str| -> Option<f64> {
        if let Some(v) = rtv.get(n) {
            return Some(*v);
        }
        Some(match n {
            "vehicle_active" => 0.0,
            "stall_active" | "event_active" | "open" | "light" | "ad_multiplier" => 1.0,
            _ => return None,
        })
    };
    let (press, offer) = eval_row(defs, ci, &nums, &cats, reverse, &rt).map_err(|m| bad(format!("식 평가: {m}")))?;
    // D-CROWD는 식이 아니라 본 밀도(rho)로 들어온다(02c §2.3).
    Ok((press, offer, rtv.get("rho").copied()))
}

/// 상황 하나를 평가해 결과 JSON을 돌려준다.
pub fn evaluate(defs: &Defs, p: &RtParams, prof: &Profile, input: &Value) -> Result<Value> {
    let o = input.as_object().ok_or_else(|| bad("입력 줄은 객체여야 한다"))?;
    let state = o.get("state").and_then(|s| s.as_object()).cloned().unwrap_or_default();
    let sget = |k: &str, d: f64| state.get(k).and_then(num).unwrap_or(d);
    let fatigue = sget("fatigue", 0.0);
    let spent = sget("spent_krw", 0.0);
    let remaining = sget("remaining_min", 180.0);
    let comp = state.get("companions").and_then(|c| c.as_object()).cloned().unwrap_or_default();
    let flag = |k: &str| comp.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let wheels = flag("stroller") || flag("luggage");
    let mut sat = [0.0; NI];
    if let Some(m) = state.get("satiation").and_then(|s| s.as_object()) {
        for (k, v) in m {
            if let (Some(it), Some(x)) = (Interest::parse(k), num(v)) {
                sat[it.ix()] = x;
            }
        }
    }
    let alts_in = o.get("alternatives").and_then(|a| a.as_array()).ok_or_else(|| bad("alternatives가 없음"))?;
    if alts_in.is_empty() {
        return Err(bad("alternatives가 비었음"));
    }
    let mut alts = Vec::with_capacity(alts_in.len());
    for a in alts_in {
        let a = a.as_object().ok_or_else(|| bad("대안은 객체여야 한다"))?;
        let mut ex = Vec::new();
        for item in a.get("exposures").and_then(|e| e.as_array()).map(|v| v.as_slice()).unwrap_or(&[]) {
            let item = item.as_object().ok_or_else(|| bad("노출 항목은 객체여야 한다"))?;
            let reverse = item.get("dir").and_then(|d| d.as_str()) == Some("b2a");
            let (mut press, _, rho) = eval_item(defs, item, reverse)?;
            if let Some(r) = rho {
                press[Dim::Crowd.ix()] = r;
            }
            let amount = item.get("amount").and_then(num).unwrap_or(1.0);
            ex.push(Exposure { demand: press, amount, weight: amount });
        }
        let demand = aggregate(&ex, &defs.dims);
        let mut offer = [0.0; NI];
        for item in a.get("offers").and_then(|e| e.as_array()).map(|v| v.as_slice()).unwrap_or(&[]) {
            let item = item.as_object().ok_or_else(|| bad("제공 항목은 객체여야 한다"))?;
            let (_, off, _) = eval_item(defs, item, false)?;
            let amount = item.get("amount").and_then(num).unwrap_or(1.0);
            for k in 0..NI {
                offer[k] += off[k] * amount;
            }
        }
        alts.push(Alt {
            demand,
            offer,
            purpose: a.get("purpose").and_then(num).unwrap_or(0.0),
            achieve: a.get("achieve").and_then(num).unwrap_or(1.0),
            dt_min: a.get("dt_min").and_then(num).unwrap_or(0.0),
            cost_krw: a.get("cost_krw").and_then(num).unwrap_or(0.0),
            blocked: a.get("blocked").and_then(|b| b.as_bool()).unwrap_or(false),
        });
    }
    let caps = caps_from(defs, prof, fatigue, wheels, spent, remaining);
    let r = react(&person(prof), &caps, &sat, remaining, &alts, &defs.dims, &p.react, 0.5);
    let mut out_alts = Vec::with_capacity(alts.len());
    for (i, a) in alts.iter().enumerate() {
        let t = &r.terms[i];
        let mut row = Map::new();
        row.insert("alt".into(), json!(i));
        row.insert("action".into(), alts_in[i].get("action").cloned().unwrap_or(Value::Null));
        row.insert("target".into(), alts_in[i].get("target").cloned().unwrap_or(Value::Null));
        row.insert("prob".into(), json!(r.probs[i]));
        row.insert("utility".into(), json!(r.utils[i]));
        row.insert("feasible".into(), json!(r.feasible[i]));
        row.insert("purpose".into(), json!(t.purpose));
        row.insert("interest".into(), json!(t.interest_total()));
        row.insert("cost".into(), json!(t.cost_total()));
        row.insert("time".into(), json!(t.time));
        row.insert("money".into(), json!(t.money));
        for d in Dim::ALL {
            row.insert(format!("cost_{}", d.key()), json!(t.cost[d.ix()]));
        }
        let mut x = Map::new();
        for d in Dim::ALL {
            let i = d.ix();
            let pv = a.demand[i] + if d == Dim::Time { a.dt_min } else { 0.0 } + if d == Dim::Money { a.cost_krw } else { 0.0 };
            if pv > 0.0 {
                x.insert(d.id().to_string(), json!(burden(pv, caps.comfort[i], caps.limit[i])));
            }
        }
        row.insert("x".into(), Value::Object(x));
        row.insert("terms".into(), Value::Object(t.signed().into_iter().map(|(k, v)| (k, json!(v))).collect()));
        out_alts.push(Value::Object(row));
    }
    Ok(json!({
        "probe_id": o.get("probe_id").cloned().unwrap_or(Value::Null),
        "context": o.get("context").cloned().unwrap_or(Value::Null),
        "ontology_sha256": defs.sha256,
        "forced": r.forced,
        "chosen_at_u_0_5": r.chosen,
        "alts": out_alts,
    }))
}
