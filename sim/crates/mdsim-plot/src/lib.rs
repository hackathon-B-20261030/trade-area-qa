//! 명동 보행 시뮬레이션 결과 그림(SVG)과 HTML 보고서 (02d I-228-a, C-228).
//!
//! 런타임이 실행 요약을 이 크레이트의 단순 자료 구조로 옮긴 뒤 그림 함수를 부른다.
//! 이 크레이트는 의존성이 없고 런타임 내부를 모른다.
//!
//! - 그림 함수([`charts`])는 모두 `<svg xmlns="http://www.w3.org/2000/svg"`로 시작하는 독립 SVG 문자열을 돌려준다.
//! - 좌표는 지역 미터다. x는 동쪽, z는 남쪽으로 커지며, SVG의 y = z로 그려 북쪽이 위인 지도가 된다.
//! - 같은 입력은 바이트가 같은 출력을 낸다(E228-3). 외부 자원은 참조하지 않는다(E228-5).
//! - [`report_html`]은 그림을 인라인으로 담은 단일 HTML5 쪽을 만든다.

pub mod charts;
pub mod report;
pub mod svg;

pub use charts::*;
pub use report::{report_html, ReportSection};

/// 보행 격자. 행 우선 배열이며 `data[r * w + c]`가 r행 c열 칸의 값이다.
///
/// 값: 0 = 비어 있음, 1 = 걷기 가능, 2 = 보도·횡단(조금 더 짙게 그림).
/// `origin`은 (0행, 0열) 칸의 최소 모서리 (x, z)이고, 열은 x(동쪽), 행은 z(남쪽)로 커진다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Raster {
    pub origin: (f64, f64),
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// 지도 바탕: 범위 상자, 건물 다각형, 보행 격자, 보행 간선 중심선.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapBase {
    /// (min_x, min_z, max_x, max_z). 크기가 0이거나 유한하지 않으면 그릴 자료의 범위로 대신한다.
    pub bbox: (f64, f64, f64, f64),
    pub buildings: Vec<Vec<(f64, f64)>>,
    pub walk: Option<Raster>,
    pub edges: Vec<Vec<(f64, f64)>>,
}

/// 장소 한 곳(업종 색으로 그림). `label`이 있으면 지도에 이름을 쓴다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacePoint {
    pub pos: (f64, f64),
    pub category: String,
    pub label: Option<String>,
}

/// 출입구 한 곳(종류마다 다른 모양으로 그림).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortalPoint {
    pub pos: (f64, f64),
    pub kind: String,
    pub name: String,
}

/// 이름이 붙은 값 계열.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

/// 값이 붙은 간선 한 개(밀도 지도용).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeValue {
    pub pts: Vec<(f64, f64)>,
    pub value: f64,
}

/// 몸 하나의 궤적과 색 기준 집단.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub pts: Vec<(f64, f64)>,
    pub group: String,
}

/// 렌더 프레임 속 몸 하나의 표지.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    pub pos: (f64, f64),
    /// 진행 방향(라디안, atan2(dz, dx)).
    pub heading: f64,
    /// 시선 방향(라디안, atan2(dz, dx)).
    pub gaze: f64,
    /// 시야 부채꼴 반지름(m).
    pub focal_range: f64,
    /// 시야 부채꼴 전체 각(도). 반각은 이 값의 절반이다.
    pub focal_angle_deg: f64,
    pub group: String,
    /// 보고 있는 대상의 위치. 있으면 점선 시선을 긋는다.
    pub looking_at: Option<(f64, f64)>,
    pub activity: String,
}

/// 스냅샷 한 장: 설명, 몸 표지, 알아차린 장소 위치, 장소.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapshotPanel {
    pub caption: String,
    pub glyphs: Vec<Glyph>,
    pub noticed: Vec<(f64, f64)>,
    pub places: Vec<PlacePoint>,
}

/// 100% 누적 막대의 한 행. 부분 값은 행마다 합이 1이 되도록 맞춘다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackRow {
    pub label: String,
    pub parts: Vec<(String, f64)>,
}

/// 묶음 막대 자료: 범주(가로축)와 계열(범주마다 막대 하나씩).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupedBars {
    pub categories: Vec<String>,
    pub series: Vec<Series>,
}

/// 가로 막대 한 줄. `secondary`는 0~1 비율로 보고 백분율로 쓴다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HBar {
    pub label: String,
    pub value: f64,
    pub secondary: Option<f64>,
}

/// 상자 그림의 한 집단.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dist {
    pub label: String,
    pub values: Vec<f64>,
}

/// 온톨로지 지도의 마디.
///
/// `kind`: evidence, persona, capability, sensitivity, interest, perception, propensity,
/// dimension, interest_dim. 모르는 종류는 가운데 열에 둔다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
}

/// 온톨로지 지도의 연결선(`from`·`to`는 [`GNode::id`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

/// 결정 하나의 대안 막대.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AltBar {
    pub label: String,
    pub prob: f64,
    pub chosen: bool,
}

#[cfg(test)]
mod testutil;
