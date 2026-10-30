//! E228·E225-6·E226-5·E204-4(probe) 통합 테스트: 실제 `mdsim` 실행 파일로 고정 자료를 끝까지 돌린다.
//!
//! 짧은 시나리오(1시간) 실행을 한 번 하고 같은 결과를 여러 검사가 나눠 쓴다(OnceLock).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mdsim")
}

fn mdsim(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().expect("mdsim 실행")
}

struct Fx {
    root: PathBuf,
    defs: String,
    bundle: String,
    scenario: String,
    /// 기준 실행 폴더
    run_a: PathBuf,
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mdsim-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn fx() -> &'static Fx {
    static FX: OnceLock<Fx> = OnceLock::new();
    FX.get_or_init(|| {
        let root = tmp("fx");
        let o = mdsim(&["fixture", "--out", root.to_str().unwrap()]);
        assert!(o.status.success(), "fixture: {}", String::from_utf8_lossy(&o.stderr));
        let j: Value = serde_json::from_slice(&o.stdout).unwrap();
        let s = |k: &str| root.join(j[k].as_str().unwrap()).to_string_lossy().to_string();
        let (defs, bundle, scenario) = (s("defs"), s("bundle_quick"), s("scenario_quick"));
        let run_a = tmp("run-a");
        let o = mdsim(&["run", "--root", root.to_str().unwrap(), "--defs", &defs, "--bundle", &bundle, "--scenario", &scenario, "--out", run_a.to_str().unwrap()]);
        assert!(o.status.success(), "run: {}", String::from_utf8_lossy(&o.stderr));
        Fx { root, defs, bundle, scenario, run_a }
    })
}

fn manifest(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap()
}

fn run_args<'a>(f: &'a Fx, out: &'a str) -> Vec<&'a str> {
    vec!["run", "--root", f.root.to_str().unwrap(), "--defs", &f.defs, "--bundle", &f.bundle, "--scenario", &f.scenario, "--out", out]
}

#[test]
fn e225_6_manifest_fields() {
    let f = fx();
    let m = manifest(&f.run_a);
    for k in ["schema_version", "run_id", "bundle_id", "ontology_sha256", "dimensions_version", "seed", "day_type", "window", "dt_s", "representation_ratio", "interactions", "configuration", "months", "status", "files"] {
        assert!(m.get(k).is_some(), "manifest에 {k} 없음");
    }
    assert_eq!(m["status"], "complete");
    for t in ["agents", "events", "decisions", "decision_alts", "metrics_10min"] {
        let e = &m["files"][t];
        assert!(e["sha256"].as_str().map(|s| s.len() == 64).unwrap_or(false), "{t} 해시");
        assert!(f.run_a.join(e["path"].as_str().unwrap()).exists(), "{t} 파일");
    }
    // 번들 manifest의 정의 해시·번들 ID와 같다(E207-3 교차)
    let bm: Value = serde_json::from_str(&std::fs::read_to_string(Path::new(&f.bundle).join("manifest.json")).unwrap()).unwrap();
    assert_eq!(m["ontology_sha256"], bm["ontology_sha256"]);
    assert_eq!(m["bundle_id"], bm["bundle_id"]);
    assert_eq!(m["seed"], bm["scenarios"][0]["seed"]);
}

#[test]
fn e228_3_rerun_byte_identical() {
    let f = fx();
    let b = tmp("run-b");
    let o = mdsim(&run_args(f, b.to_str().unwrap()));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (ma, mb) = (manifest(&f.run_a), manifest(&b));
    assert_eq!(ma["files"], mb["files"], "같은 입력·시드는 같은 표 바이트");
    assert_eq!(ma["frames"], mb["frames"], "렌더 묶음도 같다");
    for e in std::fs::read_dir(f.run_a.join("plots")).unwrap() {
        let e = e.unwrap();
        let other = b.join("plots").join(e.file_name());
        assert_eq!(std::fs::read(e.path()).unwrap(), std::fs::read(other).unwrap(), "{:?}", e.file_name());
    }
    assert_eq!(std::fs::read(f.run_a.join("report.html")).unwrap(), std::fs::read(b.join("report.html")).unwrap());
    let _ = std::fs::remove_dir_all(b);
}

