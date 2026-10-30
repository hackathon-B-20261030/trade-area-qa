//! 월 고리 (C-211, 02d I-211-r). 달마다 하루 유형별 하루를 돌려 상점 매출을 모으고,
//! 상점 행위자가 다음 달 상태를 고른다. 같은 입력은 같은 결과를 낸다.

use crate::output::{id_maps, StoreMonthRow, TableStreams};
use crate::record::{EventRow, FileSink, RowsPart};
use crate::shops::{apply_decision, apply_to_space, category_stats, configuration_actions, day_totals, days_in_month, decide_store, init_stores, month_obs, month_seed, set_baseline, DayTotals, ShopParams, Store, StoreDecision, DAY_TYPES};
use crate::space::Space;
use crate::step::{Sim, SimState, World};
use crate::visit::{plan_arrivals, plan_background, representation_ratio, sort_spawns, visit_decisions_streamed, VisitDecision};
use mdsim_core::bundle::Bundle;
use mdsim_core::defs::Defs;
use mdsim_core::scenario::{DayType, Scenario};
use mdsim_core::{Result, SimError};
use std::path::Path;

/// 그림에 쓰는 마지막 달·시나리오 하루 유형의 하루.
pub struct LastDay {
    pub space: Space,
    pub scn: Scenario,
    pub state: SimState,
    pub visit: Vec<VisitDecision>,
    pub events: Vec<EventRow>,
    pub sample: RowsPart,
}

pub struct MonthlyOutcome {
    pub stores: Vec<Store>,
    pub rows: Vec<StoreMonthRow>,
    pub decisions: Vec<StoreDecision>,
    pub ratio: f64,
    pub days: [u32; 3],
    pub last: Option<LastDay>,
    pub bodies: f64,
}

