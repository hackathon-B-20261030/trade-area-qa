//! E211 상점 월 반응(C-211, 02d I-211-r): 월 고리의 기록·단조성·폐업 반영·월 매출 재현·구성 고정·가정 표시.

use mdsim_core::bundle::Bundle;
use mdsim_core::defs::Defs;
use mdsim_core::fixture::{write_fixture, FixturePaths};
use mdsim_core::pq::{col_f64, col_i64, col_str, read_table};
use mdsim_core::scenario::Scenario;
use mdsim_runtime::monthly::{run_monthly, MonthlyOutcome};
use mdsim_runtime::output::{write_monthly, RunInfo, TableStreams};
use mdsim_runtime::shops::{apply_decision, apply_to_space, decide_store, finance_label, MonthObs, ShopParams, Store, StoreDecision};
use mdsim_runtime::space::Space;
use mdsim_runtime::step::World;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

struct Fx {
    paths: FixturePaths,
    defs: Defs,
    bundle: Bundle,
    space: Space,
    scn: Scenario,
}

fn fx() -> &'static Fx {
    static FX: OnceLock<Fx> = OnceLock::new();
    FX.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("mdsim-e211-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = write_fixture(&root).unwrap();
        let defs = Defs::load(&paths.defs).unwrap();
        let bundle = Bundle::load(&paths.bundle_monthly, &paths.root, &defs).unwrap();
        let space = Space::build(&defs, &bundle).unwrap();
        let scn = Scenario::load(&paths.scenario_monthly).unwrap();
        Fx { paths, defs, bundle, space, scn }
    })
}

struct Run {
    out: PathBuf,
    oc: MonthlyOutcome,
}

fn run(name: &str, scn: &Scenario) -> Run {
    let f = fx();
    let out = f.paths.root.join(format!("run-{name}"));
    let _ = std::fs::remove_dir_all(&out);
    let mut streams = TableStreams::new(&out).unwrap();
    let oc = run_monthly(&f.defs, &f.bundle, &f.space, scn, "sha", &out, &mut streams, 4096).unwrap();
    let w = World::new(&f.defs, &f.bundle, &f.space, scn, "sha".into()).unwrap();
    let info = RunInfo { run_id: name.into(), rep: 0, representation_ratio: oc.ratio, n_visit: f.bundle.profiles.len(), frames: BTreeMap::new() };
    write_monthly(&out, &w, &info, streams, BTreeMap::new(), oc.stores.iter().any(|s| s.finance_assumed)).unwrap();
    Run { out, oc }
}

/// 기본 구성(store_adaptation, 3개월)의 실행을 한 번 해서 나눠 쓴다.
fn base() -> &'static Run {
    static R: OnceLock<Run> = OnceLock::new();
    R.get_or_init(|| run("base", &fx().scn))
}

/// 짧은 창(30분) 변형: 정책 실험·스트레스 실험용.
fn short(mut scn: Scenario) -> Scenario {
    scn.window.end = "14:30".into();
    scn
}

/// ① 모든 상점이 매월 하나의 반응과 근거(결정 기록, 난수 키, 대안별 항)를 남긴다.
#[test]
fn e211_1_three_month_records() {
    let r = base();
    let n_stores = r.oc.stores.len();
    assert!(n_stores >= 20, "상점 {n_stores}");
    assert_eq!(r.oc.rows.len(), n_stores * 3, "상점 × 3개월");
    for m in 1..=3u32 {
        let rows: Vec<_> = r.oc.rows.iter().filter(|x| x.month == m).collect();
        assert_eq!(rows.len(), n_stores);
        let decs: Vec<&StoreDecision> = r.oc.decisions.iter().filter(|d| d.month == m).collect();
        let open_rows = rows.iter().filter(|x| x.open).count();
        assert_eq!(decs.len(), open_rows, "{m}달: 영업 상점마다 결정 하나");
        for d in decs {
            assert!(d.rng_key.contains("\"STORE\""), "난수 키 {}", d.rng_key);
            assert!(!d.alts.is_empty());
            let s: f64 = d.alts.iter().map(|a| a.prob).sum();
            assert!((s - 1.0).abs() < 1e-9);
            if m == 1 {
                assert_eq!(d.alts.len(), 1, "첫 달은 기준 달(유지만)");
            }
        }
    }
    // 기록 표: decisions에 STORE 결정, store_months에 행
    let dec = read_table(&r.out.join("decisions.parquet"), "decisions").unwrap();
    let ctx = col_str(&dec, "context", "decisions").unwrap();
    let ids = col_str(&dec, "decision_id", "decisions").unwrap();
    let n_store_dec = ctx.iter().filter(|c| *c == "STORE").count();
    assert_eq!(n_store_dec, r.oc.decisions.len());
    assert!(ids.iter().zip(&ctx).filter(|(_, c)| *c == "STORE").all(|(i, _)| i.starts_with("S:")));
    let sm = read_table(&r.out.join("store_months.parquet"), "store_months").unwrap();
    assert_eq!(sm.num_rows(), n_stores * 3);
}

