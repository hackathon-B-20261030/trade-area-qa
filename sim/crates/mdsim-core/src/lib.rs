//! 명동 에이전트 시뮬레이션 공통 크레이트 (02d I-CORE-a·b).
//!
//! 정의(`ontology/v1/`) 읽기와 식 평가, 02b §6 키드 난수, 평면 기하, 입력 번들(Parquet) 읽기,
//! 실행 기록 쓰기 도우미, 합성 고정 자료를 담는다.

// 값 검사에 `!(x > 0.0)` 꼴을 쓴다: NaN도 함께 거부하려는 뜻이다.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod bundle;
pub mod defs;
pub mod dims;
pub mod error;
pub mod expr;
pub mod fixture;
pub mod geom;
pub mod pq;
pub mod rng;
pub mod scenario;

pub use error::{Result, SimError};