#[test]
fn e225_3_resume_from_checkpoint_is_identical() {
    let f = fx();
    // 기준 실행 폴더를 복사해 14:30 체크포인트에서 이어 달린다.
    let c = tmp("run-c");
    let o = Command::new("cp").args(["-R", f.run_a.to_str().unwrap(), c.to_str().unwrap()]).output().unwrap();
    assert!(o.status.success());
    let ck = c.join("checkpoints").join("ckpt_1430.json.gz");
    assert!(ck.exists(), "10분 체크포인트");
    let mut args = run_args(f, c.to_str().unwrap());
    let cks = ck.to_string_lossy().to_string();
    args.extend(["--resume", &cks]);
    let o = mdsim(&args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (ma, mc) = (manifest(&f.run_a), manifest(&c));
    assert_eq!(ma["files"], mc["files"], "이어 달린 표가 처음부터 돈 것과 같다");
    assert_eq!(ma["frames"], mc["frames"]);
    let _ = std::fs::remove_dir_all(c);
}

#[test]
fn e226_5_manifest_hash_detects_tampering() {
    let f = fx();
    let d = tmp("run-d");
    assert!(Command::new("cp").args(["-R", f.run_a.to_str().unwrap(), d.to_str().unwrap()]).output().unwrap().status.success());
    let o = mdsim(&["verify", "--run", d.to_str().unwrap()]);
    assert!(o.status.success());
    let p = d.join("events.parquet");
    let mut bytes = std::fs::read(&p).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&p, bytes).unwrap();
    let o = mdsim(&["verify", "--run", d.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&o.stderr).contains("events"));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn e228_1_stage_failure_report() {
    let f = fx();
    // 이미 있는 산출 폴더: 덮어쓰지 않고 종료 코드 5, 단계 args
    let o = mdsim(&run_args(f, f.run_a.to_str().unwrap()));
    assert_eq!(o.status.code(), Some(5));
    let line: Value = serde_json::from_slice(o.stderr.split(|&b| b == b'\n').next().unwrap()).unwrap();
    assert_eq!(line["stage"], "args");
    // 정의가 바뀌면 번들 읽기에서 멈춘다(종료 코드 2, 단계 load_bundle)
    let defs2 = tmp("defs2");
    std::fs::create_dir_all(&defs2).unwrap();
    for e in std::fs::read_dir(&f.defs).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), defs2.join(e.file_name())).unwrap();
    }
    let p = defs2.join("parameters.yaml");
    let mut s = std::fs::read_to_string(&p).unwrap();
    s.push_str("# 바뀐 정의\n");
    std::fs::write(&p, s).unwrap();
    let out = tmp("run-e");
    let o = mdsim(&["run", "--root", f.root.to_str().unwrap(), "--defs", defs2.to_str().unwrap(), "--bundle", &f.bundle, "--scenario", &f.scenario, "--out", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    let line: Value = serde_json::from_slice(o.stderr.split(|&b| b == b'\n').next().unwrap()).unwrap();
    assert_eq!(line["stage"], "load_bundle");
    assert!(line["message"].as_str().unwrap().contains("정의 해시"));
    // 다른 번들의 시나리오는 거부(load_scenario)
    let other = Path::new(&f.scenario).parent().unwrap().parent().unwrap().parent().unwrap();
    let foreign = std::fs::read_dir(other).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.as_path() != Path::new(&f.bundle).parent().unwrap_or(Path::new("")) && p.join("scenarios").exists() && !p.ends_with(Path::new(&f.bundle).file_name().unwrap())).unwrap();
    let foreign_scn = std::fs::read_dir(foreign.join("scenarios")).unwrap().next().unwrap().unwrap().path();
    let o = mdsim(&["run", "--root", f.root.to_str().unwrap(), "--defs", &f.defs, "--bundle", &f.bundle, "--scenario", foreign_scn.to_str().unwrap(), "--out", out.to_str().unwrap()]);
    assert_ne!(o.status.code(), Some(0));
    let line: Value = serde_json::from_slice(o.stderr.split(|&b| b == b'\n').next().unwrap()).unwrap();
    assert_eq!(line["stage"], "load_scenario");
    let _ = std::fs::remove_dir_all(defs2);
}