/// 월 고리를 돈다. `streams`에 하루 기록(달·하루 유형 표시)과 상점 기록을 쓴다.
#[allow(clippy::too_many_arguments)]
pub fn run_monthly(defs: &Defs, bundle: &Bundle, base_space: &Space, scn: &Scenario, scenario_sha256: &str, out: &Path, streams: &mut TableStreams, visit_chunk: usize) -> Result<MonthlyOutcome> {
    let months = scn.months.unwrap_or(0);
    if months == 0 {
        return Err(SimError::Run("run_monthly는 months ≥ 1일 때만 부른다".into()));
    }
    let (y, mo, _) = scn.date_ymd().ok_or_else(|| SimError::Run("date 형식".into()))?;
    let days = days_in_month(y, mo, scn.event_days_per_month.unwrap_or(0.0).round().max(0.0) as u32)?;
    let configuration = scn.configuration.clone().unwrap_or_else(|| "static_stores".into());
    let allowed = configuration_actions(&configuration)?;
    let sp = ShopParams::load(&defs.params)?;
    let mut stores = init_stores(base_space, bundle);
    let scenario_day = scn.day();
    // 대표 비율은 첫 달 하루 유형별로 정하고 그 뒤 달에도 그대로 쓴다(상점 변화가 몸 수에 드러나게).
    let mut ratio_by_day: [Option<f64>; 3] = [None; 3];
    let mut rows = Vec::new();
    let mut all_decisions = Vec::new();
    let mut last = None;
    let mut bodies = 0.0;
    let mut memory: Vec<Option<crate::shops::MonthObs>> = vec![None; stores.len()];
    let tmp = out.join(".day");
    for month in 1..=months {
        let mut space_m = base_space.clone();
        apply_to_space(&mut space_m, &stores);
        let mut per_day: [Option<DayTotals>; 3] = [None, None, None];
        for (k, dt) in DAY_TYPES.iter().enumerate() {
            if days[k] == 0 {
                continue;
            }
            let mut scn_d = scn.clone();
            scn_d.day_type = dt.as_str().into();
            scn_d.seed = month_seed(scn.seed, month, *dt);
            if *dt != DayType::EventDay {
                scn_d.events.clear();
            }
            if let Some(r) = ratio_by_day[k] {
                scn_d.representation_ratio = Some(r);
            }
            let w = World::new(defs, bundle, &space_m, &scn_d, scenario_sha256.to_string())?;
            streams.set_tag(Some((month, dt.as_str().into())));
            let visit = visit_decisions_streamed(defs, &space_m, bundle, &scn_d, &w.p, &w.rng, visit_chunk, &mut |c| streams.visit_chunk(&w, c))?;
            let ratio = representation_ratio(&visit, &scn_d, &w.p);
            ratio_by_day[k].get_or_insert(ratio);
            let mut spawns = plan_arrivals(&space_m, bundle, &scn_d, &w.p, &w.rng, &visit, ratio);
            spawns.extend(plan_background(&space_m, bundle, &scn_d, &w.p, &w.rng));
            let spawns = sort_spawns(spawns);
            let _ = std::fs::remove_dir_all(&tmp);
            let mut sink = FileSink::new(&tmp, scn_d.window_s().0, scn_d.dt_s)?;
            sink.write_checkpoints = false;
            sink.write_frames = false;
            let mut sim = Sim::new(&w, &spawns);
            sim.run(&mut sink)?;
            let ids = id_maps(&w, &sim.state);
            let mut events = Vec::new();
            let mut sample = RowsPart::default();
            FileSink::for_each_part(&tmp, &mut |part| {
                streams.runtime_part(&w, &sim.state, &ids, &part)?;
                if sample.decisions.is_empty() {
                    if let Some(d) = part.decisions.iter().find(|d| d.context == "NEXT" && d.n_alts >= 3) {
                        sample.decisions = vec![d.clone()];
                        sample.alts = part.alts.iter().filter(|a| a.decision == d.id).cloned().collect();
                    }
                }
                events.extend(part.events);
                Ok(())
            })?;
            streams.day_end(&w, &sim.state)?;
            bodies += sim.state.bodies.len() as f64;
            per_day[k] = Some(day_totals(&events, space_m.places.len()));
            let _ = std::fs::remove_dir_all(&tmp);
            if month == months && *dt == scenario_day {
                let state = sim.state;
                sample.events = events.clone();
                last = Some(LastDay { space: space_m.clone(), scn: scn_d.clone(), state, visit, events, sample });
            }
        }
        // 상점별 그달 관찰과 결정
        let obs: Vec<_> = stores.iter().map(|s| month_obs(days, &per_day, s.place)).collect();
        if month == 1 {
            for (s, o) in stores.iter_mut().zip(&obs) {
                set_baseline(s, o, &sp);
            }
        }
        // 결정은 기억한 관찰(지수 평균)로 한다(S-E42). 첫 달의 기억은 재무 기준값이다.
        let seen: Vec<_> = obs
            .iter()
            .enumerate()
            .map(|(i, o)| if month == 1 { crate::shops::baseline_memory(&stores[i], o) } else { crate::shops::remembered(memory[i].as_ref(), o, &stores[i], &sp) })
            .collect();
        for (i, o) in seen.iter().enumerate() {
            memory[i] = Some(o.clone());
        }
        let cats = category_stats(&stores, &seen);
        let rng = mdsim_core::rng::KeyRng::new(scn.seed);
        let mut decisions = Vec::new();
        let mut month_rows = Vec::new();
        for ((s, o), seen_o) in stores.iter().zip(&obs).zip(&seen) {
            let (action, prob, cb, dg) = if s.closed {
                ("CLOSE".to_string(), 1.0, 0.0, 0.0)
            } else if allowed.is_empty() {
                let d = decide_store(s, seen_o, month, &["KEEP"], scn.rent_support_pct, scn.labor_cost_index, &cats, &sp, &rng);
                ("KEEP".to_string(), 1.0, d.cost_burden, d.demand_gap)
            } else if month == 1 {
                // 첫 달은 재무의 기준 달이다(02c S-I7): 유지만 하고 근거를 남긴다(S-E42).
                let d = decide_store(s, seen_o, month, &["KEEP"], scn.rent_support_pct, scn.labor_cost_index, &cats, &sp, &rng);
                let out = ("KEEP".to_string(), 1.0, d.cost_burden, d.demand_gap);
                decisions.push(d);
                out
            } else {
                let d = decide_store(s, seen_o, month, allowed, scn.rent_support_pct, scn.labor_cost_index, &cats, &sp, &rng);
                let out = (d.alts[d.chosen].action.clone(), d.alts[d.chosen].prob, d.cost_burden, d.demand_gap);
                decisions.push(d);
                out
            };
            month_rows.push(StoreMonthRow {
                month,
                place_id: s.id.clone(),
                action,
                revenue_krw: o.revenue,
                cost_burden: cb,
                demand_gap: dg,
                open: !s.closed,
                price_level_krw: s.price,
                open_min: s.open_min,
                close_min: s.close_min,
                service_rate_per_min: s.service_rate,
                category: s.category.clone(),
                customers_month: o.customers,
                staff_count: s.staff,
                revenue_day: o.revenue_day,
                days: o.days,
                prob,
                assumed: s.finance_assumed,
            });
        }
        streams.store_month(month, &decisions, &month_rows)?;
        for d in &decisions {
            let s = stores.iter_mut().find(|s| s.place == d.place).expect("상점");
            apply_decision(s, d, &cats, &sp);
        }
        rows.extend(month_rows);
        all_decisions.extend(decisions);
    }
    streams.set_tag(None);
    let ratio = ratio_by_day.iter().flatten().copied().next().unwrap_or(0.0);
    Ok(MonthlyOutcome { stores, rows, decisions: all_decisions, ratio, days, last, bodies })
}