/// ② 지원율이 오르거나 인건비 지수가 내리면 같은 조건에서 폐업 확률이 오르지 않는다(단위).
#[test]
fn e211_2_support_and_labor_monotone() {
    let f = fx();
    let sp = ShopParams::load(&f.defs.params).unwrap();
    let rng = mdsim_core::rng::KeyRng::new(7);
    let all = ["KEEP", "PRICE_UP", "PRICE_DOWN", "HOURS_UP", "HOURS_DOWN", "STAFF_UP", "STAFF_DOWN", "SWITCH_CATEGORY", "CLOSE"];
    let store = Store { place: 0, id: "S:test".into(), class_ix: 0, closed: false, price: 10000.0, discount: 0.0, open_min: 600.0, close_min: 1260.0, service_rate: 1.0, category: "restaurant".into(), offer: [0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], staff: 3.0, wage: 1.0e6, rent: 2.5e6, finance_assumed: true, r0: 1.0e7, t0: 10000.0 };
    let cats = BTreeMap::new();
    let p_close = |rev_share: f64, support: f64, labor: f64| {
        let obs = MonthObs { revenue: 1.0e7 * rev_share, customers: 1000.0 * rev_share, abandoned: 0.0, revenue_day: [0.0; 3], days: [21, 9, 0], sampled_customers: 100.0 };
        let d = decide_store(&store, &obs, 2, &all, support, labor, &cats, &sp, &rng);
        d.alts.iter().find(|a| a.action == "CLOSE").map(|a| a.prob).unwrap()
    };
    for rev in [0.4, 0.6, 0.8, 1.0] {
        let mut prev = f64::INFINITY;
        for support in [0.0, 10.0, 30.0, 60.0] {
            let p = p_close(rev, support, 1.0);
            assert!(p <= prev + 1e-15, "매출 {rev}, 지원 {support}: {p} > {prev}");
            prev = p;
        }
        let mut prev = f64::INFINITY;
        for labor in [1.5, 1.2, 1.0, 0.8] {
            let p = p_close(rev, 0.0, labor);
            assert!(p <= prev + 1e-15, "매출 {rev}, 인건비 {labor}: {p} > {prev}");
            prev = p;
        }
    }
    // 매출이 크게 줄면 폐업 확률이 실제로 커진다(검사가 무의미하지 않음)
    assert!(p_close(0.4, 0.0, 1.0) > p_close(1.0, 0.0, 1.0));
}

/// ③ 폐업한 상점은 다음 달 대안에서 빠지고, 공간에서도 갈 수 없는 곳이 된다.
#[test]
fn e211_3_closed_excluded() {
    let f = fx();
    // 단위: CLOSE를 반영하면 장소가 닫힌다
    let mut stores = mdsim_runtime::shops::init_stores(&f.space, &f.bundle);
    let sp = ShopParams::load(&f.defs.params).unwrap();
    let s0 = stores[0].clone();
    let d = StoreDecision { month: 2, place: s0.place, place_id: s0.id.clone(), alts: vec![mdsim_runtime::shops::StoreAlt { action: "CLOSE".into(), revenue: 0.0, customers: 0.0, fixed: 0.0, profit: 0.0, cost_burden: 0.0, demand_gap: 0.0, phi_cost: 0.0, phi_demand: 0.0, action_cost: 0.0, utility: 0.0, prob: 1.0 }], chosen: 0, rng_key: String::new(), obs: MonthObs::default(), cost_burden: 0.0, demand_gap: 0.0 };
    apply_decision(&mut stores[0], &d, &BTreeMap::new(), &sp);
    assert!(stores[0].closed);
    let mut space = f.space.clone();
    apply_to_space(&mut space, &stores);
    let pl = &space.places[s0.place as usize];
    assert!(!pl.go_to && !pl.can_buy);
    let env = |t_min: f64| mdsim_runtime::env::env_at(&f.scn, &space, 0.6, t_min * 60.0);
    for t in [600.0, 840.0, 1200.0] {
        assert!(!env(t).open[s0.place as usize], "{t}분에 열려 있음");
    }
    // 통합: 인건비를 크게 올린 스트레스 실행에서 폐업이 생기고, 그 뒤 달에는 결정이 없다
    let mut scn = short(f.scn.clone());
    scn.labor_cost_index = 3.0;
    let r = run("stress", &scn);
    let closed_at: BTreeMap<String, u32> = r.oc.decisions.iter().filter(|d| d.alts[d.chosen].action == "CLOSE").map(|d| (d.place_id.clone(), d.month)).collect();
    assert!(!closed_at.is_empty(), "인건비 3배에서 폐업이 없음");
    for (pid, m) in &closed_at {
        assert!(!r.oc.decisions.iter().any(|d| &d.place_id == pid && d.month > *m), "{pid}: 폐업 뒤에도 결정");
        for row in r.oc.rows.iter().filter(|x| &x.place_id == pid && x.month > *m) {
            assert_eq!(row.action, "CLOSE");
            assert!(!row.open);
            assert_eq!(row.revenue_krw, 0.0, "폐업 뒤 매출");
        }
    }
}