#[test]
fn e228_4_5_6_report_contents() {
    let f = fx();
    let html = std::fs::read_to_string(f.run_a.join("report.html")).unwrap();
    assert_eq!(html.matches("<svg").count(), 13, "그림 13장 인라인");
    for k in ["시나리오", "번들", "정의 해시", "시드", "몸", "결정"] {
        assert!(html.contains(k), "요약 표의 {k}");
    }
    assert!(html.contains("synthetic_fixture"), "가정 라벨");
    // E228-5: 외부 자원 없음(SVG 이름공간 말고는 http가 없다)
    for name in std::fs::read_dir(f.run_a.join("plots")).unwrap().map(|e| e.unwrap().path()).chain(std::iter::once(f.run_a.join("report.html"))) {
        let s = std::fs::read_to_string(&name).unwrap();
        let stripped = s.replace("http://www.w3.org/2000/svg", "");
        assert!(!stripped.contains("http"), "{name:?}에 외부 참조");
        for bad in ["<script", "<img", "<link", "@import", "url(http"] {
            assert!(!s.contains(bad), "{name:?}에 {bad}");
        }
    }
    // E228-6: 온톨로지 지도에 근거 → 능력 → 차원 사슬이 있다
    let onto = std::fs::read_to_string(f.run_a.join("plots/p12_ontology_map.svg")).unwrap();
    for k in ["근거", "능력", "D-CLIMB", "민감도", "관심"] {
        assert!(onto.contains(k), "온톨로지 지도의 {k}");
    }
    for k in ["나이", "성별", "국적"] {
        assert!(!onto.contains(&format!("{k} →")), "금지 간선 표시 없음");
    }
}

#[test]
fn e204_4_probe_exact_and_deterministic() {
    let f = fx();
    let dir = tmp("probe");
    std::fs::create_dir_all(&dir).unwrap();
    let seg = |grade: f64, len: f64| serde_json::json!({"class": "walk_segment", "dir": "a2b", "amount": 1.0, "attrs": {"grade_pct": grade, "length_m": len, "width_m": 6.0, "walk_mode": "full", "road_type": "residential", "surface": "dry", "ad_intensity": 0.2, "way_difficulty": 0.4, "safe_demand": 0.0}});
    let line = serde_json::json!({
        "probe_id": "steep-vs-flat",
        "persona_id": "P:fx-0001",
        "state": {"fatigue": 0.0, "spent_krw": 0, "remaining_min": 120, "companions": {"stroller": false, "luggage": false, "child": false}},
        "context": "ROUTE",
        "alternatives": [
            {"action": "PROCEED", "purpose": 1.0, "dt_min": 2.0, "exposures": [seg(14.0, 120.0)]},
            {"action": "PROCEED", "target": "route:1", "purpose": 1.0, "dt_min": 5.0, "exposures": [seg(0.0, 300.0)]},
            {"action": "ABANDON", "purpose": 0.0}
        ]
    });
    let inp = dir.join("in.jsonl");
    std::fs::write(&inp, format!("{line}\n")).unwrap();
    let outp = |n: &str| dir.join(n);
    for n in ["a.jsonl", "b.jsonl"] {
        let o = mdsim(&["probe", "--root", f.root.to_str().unwrap(), "--defs", &f.defs, "--bundle", &f.bundle, "--in", inp.to_str().unwrap(), "--out", outp(n).to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let a = std::fs::read_to_string(outp("a.jsonl")).unwrap();
    assert_eq!(a, std::fs::read_to_string(outp("b.jsonl")).unwrap(), "같은 입력 → 같은 출력(난수 없음)");
    let r: Value = serde_json::from_str(a.lines().next().unwrap()).unwrap();
    let alts = r["alts"].as_array().unwrap();
    let sum: f64 = alts.iter().map(|x| x["prob"].as_f64().unwrap()).sum();
    assert!((sum - 1.0).abs() < 1e-12, "확률 합 {sum}");
    assert!(alts[0]["cost_climb"].as_f64().unwrap() > 0.0, "가파른 길은 오르막 부담");
    assert_eq!(alts[1]["cost_climb"].as_f64().unwrap(), 0.0);
    assert!(alts[0]["x"]["D-CLIMB"].as_f64().is_some(), "부담 위치 x");
    assert_eq!(r["ontology_sha256"].as_str().unwrap().len(), 64);
    // 효용 = 항 합(02c §2.6)
    for x in alts {
        let t = x["terms"].as_object().unwrap();
        let s: f64 = t.values().map(|v| v.as_f64().unwrap()).sum();
        assert!((s - x["utility"].as_f64().unwrap()).abs() < 1e-9);
    }
    let _ = std::fs::remove_dir_all(dir);
}
