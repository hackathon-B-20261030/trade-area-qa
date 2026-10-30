//! E228-2 계약 위반 거부(단위): 입력 계약 위반은 시뮬레이션 전에 거부된다.

use mdsim_core::bundle::{Bundle, BundleManifest};
use mdsim_core::defs::Defs;
use mdsim_core::fixture::write_fixture;
use mdsim_core::pq::{file_sha256, read_table, write_batches, ColData, Table};
use mdsim_core::SimError;
use std::path::{Path, PathBuf};

fn fresh(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("mdsim-e228-2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let p = write_fixture(&root).unwrap();
    (root, p.defs, p.bundle)
}

/// 역할의 (첫) 입력 파일 경로.
fn input(root: &Path, bundle: &Path, key: &str) -> PathBuf {
    let m: BundleManifest = serde_json::from_str(&std::fs::read_to_string(bundle.join("manifest.json")).unwrap()).unwrap();
    root.join(&m.files[key][0].path)
}

/// 파일을 바꾼 뒤 manifest의 해시·행 수를 맞춘다.
fn update_sha(root: &Path, bundle: &Path, key: &str) {
    let mpath = bundle.join("manifest.json");
    let mut m: BundleManifest = serde_json::from_str(&std::fs::read_to_string(&mpath).unwrap()).unwrap();
    let entry = &mut m.files.get_mut(key).unwrap()[0];
    let path = root.join(&entry.path);
    entry.sha256 = file_sha256(&path).unwrap();
    entry.rows = Some(read_table(&path, key).unwrap().num_rows() as u64);
    std::fs::write(&mpath, serde_json::to_string_pretty(&m).unwrap()).unwrap();
}

#[test]
fn e228_2_defs_hash_mismatch_is_rejected_with_code_2() {
    let (root, defs_dir, bundle) = fresh("hash");
    let p = defs_dir.join("parameters.yaml");
    let mut s = std::fs::read_to_string(&p).unwrap();
    s.push_str("# 바뀐 정의\n");
    std::fs::write(&p, s).unwrap();
    let defs = Defs::load(&defs_dir).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert!(matches!(err, SimError::Defs(_)), "{err}");
    assert_eq!(err.exit_code(), 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn e228_2_forbidden_person_columns_are_rejected() {
    let (root, defs_dir, bundle) = fresh("age");
    let t = Table::new().col("persona_id", ColData::Str(vec!["x".into()])).col("age", ColData::F64(vec![70.0]));
    t.write_parquet(&input(&root, &bundle, "profiles"), &[]).unwrap();
    update_sha(&root, &bundle, "profiles");
    let defs = Defs::load(&defs_dir).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("age"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn e228_2_dangling_reference_is_rejected() {
    let (root, defs_dir, bundle) = fresh("ref");
    let path = input(&root, &bundle, "nodes");
    let b = read_table(&path, "nodes").unwrap();
    let sliced = b.slice(1, b.num_rows() - 1);
    write_batches(&path, &[sliced], &[]).unwrap();
    update_sha(&root, &bundle, "nodes");
    let defs = Defs::load(&defs_dir).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("nodes") || err.to_string().contains("노드"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn e228_2_file_hash_and_missing_entry_are_rejected() {
    let (root, defs_dir, bundle) = fresh("file");
    let defs = Defs::load(&defs_dir).unwrap();
    // 파일만 바꾸고 manifest는 그대로
    let t = Table::new().col("hour", ColData::I64(vec![14])).col("portal_kind", ColData::Str(vec!["hotel".into()])).col("count", ColData::F64(vec![1.0]));
    t.write_parquet(&input(&root, &bundle, "background_flows"), &[]).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert_eq!(err.exit_code(), 3, "{err}");
    // 필수 항목 삭제
    update_sha(&root, &bundle, "background_flows");
    let mpath = bundle.join("manifest.json");
    let mut m: BundleManifest = serde_json::from_str(&std::fs::read_to_string(&mpath).unwrap()).unwrap();
    m.files.remove("competitors");
    std::fs::write(&mpath, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("competitors"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn e228_2_row_count_and_schema_version_are_checked() {
    let (root, defs_dir, bundle) = fresh("rows");
    let defs = Defs::load(&defs_dir).unwrap();
    let mpath = bundle.join("manifest.json");
    let orig = std::fs::read_to_string(&mpath).unwrap();
    let mut m: BundleManifest = serde_json::from_str(&orig).unwrap();
    m.files.get_mut("places").unwrap()[0].rows = Some(1);
    std::fs::write(&mpath, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert!(err.to_string().contains("행 수"), "{err}");
    let mut m: BundleManifest = serde_json::from_str(&orig).unwrap();
    m.schema_version = 2;
    std::fs::write(&mpath, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert!(err.to_string().contains("schema_version"), "{err}");
    // 두 번째 nodes 파일은 계약 위반(정확히 1개)
    let mut m: BundleManifest = serde_json::from_str(&orig).unwrap();
    let dup = m.files["nodes"][0].clone();
    m.files.get_mut("nodes").unwrap().push(dup);
    std::fs::write(&mpath, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    let err = Bundle::load(&bundle, &root, &defs).unwrap_err();
    assert!(err.to_string().contains("nodes"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn e228_2_missing_parameter_key_is_rejected() {
    let (root, defs_dir, _bundle) = fresh("param");
    let p = defs_dir.join("parameters.yaml");
    let s: String = std::fs::read_to_string(&p).unwrap().lines().filter(|l| !l.contains("look.k_max")).map(|l| format!("{l}\n")).collect();
    std::fs::write(&p, s).unwrap();
    let err = Defs::load(&defs_dir).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(err.to_string().contains("look.k_max"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}
