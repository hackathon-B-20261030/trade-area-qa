//! 합성 예시 자료로 그림 13장(p01~p13)과 보고서를 만들어 지정한 폴더에 쓴다.
//!
//! 사용: `cargo run -p mdsim-plot --example demo -- <출력 폴더>`

#[path = "../tests/common/mod.rs"]
mod common;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("사용법: demo <출력 폴더>");
        return ExitCode::from(2);
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("폴더를 만들 수 없음 {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let figs = common::figures();
    let mut outputs: Vec<(PathBuf, &str)> = figs.iter().map(|f| (dir.join(format!("{}.svg", f.stem)), f.svg.as_str())).collect();
    let report = common::report(&figs);
    outputs.push((dir.join("report.html"), report.as_str()));
    for (path, body) in &outputs {
        if let Err(e) = std::fs::write(path, body) {
            eprintln!("쓸 수 없음 {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("{} ({} 바이트)", path.display(), body.len());
    }
    ExitCode::SUCCESS
}
