//! 명동 에이전트 시뮬레이션 런타임 (02d). 공간·환경·반응·결정·지각·이동·step·기록.

// 시뮬레이션 핵심 함수는 상태 조각을 여럿 받는다(구조체로 묶으면 병렬 단계의 빌림이 꼬인다).
// 차원·관심 배열은 같은 인덱스로 여러 배열을 함께 읽는다. 값 검사의 `!(x > 0.0)`은 NaN도 거부하려는 뜻이다.
#![allow(clippy::too_many_arguments, clippy::type_complexity, clippy::needless_range_loop, clippy::neg_cmp_op_on_partial_ord)]

pub mod behavior;
pub mod collective;
pub mod env;
pub mod locomotion;
pub mod monthly;
pub mod output;
pub mod params;
pub mod perception;
pub mod probe;
pub mod react;
pub mod record;
pub mod route;
pub mod shops;
pub mod situation;
pub mod space;
pub mod state;
pub mod step;
pub mod summary;
pub mod visit;