/// ④ 월 매출은 하루 유형별 하루 매출 × 일수이고, 하루 매출은 그날 purchase 사건의 합이다.
#[test]
fn e211_4_monthly_revenue_reproduces() {
    let r = base();
    for row in &r.oc.rows {
        let rebuilt: f64 = (0..3).map(|k| row.revenue_day[k] * row.days[k] as f64).sum();
        assert!((rebuilt - row.revenue_krw).abs() <= 1e-9 * row.revenue_krw.abs().max(1.0), "{} {}달", row.place_id, row.month);
    }
    let ev = read_table(&r.out.join("events.parquet"), "events").unwrap();
    let (kind, place, value) = (col_str(&ev, "kind", "e").unwrap(), col_str(&ev, "place_id", "e").unwrap(), col_f64(&ev, "value", "e").unwrap());
    let (month, day) = (col_i64(&ev, "month", "e").unwrap(), col_str(&ev, "day_type", "e").unwrap());
    let mut day_rev: BTreeMap<(i64, String, String), f64> = BTreeMap::new();
    for i in 0..ev.num_rows() {
        if kind[i] == "purchase" {
            *day_rev.entry((month[i], day[i].clone(), place[i].clone())).or_default() += value[i];
        }
    }
    let dts = ["weekday", "weekend", "event_day"];
    let mut checked = 0;
    for row in &r.oc.rows {
        for (k, dt) in dts.iter().enumerate() {
            if row.days[k] == 0 {
                continue;
            }
            let ev_sum = day_rev.get(&(row.month as i64, dt.to_string(), row.place_id.clone())).copied().unwrap_or(0.0);
            assert!((ev_sum - row.revenue_day[k]).abs() < 1e-6, "{} {}달 {dt}: 사건 {ev_sum} ≠ {}", row.place_id, row.month, row.revenue_day[k]);
            checked += 1;
        }
    }
    assert!(checked > 0);
}

/// ⑤ 정책 실험 구성에서는 가격·영업시간이 상점 반응으로 바뀌지 않는다(역할 u).
#[test]
fn e211_5_policy_experiment_fixed() {
    let f = fx();
    let mut scn = short(f.scn.clone());
    scn.configuration = Some("policy_experiment".into());
    let r = run("policy", &scn);
    for d in &r.oc.decisions {
        assert!(d.alts.iter().all(|a| !a.action.starts_with("PRICE") && !a.action.starts_with("HOURS")), "{}: {:?}", d.place_id, d.alts.iter().map(|a| &a.action).collect::<Vec<_>>());
    }
    let first: BTreeMap<&str, (f64, f64, f64)> = r.oc.rows.iter().filter(|x| x.month == 1).map(|x| (x.place_id.as_str(), (x.price_level_krw, x.open_min, x.close_min))).collect();
    for row in r.oc.rows.iter().filter(|x| x.open) {
        let (p, o, c) = first[row.place_id.as_str()];
        assert_eq!((row.price_level_krw, row.open_min, row.close_min), (p, o, c), "{} {}달", row.place_id, row.month);
    }
}

/// ⑥ 결과에 가정 표시와 계수 출처가 남는다.
#[test]
fn e211_6_assumption_labels() {
    let r = base();
    assert!(r.oc.rows.iter().all(|x| x.assumed), "고정 자료는 상점 재무가 없어 모두 가정");
    assert!(finance_label(&r.oc.stores).starts_with("assumed:shop."));
    let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(r.out.join("manifest.json")).unwrap()).unwrap();
    let flags: Vec<&str> = m["assumption_flags"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(flags.contains(&"store_finance_assumed"), "{flags:?}");
    assert_eq!(m["configuration"], "store_adaptation");
    assert_eq!(m["months"], 3);
    assert!(m["files"]["store_months"]["sha256"].as_str().unwrap().len() == 64);
    let sm = read_table(&r.out.join("store_months.parquet"), "store_months").unwrap();
    for c in ["month", "place_id", "action", "revenue_krw", "cost_burden", "demand_gap", "open", "price_level_krw", "open_min", "close_min", "service_rate_per_min", "category", "assumed"] {
        assert!(sm.column_by_name(c).is_some(), "store_months.{c} (02c §2.6)");
    }
}
