//! 그림 13종(02d I-228-a의 p01~p13).
//!
//! 모든 함수는 `<svg xmlns="http://www.w3.org/2000/svg"`로 시작하는 독립 SVG 문자열 하나를 돌려준다.
//!
//! - 좌표: 지역 미터, x는 동쪽, z는 남쪽으로 커진다. SVG의 y = z로 그려 북쪽이 위인 지도가 된다.
//! - 결정성: 같은 입력이면 바이트가 같은 출력이다. 난수·시각·HashMap 순회를 쓰지 않는다.
//! - 빈 입력·NaN·무한대·크기 0 영역에서도 멈추지 않는다. 자료가 없으면 "자료 없음"을 쓰고
//!   유한하지 않은 값은 건너뛴다.
//! - 색: 범주는 [`CATEGORICAL`] 10색을 고정 순서로 쓰고(넘치면 '기타' 회색으로 묶음), 밀도 등급은
//!   [`ORDINAL_RAMP`]의 한 색상 단계, 부호는 [`POSITIVE_COLOR`]·[`NEGATIVE_COLOR`]를 쓴다.
//! - 지도 안의 `clipPath` id는 그림 종류·제목·자르기 사각형의 해시라서, 같은 id는 같은 사각형을 뜻한다.

use crate::svg::{fixed, fnv1a32, num, text_width, truncate, Attrs, PathData, Svg, INK, SURFACE};
use crate::{
    AltBar, Dist, EdgeValue, GEdge, GNode, Glyph, GroupedBars, HBar, MapBase, PlacePoint, PortalPoint, Raster,
    SnapshotPanel, StackRow, Track,
};
use crate::Series;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

// ============================================================================
// 팔레트와 공통 상수
// ============================================================================

/// 범주 색 10개(고정 순서, 순환하지 않음).
///
/// 앞 8색은 색각 이상 시뮬레이션(Machado 2009)에서 이웃 쌍 구별 기준을 통과한 기준 팔레트이고,
/// 9·10번(청록·갈색)은 같은 검사기로 밝기 대역·채도 하한·이웃 쌍 구별을 확인해 덧붙였다.
/// 범주가 10개를 넘으면 11번째부터 [`OTHER_COLOR`]('기타')로 묶는다.
pub const CATEGORICAL: [&str; 10] = [
    "#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948", "#0e8fa5", "#a3651a",
];

/// 10개를 넘는 범주를 묶는 '기타' 색(회색).
pub const OTHER_COLOR: &str = "#9a9893";

/// 순서형 단계(밝은 파랑 → 짙은 남색). 등급 수에 맞춰 [`ordinal_color`]가 선형 보간한다.
///
/// 2~7등급에서 밝기 단조·이웃 밝기 차(OKLCH ΔL ≥ 0.06)·밝은 끝 대비(2:1 이상)를 만족한다.
pub const ORDINAL_RAMP: [&str; 10] = [
    "#86b6ef", "#6da7ec", "#5598e7", "#3987e5", "#2a78d6", "#256abf", "#1c5cab", "#184f95", "#104281", "#0d366b",
];

/// 양(+) 기여 색(p13).
pub const POSITIVE_COLOR: &str = "#2a78d6";

/// 음(−) 기여 색(p13).
pub const NEGATIVE_COLOR: &str = "#e34948";

/// 자료가 없을 때 쓰는 문구.
pub const NO_DATA: &str = "자료 없음";

/// p13 오른쪽 판에 그리는 기여 항의 최대 개수(절댓값 큰 순).
pub const DECISION_MAX_TERMS: usize = 8;

/// p05에 그리는 스냅샷의 최대 장 수. 넘는 장은 버린다.
pub const SNAPSHOT_MAX_PANELS: usize = 6;

const INK2: &str = "#52514e";
const MUTED: &str = "#898781";
const GRID: &str = "#e1e0d9";
const AXIS: &str = "#c3c2b7";
const NEUTRAL_BAR: &str = "#b9b6ad";
const TRACK_BG: &str = "#e9e7e0";
const WALK_FILL: &str = "#e8e6df";
const SIDEWALK_FILL: &str = "#d9d6cc";
const BUILDING_FILL: &str = "#c9c6bd";
const BUILDING_STROKE: &str = "#b1aea4";
const NETWORK_STROKE: &str = "#a9a69c";
const PORTAL_FILL: &str = "#34332f";
const PLACE_NEUTRAL: &str = "#6e6c66";
const NOTICE_RING: &str = "#e34948";
const GAZE_STROKE: &str = "#52514e";
const HEADING_STROKE: &str = "#2b2a28";
const EDGE_STROKE: &str = "#8f8c84";

const W: f64 = 960.0;
const H: f64 = 600.0;
const PAD: f64 = 24.0;
const TITLE_Y: f64 = 34.0;
const LEGEND_Y: f64 = 48.0;

// ============================================================================
// 색 도우미
// ============================================================================

fn rgb(c: &str) -> (f64, f64, f64) {
    let h = c.trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return (f64::from((v >> 16) & 0xff), f64::from((v >> 8) & 0xff), f64::from(v & 0xff));
        }
    }
    (128.0, 128.0, 128.0)
}

fn hex_color(r: f64, g: f64, b: f64) -> String {
    let q = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", q(r), q(g), q(b))
}

/// 두 색을 sRGB에서 섞는다(t = 0이면 a, 1이면 b).
fn mix(a: &str, b: &str, t: f64) -> String {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    hex_color(ar + (br - ar) * t, ag + (bg - ag) * t, ab + (bb - ab) * t)
}

/// 흰색 쪽으로 옅게 한 색(채움용).
fn tint(c: &str, strength: f64) -> String {
    mix("#ffffff", c, strength)
}

/// `n`등급 가운데 `i`번째(0부터) 순서형 색. 밝은 쪽이 낮은 등급이다.
pub fn ordinal_color(i: usize, n: usize) -> String {
    if n <= 1 {
        return ORDINAL_RAMP[4].to_string();
    }
    let last = ORDINAL_RAMP.len() - 1;
    let p = i.min(n - 1) as f64 * last as f64 / (n - 1) as f64;
    let j = (p.floor() as usize).min(last - 1);
    mix(ORDINAL_RAMP[j], ORDINAL_RAMP[j + 1], p - j as f64)
}

/// 채움색 위 글자색(밝은 채움엔 검정, 어두운 채움엔 흰색).
fn label_ink(fill_color: &str) -> &'static str {
    let (r, g, b) = rgb(fill_color);
    let lin = |v: f64| {
        let c = v / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    if l > 0.36 {
        INK
    } else {
        "#ffffff"
    }
}

/// 이름 → 범주 색. 10개 이하면 모두 고유색, 넘치면 앞 10개만 고유색이고 나머지는 '기타'다.
struct Palette {
    names: Vec<String>,
    index: BTreeMap<String, usize>,
}

impl Palette {
    fn new<I: IntoIterator<Item = String>>(names: I) -> Palette {
        let mut index = BTreeMap::new();
        let mut uniq = Vec::new();
        for n in names {
            if !index.contains_key(&n) {
                index.insert(n.clone(), uniq.len());
                uniq.push(n);
            }
        }
        Palette { names: uniq, index }
    }

    fn color(&self, name: &str) -> &'static str {
        match self.index.get(name) {
            Some(&i) if i < CATEGORICAL.len() => CATEGORICAL[i],
            _ => OTHER_COLOR,
        }
    }

    fn shown(&self) -> &[String] {
        &self.names[..self.names.len().min(CATEGORICAL.len())]
    }

    fn folded(&self) -> usize {
        self.names.len().saturating_sub(CATEGORICAL.len())
    }

    /// 범례 항목(이름별 개수가 있으면 괄호로 붙인다).
    fn items(&self, sw: fn(String) -> Swatch, count: &dyn Fn(&str) -> Option<usize>) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .shown()
            .iter()
            .map(|n| {
                let label = match count(n) {
                    Some(c) => format!("{n} ({c})"),
                    None => n.clone(),
                };
                item(sw(self.color(n).to_string()), label)
            })
            .collect();
        if self.folded() > 0 {
            let rest: Option<usize> = self.names[CATEGORICAL.len()..].iter().map(|n| count(n)).sum();
            let k = self.folded();
            let label = match rest {
                Some(c) => format!("기타 {k}종 ({c})"),
                None => format!("기타 {k}종"),
            };
            items.push(item(sw(OTHER_COLOR.to_string()), label));
        }
        items
    }
}

/// 계열 순번의 색(10개 초과분은 '기타').
fn series_color(i: usize) -> &'static str {
    CATEGORICAL.get(i).copied().unwrap_or(OTHER_COLOR)
}

// ============================================================================
// 숫자 서식과 눈금
// ============================================================================

fn group_digits(s: &str) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s),
    };
    let (int, frac) = match rest.find('.') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let digits: Vec<char> = int.chars().collect();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*ch);
    }
    format!("{sign}{out}{frac}")
}

/// 글자 맨 앞의 '-'를 빼기 기호 '−'(U+2212)로 바꾼다(그림 속 글자용, 좌표 숫자에는 쓰지 않음).
fn minus_sign(s: String) -> String {
    match s.strip_prefix('-') {
        Some(rest) => format!("−{rest}"),
        None => s,
    }
}

/// 값 글자: 1억 이상은 '억', 10만 이상은 '만' 단위, 100 이상은 정수(천 단위 쉼표), 그 밖은 소수 1~2자리.
/// 음수는 '−'(U+2212)로 쓴다.
pub fn fmt_value(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_string();
    }
    let a = v.abs();
    let s = if a >= 1e8 {
        format!("{}억", fixed(v / 1e8, 1))
    } else if a >= 1e5 {
        format!("{}만", fixed(v / 1e4, 1))
    } else if a >= 100.0 {
        group_digits(&fixed(v, 0))
    } else if a >= 10.0 {
        fixed(v, 1)
    } else {
        fixed(v, 2)
    };
    minus_sign(s)
}

/// 0~1 비율을 백분율 글자로(소수 첫째 자리).
fn fmt_pct(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_string();
    }
    let mut s = format!("{:.1}", v * 100.0);
    if s == "-0.0" {
        s = "0.0".to_string();
    }
    minus_sign(format!("{s}%"))
}

/// 부호를 붙인 값 글자(음수는 '−').
fn fmt_signed(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_string();
    }
    let s = fmt_value(v.abs());
    if s == "0" {
        s
    } else if v > 0.0 {
        format!("+{s}")
    } else {
        format!("−{s}")
    }
}

/// 축 눈금: 하한·상한·간격·눈금 값.
#[derive(Clone, Debug, PartialEq)]
pub struct Ticks {
    pub lo: f64,
    pub hi: f64,
    pub step: f64,
    pub values: Vec<f64>,
}

fn snap(v: f64) -> f64 {
    if v.abs() < 1e-12 {
        0.0
    } else {
        v
    }
}

/// 1·2·5 × 10^k 가운데 `raw` 이상인 가장 작은 간격.
pub fn nice_step(raw: f64) -> f64 {
    if !(raw.is_finite() && raw > 0.0) {
        return 1.0;
    }
    let raw = raw.clamp(1e-12, 1e15);
    let base = 10f64.powi(raw.log10().floor() as i32);
    let f = raw / base;
    let m = if f <= 1.0 + 1e-9 {
        1.0
    } else if f <= 2.0 + 1e-9 {
        2.0
    } else if f <= 5.0 + 1e-9 {
        5.0
    } else {
        10.0
    };
    m * base
}

/// [lo, hi]를 덮는 1·2·5 × 10^k 간격의 눈금(약 `target`개).
///
/// 유한하지 않은 범위는 [0, 1], 폭이 0인 범위는 값의 ±50%(값이 0이면 [0, 1])로 넓힌다.
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Ticks {
    let (mut a, mut b) = if lo.is_finite() && hi.is_finite() { (lo.min(hi), lo.max(hi)) } else { (0.0, 1.0) };
    a = a.clamp(-1e12, 1e12);
    b = b.clamp(-1e12, 1e12);
    if b - a < 1e-12 * a.abs().max(1.0) {
        if a.abs() < 1e-12 {
            a = 0.0;
            b = 1.0;
        } else {
            let d = a.abs() * 0.5;
            a -= d;
            b += d;
        }
    }
    let step = nice_step((b - a) / (target.max(2) - 1) as f64);
    let k0 = (a / step + 1e-9).floor();
    let mut k1 = (b / step - 1e-9).ceil();
    if k1 <= k0 {
        k1 = k0 + 1.0;
    }
    let values = ((k0 as i64)..=(k1 as i64)).map(|k| snap(k as f64 * step)).collect();
    Ticks { lo: snap(k0 * step), hi: snap(k1 * step), step, values }
}

/// 0에서 시작하는 두 축의 눈선을 맞춘다. 칸 수 m(3~8)마다 두 축을 1·2·5 간격으로 덮고
/// 남는 빈 공간이 가장 작은 m을 고른다.
fn joint_ticks(max_l: f64, max_r: f64) -> (Ticks, Ticks) {
    let ml = if max_l.is_finite() && max_l > 0.0 { max_l } else { 1.0 };
    let mr = if max_r.is_finite() && max_r > 0.0 { max_r } else { 1.0 };
    let mut best = (f64::INFINITY, 5usize, nice_step(ml / 5.0), nice_step(mr / 5.0));
    for m in 3..=8usize {
        let mf = m as f64;
        let (sl, sr) = (nice_step(ml / mf), nice_step(mr / mf));
        let waste = (1.0 - ml / (mf * sl)) + (1.0 - mr / (mf * sr));
        let score = waste + 0.02 * (mf - 5.0).abs();
        if score < best.0 - 1e-12 {
            best = (score, m, sl, sr);
        }
    }
    let (_, m, sl, sr) = best;
    let mk = |s: f64| Ticks { lo: 0.0, hi: snap(m as f64 * s), step: s, values: (0..=m).map(|k| snap(k as f64 * s)).collect() };
    (mk(sl), mk(sr))
}

#[derive(Clone, Copy, Debug)]
struct Unit {
    div: f64,
    suffix: &'static str,
}

/// 눈금 글자 단위: 눈금 절댓값이 1억 이상이면 '억', 10만 이상이면 '만'.
fn unit_for(t: &Ticks) -> Unit {
    let m = t.lo.abs().max(t.hi.abs());
    if m >= 1e8 {
        Unit { div: 1e8, suffix: "억" }
    } else if m >= 1e5 {
        Unit { div: 1e4, suffix: "만" }
    } else {
        Unit { div: 1.0, suffix: "" }
    }
}

fn decimals_for(step: f64) -> usize {
    if !(step.is_finite() && step > 0.0) {
        return 0;
    }
    for d in 0..=6usize {
        let x = step * 10f64.powi(d as i32);
        if (x - x.round()).abs() < 1e-6 * x.abs().max(1.0) {
            return d;
        }
    }
    6
}

fn fmt_tick(v: f64, step: f64, u: Unit) -> String {
    let x = v / u.div;
    if x.abs() < 1e-12 {
        return "0".to_string();
    }
    minus_sign(format!("{}{}", group_digits(&fixed(x, decimals_for(step / u.div))), u.suffix))
}

fn fmt_tick_pct(v: f64, step: f64) -> String {
    let x = v * 100.0;
    if x.abs() < 1e-9 {
        return "0%".to_string();
    }
    minus_sign(format!("{}%", fixed(x, decimals_for(step * 100.0))))
}

fn tick_label_width(t: &Ticks, u: Unit, size: f64) -> f64 {
    t.values.iter().map(|v| text_width(&fmt_tick(*v, t.step, u), size)).fold(0.0, f64::max)
}

fn min_max<I: IntoIterator<Item = f64>>(it: I) -> Option<(f64, f64)> {
    let mut r: Option<(f64, f64)> = None;
    for v in it {
        if v.is_finite() {
            r = Some(match r {
                None => (v, v),
                Some((a, b)) => (a.min(v), b.max(v)),
            });
        }
    }
    r
}

/// 값이 모두 0 이상이고 최솟값이 최댓값의 절반 이하이면 축을 0부터 시작한다.
fn zero_based(lo: f64, hi: f64) -> f64 {
    if lo >= 0.0 && lo <= 0.5 * hi {
        0.0
    } else {
        lo
    }
}

/// 선형 척도.
#[derive(Clone, Copy, Debug)]
struct Lin {
    d0: f64,
    d1: f64,
    r0: f64,
    r1: f64,
}

impl Lin {
    fn new(d0: f64, d1: f64, r0: f64, r1: f64) -> Lin {
        Lin { d0, d1, r0, r1 }
    }

    fn map(&self, v: f64) -> f64 {
        let span = self.d1 - self.d0;
        if span.abs() < 1e-300 || !span.is_finite() {
            (self.r0 + self.r1) / 2.0
        } else {
            self.r0 + (v - self.d0) / span * (self.r1 - self.r0)
        }
    }
}

// ============================================================================
// 글자·틀·범례
// ============================================================================

fn txt(size: f64, color: &str) -> Attrs {
    Attrs::new().n("font-size", size).s("fill", color)
}

fn txt_end(size: f64, color: &str) -> Attrs {
    txt(size, color).s("text-anchor", "end")
}

fn txt_mid(size: f64, color: &str) -> Attrs {
    txt(size, color).s("text-anchor", "middle")
}

fn bold(a: Attrs) -> Attrs {
    a.s("font-weight", "700")
}

/// 바탕색 테두리를 둘러 지도 위 글자를 읽기 쉽게 한다.
fn halo(a: Attrs) -> Attrs {
    a.s("stroke", SURFACE).n("stroke-width", 3.0).s("stroke-linejoin", "round").s("paint-order", "stroke")
}

fn stroke(color: &str, width: f64) -> Attrs {
    Attrs::new().s("stroke", color).n("stroke-width", width)
}

fn fill(color: &str) -> Attrs {
    Attrs::new().s("fill", color)
}

/// 바탕·`<title>`·제목 글자를 쓴 그림 틀.
fn frame(width: f64, height: f64, title: &str) -> Svg {
    let mut s = Svg::new(width, height, title);
    let t = truncate(title, 18.0, width - 2.0 * PAD);
    s.text(PAD, TITLE_Y, &t, bold(txt(18.0, INK)));
    s
}

fn no_data(s: &mut Svg, cx: f64, cy: f64) {
    s.text(cx, cy, NO_DATA, txt_mid(15.0, INK2));
}

fn empty_figure(title: &str, height: f64) -> String {
    let mut s = frame(W, height, title);
    no_data(&mut s, W / 2.0, height / 2.0 + 10.0);
    s.finish()
}

/// 세로 축 제목(왼쪽은 -90°, 오른쪽은 90° 회전).
fn y_title(s: &mut Svg, x: f64, cy: f64, label: &str, max_len: f64, right: bool) {
    let rot = if right { 90 } else { -90 };
    let t = format!("rotate({rot} {} {})", num(x), num(cy));
    s.text(x, cy, &truncate(label, 12.0, max_len), txt_mid(12.0, INK2).s("transform", &t));
}

#[derive(Clone, Debug)]
enum Swatch {
    Blank,
    Rect(String),
    Dot(String),
    Line(String),
    LineW(String, f64),
    Dash(String),
    Ring(String),
    Wedge(String),
    Tick,
    Square(String),
    Shape(usize),
}

#[derive(Clone, Debug)]
struct Item {
    sw: Swatch,
    label: String,
}

fn item(sw: Swatch, label: impl Into<String>) -> Item {
    Item { sw, label: label.into() }
}

struct Section {
    title: String,
    items: Vec<Item>,
}

const LEG_FS: f64 = 12.0;
const LEG_ROW: f64 = 20.0;
const SW_W: f64 = 16.0;

fn draw_swatch(s: &mut Svg, sw: &Swatch, x: f64, cy: f64) {
    match sw {
        Swatch::Blank => {}
        Swatch::Rect(c) => {
            s.rect(x + 2.0, cy - 6.0, 12.0, 12.0, fill(c).n("rx", 2.0).s("stroke", INK).n("stroke-opacity", 0.12))
        }
        Swatch::Dot(c) => s.circle(x + 8.0, cy, 5.0, fill(c)),
        Swatch::Line(c) => s.line(x, cy, x + SW_W, cy, stroke(c, 2.0).s("stroke-linecap", "round")),
        Swatch::LineW(c, w) => s.line(x + 1.0, cy, x + SW_W - 1.0, cy, stroke(c, *w).s("stroke-linecap", "round")),
        Swatch::Dash(c) => s.line(x, cy, x + SW_W, cy, stroke(c, 1.2).s("stroke-dasharray", "3 2")),
        Swatch::Ring(c) => s.circle(x + 8.0, cy, 5.0, stroke(c, 2.0).s("fill", "none")),
        Swatch::Wedge(c) => {
            let mut p = PathData::new();
            p.m(x + 1.0, cy + 6.0).l(x + 8.5, cy - 7.0).arc(15.0, false, true, x + 15.1, cy + 0.9).z();
            s.path(p.as_str(), fill(c).n("fill-opacity", 0.25).s("stroke", c).n("stroke-opacity", 0.6));
        }
        Swatch::Tick => {
            s.line(x + 4.0, cy, x + 15.0, cy, stroke(HEADING_STROKE, 1.6).s("stroke-linecap", "round"));
            s.circle(x + 4.0, cy, 3.5, fill(MUTED).s("stroke", SURFACE));
        }
        Swatch::Square(c) => s.rect(x + 4.5, cy - 3.5, 7.0, 7.0, fill(c)),
        Swatch::Shape(k) => s.path(&shape_path(*k, x + 8.0, cy, 6.0), fill(PORTAL_FILL)),
    }
}

fn item_width(it: &Item) -> f64 {
    SW_W + 6.0 + text_width(&it.label, LEG_FS) + 18.0
}

/// 가로 범례 배치: 항목마다 (x 오프셋, 줄 번호). 폭을 넘으면 다음 줄로 넘긴다.
fn row_layout(items: &[Item], max_w: f64) -> Vec<(f64, usize)> {
    let mut out = Vec::with_capacity(items.len());
    let (mut x, mut line) = (0.0, 0usize);
    for it in items {
        let w = item_width(it).min(max_w);
        if x > 0.0 && x + w > max_w {
            line += 1;
            x = 0.0;
        }
        out.push((x, line));
        x += w;
    }
    out
}

fn legend_row_height(items: &[Item], max_w: f64) -> f64 {
    row_layout(items, max_w).last().map_or(0.0, |(_, l)| (*l + 1) as f64 * LEG_ROW)
}

fn legend_row(s: &mut Svg, x: f64, y: f64, max_w: f64, items: &[Item]) {
    for (it, (dx, line)) in items.iter().zip(row_layout(items, max_w)) {
        let cy = y + line as f64 * LEG_ROW + LEG_ROW / 2.0;
        draw_swatch(s, &it.sw, x + dx, cy);
        s.text(x + dx + SW_W + 6.0, cy + 4.0, &truncate(&it.label, LEG_FS, max_w - SW_W - 6.0), txt(LEG_FS, INK2));
    }
}

fn legend_col_height(secs: &[Section]) -> f64 {
    secs.iter().map(|sec| 22.0 + sec.items.len() as f64 * LEG_ROW + 10.0).sum()
}

fn legend_col(s: &mut Svg, x: f64, y: f64, w: f64, secs: &[Section]) {
    let mut cy = y;
    for sec in secs {
        s.text(x, cy + 14.0, &truncate(&sec.title, 12.5, w), bold(txt(12.5, INK)));
        cy += 22.0;
        for it in &sec.items {
            let mid = cy + LEG_ROW / 2.0;
            draw_swatch(s, &it.sw, x, mid);
            let tx = if matches!(it.sw, Swatch::Blank) { x } else { x + SW_W + 6.0 };
            s.text(tx, mid + 4.0, &truncate(&it.label, LEG_FS, w - (tx - x)), txt(LEG_FS, INK2));
            cy += LEG_ROW;
        }
        cy += 10.0;
    }
}

// ============================================================================
// 막대 도형
// ============================================================================

/// 세로 막대. 기준선 쪽은 각지고 값 쪽 끝만 둥글다.
fn vbar_path(x: f64, w: f64, y_base: f64, y_end: f64, r: f64) -> String {
    let h = (y_end - y_base).abs();
    let r = r.min(w / 2.0).min(h).max(0.0);
    let mut p = PathData::new();
    if y_end < y_base {
        p.m(x, y_base)
            .v_abs(y_end + r)
            .q(x, y_end, x + r, y_end)
            .h_abs(x + w - r)
            .q(x + w, y_end, x + w, y_end + r)
            .v_abs(y_base)
            .z();
    } else {
        p.m(x, y_base)
            .v_abs(y_end - r)
            .q(x, y_end, x + r, y_end)
            .h_abs(x + w - r)
            .q(x + w, y_end, x + w, y_end - r)
            .v_abs(y_base)
            .z();
    }
    p.as_str().to_string()
}

/// 가로 막대. 기준선 쪽은 각지고 값 쪽 끝만 둥글다.
fn hbar_path(y: f64, h: f64, x_base: f64, x_end: f64, r: f64) -> String {
    let w = (x_end - x_base).abs();
    let r = r.min(h / 2.0).min(w).max(0.0);
    let mut p = PathData::new();
    if x_end > x_base {
        p.m(x_base, y)
            .h_abs(x_end - r)
            .q(x_end, y, x_end, y + r)
            .v_abs(y + h - r)
            .q(x_end, y + h, x_end - r, y + h)
            .h_abs(x_base)
            .z();
    } else {
        p.m(x_base, y)
            .h_abs(x_end + r)
            .q(x_end, y, x_end, y + r)
            .v_abs(y + h - r)
            .q(x_end, y + h, x_end + r, y + h)
            .h_abs(x_base)
            .z();
    }
    p.as_str().to_string()
}

// ============================================================================
// 지도 공통
// ============================================================================

type BBox = (f64, f64, f64, f64);

fn finite_pt(p: (f64, f64)) -> bool {
    p.0.is_finite() && p.1.is_finite()
}

fn bbox_ok(b: BBox) -> bool {
    b.0.is_finite() && b.1.is_finite() && b.2.is_finite() && b.3.is_finite() && b.2 - b.0 > 1e-9 && b.3 - b.1 > 1e-9
}

fn extent<I: IntoIterator<Item = (f64, f64)>>(pts: I) -> Option<BBox> {
    let mut b = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut any = false;
    for (x, z) in pts {
        if x.is_finite() && z.is_finite() {
            any = true;
            b.0 = b.0.min(x);
            b.1 = b.1.min(z);
            b.2 = b.2.max(x);
            b.3 = b.3.max(z);
        }
    }
    if any {
        Some(b)
    } else {
        None
    }
}

/// 유효한 상자는 그대로 쓰고, 아니면 자료 범위(여백 5%)로 대신한다. 한 점뿐이면 둘레 ±10m.
fn resolve_bbox<I: IntoIterator<Item = (f64, f64)>>(b: BBox, fallback: I) -> Option<BBox> {
    if bbox_ok(b) {
        return Some(b);
    }
    let (mut x0, mut z0, mut x1, mut z1) = extent(fallback)?;
    let (w, h) = (x1 - x0, z1 - z0);
    let span = w.max(h);
    if span <= 1e-9 {
        x0 -= 10.0;
        x1 += 10.0;
        z0 -= 10.0;
        z1 += 10.0;
    } else {
        let m = span * 0.05;
        let px = if w <= 1e-9 { span / 2.0 } else { m };
        let pz = if h <= 1e-9 { span / 2.0 } else { m };
        x0 -= px;
        x1 += px;
        z0 -= pz;
        z1 += pz;
    }
    let out = (x0, z0, x1, z1);
    if bbox_ok(out) {
        Some(out)
    } else {
        None
    }
}

fn base_points(base: &MapBase) -> Vec<(f64, f64)> {
    let mut v: Vec<(f64, f64)> = base.buildings.iter().flatten().copied().collect();
    v.extend(base.edges.iter().flatten().copied());
    if let Some(r) = &base.walk {
        if r.w > 0 && r.h > 0 && r.cell.is_finite() && r.cell > 0.0 {
            v.push(r.origin);
            v.push((r.origin.0 + r.w as f64 * r.cell, r.origin.1 + r.h as f64 * r.cell));
        }
    }
    v
}

fn overlaps(a: BBox, b: BBox) -> bool {
    a.0 <= b.2 && a.2 >= b.0 && a.1 <= b.3 && a.3 >= b.1
}

/// 지역 미터 → 화면 좌표. y = z라서 북쪽이 위다.
#[derive(Clone, Copy, Debug)]
struct MapXf {
    min_x: f64,
    min_z: f64,
    scale: f64,
    ox: f64,
    oy: f64,
}

impl MapXf {
    fn pt(&self, p: (f64, f64)) -> (f64, f64) {
        (self.ox + (p.0 - self.min_x) * self.scale, self.oy + (p.1 - self.min_z) * self.scale)
    }
}

#[derive(Clone, Copy, Debug)]
struct MapFit {
    xf: MapXf,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// 상자를 (ax, ay, aw, ah) 영역에 비율을 지켜 맞춘다(가로 가운데, 위쪽 정렬).
fn fit_map(b: BBox, ax: f64, ay: f64, aw: f64, ah: f64) -> MapFit {
    let (bw, bh) = (b.2 - b.0, b.3 - b.1);
    let scale = (aw / bw).min(ah / bh);
    let (w, h) = (bw * scale, bh * scale);
    let x = ax + (aw - w) / 2.0;
    MapFit { xf: MapXf { min_x: b.0, min_z: b.1, scale, ox: x, oy: ay }, x, y: ay, w, h }
}

/// 그림 종류·제목·사각형으로 정하는 결정적 clipPath id.
fn clip_id(kind: &str, title: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    let key = format!("{title}|{}|{}|{}|{}", num(x), num(y), num(w), num(h));
    format!("{kind}-{:08x}", fnv1a32(&key))
}

fn def_clip(s: &mut Svg, id: &str, x: f64, y: f64, w: f64, h: f64) {
    s.open("defs", &Attrs::new());
    s.open("clipPath", &Attrs::new().s("id", id));
    s.rect(x, y, w, h, Attrs::new());
    s.close();
    s.close();
}

fn open_clipped(s: &mut Svg, id: &str) {
    s.open("g", &Attrs::new().s("clip-path", &format!("url(#{id})")));
}

struct BaseStyle {
    raster: bool,
    buildings: bool,
    /// (색, 굵기, 불투명도)
    network: Option<(&'static str, f64, f64)>,
}

fn draw_base(s: &mut Svg, base: &MapBase, xf: &MapXf, view: BBox, st: &BaseStyle) {
    if st.raster {
        if let Some(r) = &base.walk {
            draw_raster(s, r, xf, view);
        }
    }
    if st.buildings {
        draw_buildings(s, &base.buildings, xf, view);
    }
    if let Some((c, w, o)) = st.network {
        let mut p = PathData::coarse();
        for e in &base.edges {
            if extent(e.iter().copied()).is_some_and(|b| overlaps(b, view)) {
                polyline_into(&mut p, e, xf);
            }
        }
        if !p.is_empty() {
            s.path(
                p.as_str(),
                stroke(c, w).n("stroke-opacity", o).s("fill", "none").s("stroke-linecap", "round").s("stroke-linejoin", "round"),
            );
        }
    }
}

fn base_legend(base: &MapBase, network: bool) -> Option<Section> {
    let mut items = Vec::new();
    if base.walk.as_ref().is_some_and(|r| r.w > 0 && r.h > 0) {
        items.push(item(Swatch::Rect(WALK_FILL.into()), "걷기 가능"));
        items.push(item(Swatch::Rect(SIDEWALK_FILL.into()), "보도·횡단"));
    }
    if !base.buildings.is_empty() {
        items.push(item(Swatch::Rect(BUILDING_FILL.into()), "건물"));
    }
    if network && !base.edges.is_empty() {
        items.push(item(Swatch::Line(NETWORK_STROKE.into()), "보행 간선"));
    }
    if items.is_empty() {
        None
    } else {
        Some(Section { title: "바탕".into(), items })
    }
}

/// 격자를 화면 약 2.5px 덩어리로 묶어(다수결) 같은 값 칸을 가로로 잇고, 위아래로 같은 띠를 합쳐 사각형으로 그린다.
const RASTER_BLOCK_PX: f64 = 2.5;
const RASTER_MAX_BLOCKS: usize = 2_000_000;
const RASTER_MAX_CELLS: usize = 64_000_000;

fn draw_raster(s: &mut Svg, r: &Raster, xf: &MapXf, view: BBox) {
    if r.w == 0 || r.h == 0 || !(r.cell.is_finite() && r.cell > 0.0) || !finite_pt(r.origin) {
        return;
    }
    // 실제 자료가 있는 행까지만 본다(없는 칸은 0).
    let h_eff = r.h.min(r.data.len().div_ceil(r.w));
    if h_eff == 0 || r.w.saturating_mul(h_eff) > RASTER_MAX_CELLS {
        return;
    }
    let cell_px = r.cell * xf.scale;
    if !(cell_px.is_finite() && cell_px > 0.0) {
        return;
    }
    let clampu = |v: f64, max: usize| -> usize {
        if v.is_nan() || v <= 0.0 {
            0
        } else if v >= max as f64 {
            max
        } else {
            v as usize
        }
    };
    let c0 = clampu(((view.0 - r.origin.0) / r.cell).floor(), r.w);
    let c1 = clampu(((view.2 - r.origin.0) / r.cell).ceil(), r.w);
    let r0 = clampu(((view.1 - r.origin.1) / r.cell).floor(), h_eff);
    let r1 = clampu(((view.3 - r.origin.1) / r.cell).ceil(), h_eff);
    if c0 >= c1 || r0 >= r1 {
        return;
    }
    let mut k = (RASTER_BLOCK_PX / cell_px).ceil().clamp(1.0, 1e6) as usize;
    while (c1.div_ceil(k) - c0 / k) * (r1.div_ceil(k) - r0 / k) > RASTER_MAX_BLOCKS && k < (1 << 20) {
        k *= 2;
    }
    let (bc0, bc1, br0, br1) = (c0 / k, c1.div_ceil(k), r0 / k, r1.div_ceil(k));
    let ncols = bc1 - bc0;
    let mut open: BTreeMap<(usize, usize, u8), usize> = BTreeMap::new();
    let mut rects: Vec<(usize, usize, usize, usize, u8)> = Vec::new();
    let mut vals = vec![0u8; ncols];
    for br in br0..br1 {
        for (i, v) in vals.iter_mut().enumerate() {
            *v = block_value(r, h_eff, br, bc0 + i, k);
        }
        let mut next = BTreeMap::new();
        let mut i = 0;
        while i < ncols {
            let v = vals[i];
            let mut j = i + 1;
            while j < ncols && vals[j] == v {
                j += 1;
            }
            if v != 0 {
                let key = (bc0 + i, bc0 + j, v);
                let start = open.remove(&key).unwrap_or(br);
                next.insert(key, start);
            }
            i = j;
        }
        for ((a, b, v), start) in std::mem::take(&mut open) {
            rects.push((a, b, start, br, v));
        }
        open = next;
    }
    for ((a, b, v), start) in open {
        rects.push((a, b, start, br1, v));
    }
    rects.sort_unstable();
    for (val, color) in [(1u8, WALK_FILL), (2u8, SIDEWALK_FILL)] {
        let mut p = PathData::coarse();
        for &(a, b, rs, re, v) in &rects {
            if v != val {
                continue;
            }
            let x0 = r.origin.0 + (a * k) as f64 * r.cell;
            let x1 = r.origin.0 + (b * k).min(r.w) as f64 * r.cell;
            let z0 = r.origin.1 + (rs * k) as f64 * r.cell;
            let z1 = r.origin.1 + (re * k).min(h_eff) as f64 * r.cell;
            let (sx0, sy0) = xf.pt((x0, z0));
            let (sx1, sy1) = xf.pt((x1, z1));
            p.m(sx0, sy0).h_abs(sx1).v_abs(sy1).h_abs(sx0).z();
        }
        if !p.is_empty() {
            s.path(p.as_str(), fill(color).s("shape-rendering", "crispEdges"));
        }
    }
}

/// k×k 칸 덩어리의 값: 비어 있지 않은 칸이 절반 이상이면 1·2 중 많은 쪽(같으면 2).
fn block_value(r: &Raster, h_eff: usize, br: usize, bc: usize, k: usize) -> u8 {
    let (rs, re) = (br * k, ((br + 1) * k).min(h_eff));
    let (cs, ce) = (bc * k, ((bc + 1) * k).min(r.w));
    let (mut n1, mut n2, mut total) = (0usize, 0usize, 0usize);
    for row in rs..re {
        let base = row * r.w;
        for col in cs..ce {
            total += 1;
            match r.data.get(base + col) {
                Some(1) => n1 += 1,
                Some(2) => n2 += 1,
                _ => {}
            }
        }
    }
    if total == 0 || (n1 + n2) * 2 < total {
        0
    } else if n2 >= n1 {
        2
    } else {
        1
    }
}

fn draw_buildings(s: &mut Svg, polys: &[Vec<(f64, f64)>], xf: &MapXf, view: BBox) {
    let mut p = PathData::coarse();
    for poly in polys {
        let pts: Vec<(f64, f64)> = poly.iter().copied().filter(|q| finite_pt(*q)).collect();
        if pts.len() < 3 {
            continue;
        }
        if !extent(pts.iter().copied()).is_some_and(|e| overlaps(e, view)) {
            continue;
        }
        let (x, y) = xf.pt(pts[0]);
        p.m(x, y);
        for q in &pts[1..] {
            let (x, y) = xf.pt(*q);
            p.l(x, y);
        }
        p.z();
    }
    if !p.is_empty() {
        s.path(p.as_str(), fill(BUILDING_FILL).s("stroke", BUILDING_STROKE).n("stroke-width", 0.6).s("stroke-linejoin", "round"));
    }
}

/// 꺾은선을 경로에 더한다. 유한하지 않은 점에서 끊는다.
fn polyline_into(p: &mut PathData, pts: &[(f64, f64)], xf: &MapXf) {
    let mut pen = false;
    for q in pts {
        if finite_pt(*q) {
            let (x, y) = xf.pt(*q);
            if pen {
                p.l(x, y);
            } else {
                p.m(x, y);
                pen = true;
            }
        } else {
            pen = false;
        }
    }
}

/// 유한한 점이 이어진 구간들.
fn finite_runs(pts: &[(f64, f64)]) -> Vec<&[(f64, f64)]> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, p) in pts.iter().enumerate() {
        if finite_pt(*p) {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(st) = start.take() {
            out.push(&pts[st..i]);
        }
    }
    if let Some(st) = start {
        out.push(&pts[st..]);
    }
    out
}

/// 직전에 남긴 점에서 `min_d`보다 가까운 점을 버린다(마지막 점은 남긴다).
fn decimate(pts: &[(f64, f64)], min_d: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        let last = i + 1 == pts.len();
        match out.last() {
            Some(q) if !last && (p.0 - q.0).hypot(p.1 - q.1) < min_d => {}
            _ => out.push(*p),
        }
    }
    out
}

/// 더글러스-포이커 단순화(화면 좌표, 허용 오차 `tol` px).
fn simplify(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    let n = pts.len();
    if n <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (ax, ay) = pts[a];
        let (bx, by) = pts[b];
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let mut best = (0.0f64, a);
        for (i, &(px, py)) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d2 = if len2 <= 1e-18 {
                (px - ax).powi(2) + (py - ay).powi(2)
            } else {
                let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                (px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)
            };
            if d2 > best.0 {
                best = (d2, i);
            }
        }
        if best.0 > tol * tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// 출입구 모양: 0 사각형, 1 위 삼각형, 2 마름모, 3 아래 삼각형, 4 오각형, 5 육각형(그 뒤는 반복).
fn shape_path(k: usize, cx: f64, cy: f64, r: f64) -> String {
    let mut p = PathData::new();
    match k % 6 {
        0 => {
            let a = r * 0.82;
            p.m(cx - a, cy - a).h_abs(cx + a).v_abs(cy + a).h_abs(cx - a).z();
        }
        1 => {
            p.m(cx, cy - r).l(cx + r * 0.92, cy + r * 0.72).l(cx - r * 0.92, cy + r * 0.72).z();
        }
        2 => {
            p.m(cx, cy - r).l(cx + r, cy).l(cx, cy + r).l(cx - r, cy).z();
        }
        3 => {
            p.m(cx, cy + r).l(cx + r * 0.92, cy - r * 0.72).l(cx - r * 0.92, cy - r * 0.72).z();
        }
        m => {
            let (sides, start) = if m == 4 { (5, -PI / 2.0) } else { (6, 0.0) };
            for i in 0..sides {
                let a = start + 2.0 * PI * i as f64 / sides as f64;
                let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
                if i == 0 {
                    p.m(x, y);
                } else {
                    p.l(x, y);
                }
            }
            p.z();
        }
    }
    p.as_str().to_string()
}

fn nice_floor(v: f64) -> f64 {
    if !(v.is_finite() && v > 0.0) {
        return 1.0;
    }
    let e = 10f64.powi(v.log10().floor() as i32);
    let f = v / e;
    let m = if f >= 5.0 {
        5.0
    } else if f >= 2.0 {
        2.0
    } else {
        1.0
    };
    m * e
}

fn fmt_len(m: f64) -> String {
    if m >= 1000.0 {
        format!("{} km", fixed(m / 1000.0, 2))
    } else {
        format!("{} m", fixed(m, 2))
    }
}

/// 축척 막대(지도 폭의 약 22%를 1·2·5 길이로).
fn scale_bar(s: &mut Svg, scale: f64, x: f64, y: f64, map_w: f64) {
    if !(scale.is_finite() && scale > 0.0 && map_w.is_finite() && map_w > 0.0) {
        return;
    }
    let m = nice_floor(map_w * 0.22 / scale);
    let px = m * scale;
    let label = fmt_len(m);
    let lw = text_width(&label, 11.0);
    s.rect(x - 6.0, y - 13.0, px + lw + 20.0, 20.0, fill(SURFACE).n("fill-opacity", 0.85).n("rx", 3.0));
    let a = stroke(INK2, 1.5).s("stroke-linecap", "square");
    s.line(x, y, x + px, y, a.clone());
    s.line(x, y - 5.0, x, y, a.clone());
    s.line(x + px, y - 5.0, x + px, y, a);
    s.text(x + px + 6.0, y + 3.5, &label, txt(11.0, INK2));
}

/// 북쪽 표시(화살표 끝이 (x, y)).
fn north_arrow(s: &mut Svg, x: f64, y: f64) {
    s.rect(x - 9.0, y - 3.0, 18.0, 31.0, fill(SURFACE).n("fill-opacity", 0.85).n("rx", 3.0));
    let mut p = PathData::new();
    p.m(x, y).l(x + 5.0, y + 12.0).l(x, y + 9.0).l(x - 5.0, y + 12.0).z();
    s.path(p.as_str(), fill(INK2));
    s.text(x, y + 24.0, "북", txt_mid(10.0, INK2));
}

/// 한 줄짜리 북쪽 표시(화살표 끝이 (x, y - 9), 글자는 왼쪽).
fn north_arrow_inline(s: &mut Svg, x: f64, y: f64) {
    let mut p = PathData::new();
    p.m(x, y - 9.0).l(x + 4.5, y + 2.0).l(x, y - 0.5).l(x - 4.5, y + 2.0).z();
    s.path(p.as_str(), fill(INK2));
    s.text(x - 9.0, y + 2.0, "북", txt_end(10.5, INK2));
}

/// 지도 아래 띠의 높이(축척 막대·북쪽 표시).
const MAP_FOOT: f64 = 34.0;

/// 지도 틀: 테두리, 그리고 지도 아래에 축척 막대(왼쪽)와 북쪽 표시(오른쪽). 지도 위 점을 가리지 않는다.
fn map_chrome(s: &mut Svg, f: &MapFit) {
    s.rect(f.x, f.y, f.w, f.h, stroke(GRID, 1.0).s("fill", "none"));
    scale_bar(s, f.xf.scale, f.x + 6.0, f.y + f.h + 18.0, f.w);
    north_arrow_inline(s, f.x + f.w - 6.0, f.y + f.h + 18.0);
}

/// 지도 위 점의 이름. 지도 사각형 안의 점만 쓰고, 오른쪽 끝을 넘으면 점의 왼쪽에 쓴다.
/// 자르기 묶음 밖에서 부르므로 이름이 지도 테두리에서 잘리지 않는다.
fn point_label(s: &mut Svg, f: &MapFit, at: (f64, f64), gap: f64, label: &str, max_w: f64, strong: bool) {
    let (x, y) = at;
    if !(x >= f.x && x <= f.x + f.w && y >= f.y && y <= f.y + f.h) {
        return;
    }
    let t = truncate(label, 11.0, max_w);
    let a = if strong { bold(txt(11.0, INK)) } else { txt(11.0, INK) };
    if x + gap + text_width(&t, 11.0) > f.x + f.w {
        s.text(x - gap, y + 4.0, &t, halo(a.s("text-anchor", "end")));
    } else {
        s.text(x + gap, y + 4.0, &t, halo(a));
    }
}

/// 지도 그림의 배치: 오른쪽에 범례 열, 왼쪽에 지도.
struct MapLayout {
    legend_x: f64,
    legend_w: f64,
    area: (f64, f64, f64, f64),
}

const MAP_TOP: f64 = 56.0;

fn map_layout() -> MapLayout {
    let legend_w = 200.0;
    let legend_x = W - PAD - legend_w;
    MapLayout { legend_x, legend_w, area: (PAD, MAP_TOP, legend_x - 2.0 * PAD, 620.0) }
}

fn map_height(f: &MapFit, legend_h: f64) -> f64 {
    (f.y + f.h + MAP_FOOT + 8.0).max(MAP_TOP + legend_h + PAD).max(360.0)
}

/// 지도 범위가 없을 때의 그림(범례와 "자료 없음").
fn empty_map(title: &str, lay: &MapLayout, secs: &[Section]) -> String {
    let h = (MAP_TOP + legend_col_height(secs) + PAD).max(400.0);
    let mut s = frame(W, h, title);
    legend_col(&mut s, lay.legend_x, MAP_TOP, lay.legend_w, secs);
    no_data(&mut s, lay.area.0 + lay.area.2 / 2.0, h / 2.0 + 10.0);
    s.finish()
}

// ============================================================================
// p01 공간 지도
// ============================================================================

/// p01: 보행 격자(옅게)·건물(회색)·장소(업종 색, 범례)·출입구(종류별 모양, 범례).
///
/// 업종 색은 장소 수가 많은 업종부터 고정 순서로 준다(같으면 이름순). `label`이 있는 장소와
/// 모든 출입구는 이름을 쓴다.
pub fn space_map(title: &str, base: &MapBase, places: &[PlacePoint], portals: &[PortalPoint]) -> String {
    let fallback = base_points(base)
        .into_iter()
        .chain(places.iter().map(|p| p.pos))
        .chain(portals.iter().map(|p| p.pos));
    let bbox = resolve_bbox(base.bbox, fallback);

    let mut cat_n: BTreeMap<&str, usize> = BTreeMap::new();
    for p in places.iter().filter(|p| finite_pt(p.pos)) {
        *cat_n.entry(p.category.as_str()).or_insert(0) += 1;
    }
    let mut cats: Vec<(&str, usize)> = cat_n.into_iter().collect();
    cats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let counts: BTreeMap<&str, usize> = cats.iter().copied().collect();
    let pal = Palette::new(cats.iter().map(|(n, _)| (*n).to_string()));

    let mut kind_n: BTreeMap<&str, usize> = BTreeMap::new();
    for p in portals.iter().filter(|p| finite_pt(p.pos)) {
        *kind_n.entry(p.kind.as_str()).or_insert(0) += 1;
    }
    let kinds: Vec<(&str, usize)> = kind_n.into_iter().collect();
    let shape_of = |k: &str| kinds.iter().position(|(n, _)| *n == k).unwrap_or(0);

    let mut place_items = pal.items(Swatch::Dot, &|n: &str| counts.get(n).copied());
    if place_items.is_empty() {
        place_items.push(item(Swatch::Blank, NO_DATA));
    }
    let mut secs = vec![Section { title: "장소 업종".into(), items: place_items }];
    if !kinds.is_empty() {
        let items = kinds.iter().enumerate().map(|(i, (n, c))| item(Swatch::Shape(i), format!("{n} ({c})"))).collect();
        secs.push(Section { title: "출입구".into(), items });
    }
    secs.extend(base_legend(base, true));

    let lay = map_layout();
    let Some(b) = bbox else {
        return empty_map(title, &lay, &secs);
    };
    let f = fit_map(b, lay.area.0, lay.area.1, lay.area.2, lay.area.3);
    let height = map_height(&f, legend_col_height(&secs));
    let mut s = frame(W, height, title);
    let id = clip_id("p01", title, f.x, f.y, f.w, f.h);
    def_clip(&mut s, &id, f.x, f.y, f.w, f.h);
    open_clipped(&mut s, &id);
    let net = Some((NETWORK_STROKE, 0.8, 0.8));
    draw_base(&mut s, base, &f.xf, b, &BaseStyle { raster: true, buildings: true, network: net });

    s.open("g", &stroke(SURFACE, 1.0));
    for p in places.iter().filter(|p| finite_pt(p.pos)) {
        let (x, y) = f.xf.pt(p.pos);
        let tip = match &p.label {
            Some(l) => format!("{} · {l}", p.category),
            None => p.category.clone(),
        };
        s.circle_tip(x, y, 3.6, fill(pal.color(&p.category)), Some(&tip));
    }
    s.close();
    for p in portals.iter().filter(|p| finite_pt(p.pos)) {
        let (x, y) = f.xf.pt(p.pos);
        let tip = format!("{} · {}", p.kind, p.name);
        s.path_tip(&shape_path(shape_of(&p.kind), x, y, 6.5), fill(PORTAL_FILL).s("stroke", SURFACE).n("stroke-width", 1.5), Some(&tip));
    }
    s.close();
    for p in places.iter().filter(|p| finite_pt(p.pos)) {
        if let Some(l) = &p.label {
            point_label(&mut s, &f, f.xf.pt(p.pos), 6.0, l, 140.0, false);
        }
    }
    for p in portals.iter().filter(|p| finite_pt(p.pos)) {
        point_label(&mut s, &f, f.xf.pt(p.pos), 9.0, &p.name, 160.0, true);
    }
    map_chrome(&mut s, &f);
    legend_col(&mut s, lay.legend_x, MAP_TOP, lay.legend_w, &secs);
    s.finish()
}

// ============================================================================
// p02 막대 + 선
// ============================================================================

/// p02: 막대(왼쪽 축) + 선(오른쪽 축). 예: 10분 도착 수 막대와 동시 체류 인원 선.
///
/// 두 계열이 모두 0 이상이면 두 축의 눈선을 맞춘다(같은 칸 수, 각 축은 1·2·5 간격).
/// 선의 최댓값에 점과 값 글자를 단다. 유한하지 않은 값은 막대를 빼고 선을 끊는다.
pub fn bar_line_chart(
    title: &str,
    x_labels: &[String],
    bars: &Series,
    line: &Series,
    y_label_bars: &str,
    y_label_line: &str,
) -> String {
    let n = x_labels.len().max(bars.values.len()).max(line.values.len());
    let pick = |v: &[f64], i: usize| v.get(i).copied().filter(|x| x.is_finite());
    let bv: Vec<Option<f64>> = (0..n).map(|i| pick(&bars.values, i)).collect();
    let lv: Vec<Option<f64>> = (0..n).map(|i| pick(&line.values, i)).collect();
    let has_b = bv.iter().any(Option::is_some);
    let has_l = lv.iter().any(Option::is_some);
    let (bmin, bmax) = min_max(bv.iter().flatten().copied()).unwrap_or((0.0, 0.0));
    let (lmin, lmax) = min_max(lv.iter().flatten().copied()).unwrap_or((0.0, 0.0));
    let aligned = bmin >= 0.0 && lmin >= 0.0;
    let (lt, rt) = if aligned {
        joint_ticks(bmax, lmax)
    } else {
        (nice_ticks(bmin.min(0.0), bmax.max(0.0), 6), nice_ticks(lmin.min(0.0), lmax.max(0.0), 6))
    };
    let (lu, ru) = (unit_for(&lt), unit_for(&rt));
    let x0 = PAD + 22.0 + tick_label_width(&lt, lu, 11.5) + 10.0;
    let x1 = W - (PAD + 22.0 + tick_label_width(&rt, ru, 11.5) + 10.0);
    let bars_name = if bars.name.is_empty() { "막대" } else { bars.name.as_str() };
    let line_name = if line.name.is_empty() { "선" } else { line.name.as_str() };
    let items = vec![
        item(Swatch::Rect(CATEGORICAL[0].into()), format!("{bars_name} (왼쪽 축)")),
        item(Swatch::Line(CATEGORICAL[1].into()), format!("{line_name} (오른쪽 축)")),
    ];
    let y0 = LEGEND_Y + legend_row_height(&items, x1 - x0) + 18.0;
    let y1 = H - 56.0;
    let ys_l = Lin::new(lt.lo, lt.hi, y1, y0);
    let ys_r = Lin::new(rt.lo, rt.hi, y1, y0);

    let mut s = frame(W, H, title);
    legend_row(&mut s, x0, LEGEND_Y, x1 - x0, &items);
    for v in &lt.values {
        let y = ys_l.map(*v);
        s.line(x0, y, x1, y, stroke(GRID, 1.0));
        s.text(x0 - 8.0, y + 4.0, &fmt_tick(*v, lt.step, lu), txt_end(11.5, INK2));
    }
    for v in &rt.values {
        let y = ys_r.map(*v);
        if !aligned {
            s.line(x1, y, x1 + 4.0, y, stroke(AXIS, 1.0));
        }
        s.text(x1 + 8.0, y + 4.0, &fmt_tick(*v, rt.step, ru), txt(11.5, INK2));
    }
    y_title(&mut s, PAD + 6.0, (y0 + y1) / 2.0, y_label_bars, y1 - y0, false);
    y_title(&mut s, W - PAD - 6.0, (y0 + y1) / 2.0, y_label_line, y1 - y0, true);
    let yb = ys_l.map(0.0);
    s.line(x0, yb, x1, yb, stroke(AXIS, 1.0));
    if n == 0 || (!has_b && !has_l) {
        no_data(&mut s, (x0 + x1) / 2.0, (y0 + y1) / 2.0);
        return s.finish();
    }

    let band = (x1 - x0) / n as f64;
    let cx = |i: usize| x0 + band * (i as f64 + 0.5);
    let label_at = |i: usize| match x_labels.get(i) {
        Some(l) if !l.is_empty() => l.clone(),
        _ => format!("{}번째", i + 1),
    };
    let lbl_w = x_labels.iter().map(|l| text_width(l, 11.0)).fold(0.0, f64::max);
    let every = (((lbl_w + 8.0) / band).ceil().max(1.0)) as usize;
    for (i, l) in x_labels.iter().enumerate() {
        if i.is_multiple_of(every) {
            s.text(cx(i), y1 + 18.0, &truncate(l, 11.0, band * every as f64 - 4.0), txt_mid(11.0, INK2));
        }
    }

    let bw = (band * 0.72).clamp(1.0, 24.0);
    s.open("g", &fill(CATEGORICAL[0]));
    for (i, v) in bv.iter().enumerate() {
        let Some(v) = *v else { continue };
        let ye = ys_l.map(v);
        if (ye - yb).abs() < 0.01 {
            continue;
        }
        let tip = format!("{}: {bars_name} {}", label_at(i), fmt_value(v));
        s.path_tip(&vbar_path(cx(i) - bw / 2.0, bw, yb, ye, 4.0), Attrs::new(), Some(&tip));
    }
    s.close();

    let mut p = PathData::new();
    let mut pen = false;
    for (i, v) in lv.iter().enumerate() {
        match v {
            Some(v) => {
                let (x, y) = (cx(i), ys_r.map(*v));
                if pen {
                    p.l(x, y);
                } else {
                    p.m(x, y);
                    pen = true;
                }
            }
            None => pen = false,
        }
    }
    let line_color = CATEGORICAL[1];
    if !p.is_empty() {
        s.path(p.as_str(), stroke(line_color, 2.0).s("fill", "none").s("stroke-linejoin", "round").s("stroke-linecap", "round"));
    }
    // 이웃이 없는 외톨이 점은 작은 점으로 보이게 한다.
    for (i, v) in lv.iter().enumerate() {
        let Some(v) = *v else { continue };
        let lonely = (i == 0 || lv[i - 1].is_none()) && (i + 1 == n || lv[i + 1].is_none());
        if lonely {
            s.circle(cx(i), ys_r.map(v), 2.5, fill(line_color));
        }
    }
    // 마우스를 올리면 값을 보여주는 투명한 점
    s.open("g", &fill("#000000").n("fill-opacity", 0.0));
    for (i, v) in lv.iter().enumerate() {
        let Some(v) = *v else { continue };
        let tip = format!("{}: {line_name} {}", label_at(i), fmt_value(v));
        s.circle_tip(cx(i), ys_r.map(v), 7.0, Attrs::new(), Some(&tip));
    }
    s.close();
    let peak = lv
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .fold(None, |acc: Option<(usize, f64)>, (i, v)| match acc {
            Some((_, m)) if m >= v => acc,
            _ => Some((i, v)),
        });
    if let Some((i, v)) = peak {
        let (x, y) = (cx(i), ys_r.map(v));
        s.circle(x, y, 4.5, fill(line_color).s("stroke", SURFACE).n("stroke-width", 2.0));
        let lbl = format!("최대 {}", fmt_value(v));
        let half = text_width(&lbl, 11.5) / 2.0;
        let lx = if x0 + half <= x1 - half { x.clamp(x0 + half, x1 - half) } else { x };
        s.text(lx, y - 10.0, &lbl, halo(bold(txt_mid(11.5, INK))));
    }
    s.finish()
}

// ============================================================================
// p03 밀도 지도
// ============================================================================

/// 값의 등급: `thresholds` 가운데 값 이하인 것의 개수.
///
/// 정렬된 기준 [t0, t1, …]에 대해 v < t0 → 0, t0 ≤ v < t1 → 1, …, v ≥ 마지막 → 기준 개수.
/// 기준값과 같으면 위 등급이다. 유한하지 않은 기준은 무시한다.
pub fn density_class(value: f64, thresholds: &[f64]) -> usize {
    thresholds.iter().filter(|t| t.is_finite() && value >= **t).count()
}

fn class_range(k: usize, th: &[f64]) -> String {
    if th.is_empty() {
        "전체".to_string()
    } else if k == 0 {
        format!("< {}", fmt_value(th[0]))
    } else if k >= th.len() {
        format!("≥ {}", fmt_value(th[th.len() - 1]))
    } else {
        format!("{}–{}", fmt_value(th[k - 1]), fmt_value(th[k]))
    }
}

/// p03: 간선을 밀도 등급 색으로 그린 지도(예: 동시 인원 최대 10분 프레임, Fruin LOS A–F).
///
/// 등급은 [`density_class`]로 정하고, 색은 [`ordinal_color`](밝음 → 짙음), 굵기는 등급마다 굵어진다.
/// `legend`는 `thresholds.len() + 1`개의 등급 이름이다(모자라면 "등급 n").
pub fn density_map(title: &str, base: &MapBase, edges: &[EdgeValue], thresholds: &[f64], legend: &[&str]) -> String {
    let mut th: Vec<f64> = thresholds.iter().copied().filter(|t| t.is_finite()).collect();
    th.sort_by(f64::total_cmp);
    let ncls = th.len() + 1;
    let colors: Vec<String> = (0..ncls).map(|k| ordinal_color(k, ncls)).collect();
    let width_of = |k: usize| 1.4 + 2.6 * k as f64 / (ncls.max(2) - 1) as f64;
    let valid: Vec<(&EdgeValue, usize)> = edges
        .iter()
        .filter(|e| e.value.is_finite() && e.pts.iter().filter(|p| finite_pt(**p)).count() >= 2)
        .map(|e| (e, density_class(e.value, &th)))
        .collect();
    let mut counts = vec![0usize; ncls];
    for (_, k) in &valid {
        counts[*k] += 1;
    }
    let fallback = base_points(base).into_iter().chain(valid.iter().flat_map(|(e, _)| e.pts.iter().copied()));
    let bbox = resolve_bbox(base.bbox, fallback);

    let items = (0..ncls)
        .map(|k| {
            let name = legend.get(k).map(|s| s.to_string()).unwrap_or_else(|| format!("등급 {}", k + 1));
            let range = class_range(k, &th);
            item(Swatch::LineW(colors[k].clone(), width_of(k)), format!("{name}  {range}  ({})", counts[k]))
        })
        .collect();
    let mut secs = vec![Section { title: "밀도 등급 (간선 수)".into(), items }];
    secs.extend(base_legend(base, false));

    let lay = map_layout();
    let Some(b) = bbox else {
        return empty_map(title, &lay, &secs);
    };
    let f = fit_map(b, lay.area.0, lay.area.1, lay.area.2, lay.area.3);
    let height = map_height(&f, legend_col_height(&secs));
    let mut s = frame(W, height, title);
    let id = clip_id("p03", title, f.x, f.y, f.w, f.h);
    def_clip(&mut s, &id, f.x, f.y, f.w, f.h);
    open_clipped(&mut s, &id);
    let net = Some((NETWORK_STROKE, 0.6, 0.5));
    draw_base(&mut s, base, &f.xf, b, &BaseStyle { raster: true, buildings: true, network: net });
    for (k, color) in colors.iter().enumerate() {
        let mut p = PathData::coarse();
        for (e, c) in &valid {
            if *c == k && extent(e.pts.iter().copied()).is_some_and(|eb| overlaps(eb, b)) {
                polyline_into(&mut p, &e.pts, &f.xf);
            }
        }
        if !p.is_empty() {
            let a = stroke(color, width_of(k))
                .s("fill", "none")
                .s("stroke-linecap", "round")
                .s("stroke-linejoin", "round")
                .s("data-class", &k.to_string());
            s.path(p.as_str(), a);
        }
    }
    s.close();
    map_chrome(&mut s, &f);
    if valid.is_empty() {
        no_data(&mut s, f.x + f.w / 2.0, f.y + f.h / 2.0);
    }
    legend_col(&mut s, lay.legend_x, MAP_TOP, lay.legend_w, &secs);
    s.finish()
}

// ============================================================================
// p04 궤적
// ============================================================================

/// p04: 몸마다의 궤적(집단 색, 가늘고 반투명). 출발 지점에 작은 점을 찍는다.
///
/// 집단 색은 집단 이름순으로 고정 순서 색을 준다(10개 초과는 '기타').
/// 궤적은 화면 좌표에서 0.6px 이내 점을 줄이고 0.4px 허용 오차로 단순화한다.
pub fn trajectories(title: &str, base: &MapBase, tracks: &[Track]) -> String {
    let fallback = base_points(base).into_iter().chain(tracks.iter().flat_map(|t| t.pts.iter().copied()));
    let bbox = resolve_bbox(base.bbox, fallback);
    let mut gcount: BTreeMap<&str, usize> = BTreeMap::new();
    for t in tracks.iter().filter(|t| t.pts.iter().any(|p| finite_pt(*p))) {
        *gcount.entry(t.group.as_str()).or_insert(0) += 1;
    }
    let pal = Palette::new(gcount.keys().map(|k| (*k).to_string()));
    let mut items = pal.items(Swatch::Line, &|n: &str| gcount.get(n).copied());
    if items.is_empty() {
        items.push(item(Swatch::Blank, NO_DATA));
    }
    let mut secs = vec![
        Section { title: "집단 (궤적 수)".into(), items },
        Section { title: "표시".into(), items: vec![item(Swatch::Dot(INK2.into()), "출발 지점")] },
    ];
    secs.extend(base_legend(base, true));

    let lay = map_layout();
    let Some(b) = bbox else {
        return empty_map(title, &lay, &secs);
    };
    let f = fit_map(b, lay.area.0, lay.area.1, lay.area.2, lay.area.3);
    let height = map_height(&f, legend_col_height(&secs));
    let mut s = frame(W, height, title);
    let id = clip_id("p04", title, f.x, f.y, f.w, f.h);
    def_clip(&mut s, &id, f.x, f.y, f.w, f.h);
    open_clipped(&mut s, &id);
    let net = Some((NETWORK_STROKE, 0.6, 0.45));
    draw_base(&mut s, base, &f.xf, b, &BaseStyle { raster: true, buildings: true, network: net });
    let mut any = false;
    s.open(
        "g",
        &Attrs::new()
            .s("fill", "none")
            .n("stroke-width", 1.3)
            .n("stroke-opacity", 0.55)
            .s("stroke-linejoin", "round")
            .s("stroke-linecap", "round"),
    );
    for t in tracks {
        let color = pal.color(&t.group);
        for run in finite_runs(&t.pts) {
            let screen: Vec<(f64, f64)> = run.iter().map(|p| f.xf.pt(*p)).collect();
            let pts = simplify(&decimate(&screen, 0.6), 0.4);
            if pts.len() >= 2 {
                any = true;
                s.polyline(&pts, Attrs::new().s("stroke", color));
            }
        }
    }
    s.close();
    s.open("g", &stroke(SURFACE, 0.8));
    for t in tracks {
        if let Some(p) = t.pts.iter().find(|p| finite_pt(**p)) {
            any = true;
            let (x, y) = f.xf.pt(*p);
            s.circle(x, y, 2.4, fill(pal.color(&t.group)));
        }
    }
    s.close();
    s.close();
    map_chrome(&mut s, &f);
    if !any {
        no_data(&mut s, f.x + f.w / 2.0, f.y + f.h / 2.0);
    }
    legend_col(&mut s, lay.legend_x, MAP_TOP, lay.legend_w, &secs);
    s.finish()
}

// ============================================================================
// p05 스냅샷 띠
// ============================================================================

const PANEL_MAX_W: f64 = 300.0;
const PANEL_MAX_H: f64 = 340.0;
const PANEL_GAP: f64 = 16.0;
const PANEL_CAPTION: f64 = 26.0;

/// 시야 부채꼴 경로(중심 `center` 라디안, 반각 `half`, 반지름 `r` px). 전체 원이면 원을 그린다.
fn wedge_path(cx: f64, cy: f64, r: f64, center: f64, half: f64) -> String {
    let mut p = PathData::new();
    if half * 2.0 >= 2.0 * PI - 1e-6 {
        p.m(cx + r, cy).arc(r, false, true, cx - r, cy).arc(r, false, true, cx + r, cy).z();
    } else {
        let (a0, a1) = (center - half, center + half);
        p.m(cx, cy)
            .l(cx + r * a0.cos(), cy + r * a0.sin())
            .arc(r, half * 2.0 > PI, true, cx + r * a1.cos(), cy + r * a1.sin())
            .z();
    }
    p.as_str().to_string()
}

/// p05: 같은 자르기 영역의 스냅샷 1~6장(3장 넘으면 2줄).
///
/// 몸 표지 = 점(집단 색) + 진행 방향 눈금 + 반투명 시야 부채꼴(중심 = 시선, 반각 = focal_angle_deg/2,
/// 반지름 = focal_range) + `looking_at`이 있으면 점선 시선. 알아차린 장소는 고리로, 장소는 작은 사각형으로,
/// 각 장 아래에 설명을 쓴다. `clip`이 유효하지 않으면 바탕 상자, 그것도 없으면 몸 위치 범위를 쓴다.
pub fn snapshot_strip(title: &str, base: &MapBase, clip: (f64, f64, f64, f64), panels: &[SnapshotPanel]) -> String {
    let panels = &panels[..panels.len().min(SNAPSHOT_MAX_PANELS)];
    let n = panels.len();
    let view = if bbox_ok(clip) {
        Some(clip)
    } else {
        let pts = panels.iter().flat_map(|p| p.glyphs.iter().map(|g| g.pos));
        resolve_bbox(base.bbox, pts)
    };
    let mut gcount: BTreeMap<&str, usize> = BTreeMap::new();
    for g in panels.iter().flat_map(|p| p.glyphs.iter()).filter(|g| finite_pt(g.pos)) {
        *gcount.entry(g.group.as_str()).or_insert(0) += 1;
    }
    let pal = Palette::new(gcount.keys().map(|k| (*k).to_string()));
    let (Some(v), true) = (view, n > 0) else {
        return empty_figure(title, 400.0);
    };
    let mut group_items = pal.items(Swatch::Dot, &|_: &str| None);
    if group_items.is_empty() {
        group_items.push(item(Swatch::Blank, NO_DATA));
    }
    let secs = vec![
        Section { title: "집단".into(), items: group_items },
        Section {
            title: "표시".into(),
            items: vec![
                item(Swatch::Tick, "몸·진행 방향"),
                item(Swatch::Wedge(CATEGORICAL[0].into()), "시야 부채꼴"),
                item(Swatch::Dash(GAZE_STROKE.into()), "시선(보는 대상)"),
                item(Swatch::Ring(NOTICE_RING.into()), "알아차린 장소"),
                item(Swatch::Square(PLACE_NEUTRAL.into()), "장소"),
            ],
        },
    ];

    let (vw, vh) = (v.2 - v.0, v.3 - v.1);
    let scale = (PANEL_MAX_W / vw).min(PANEL_MAX_H / vh);
    let (pw, ph) = (vw * scale, vh * scale);
    let cols = if n <= 3 { n } else { n.div_ceil(2) };
    let rows = if n <= 3 { 1 } else { 2 };
    let legend_w = 180.0;
    let grid_w = cols as f64 * pw + (cols - 1) as f64 * PANEL_GAP;
    let width = (PAD + grid_w + PAD + legend_w + PAD).max(640.0);
    let top = MAP_TOP;
    let grid_h = rows as f64 * (ph + PANEL_CAPTION) + (rows - 1) as f64 * PANEL_GAP;
    let height = (top + grid_h + PAD).max(top + legend_col_height(&secs) + PAD).max(360.0);

    let mut s = frame(width, height, title);
    let id = clip_id("p05", title, 0.0, 0.0, pw, ph);
    def_clip(&mut s, &id, 0.0, 0.0, pw, ph);
    let xf = MapXf { min_x: v.0, min_z: v.1, scale, ox: 0.0, oy: 0.0 };
    let net = Some((NETWORK_STROKE, 0.8, 0.6));
    let reach = |g: &Glyph| if g.focal_range.is_finite() { g.focal_range.max(5.0) } else { 5.0 };
    let near = |p: (f64, f64), m: f64| p.0 >= v.0 - m && p.0 <= v.2 + m && p.1 >= v.1 - m && p.1 <= v.3 + m;
    for (i, panel) in panels.iter().enumerate() {
        let (col, row) = (i % cols, i / cols);
        let px = PAD + col as f64 * (pw + PANEL_GAP);
        let py = top + row as f64 * (ph + PANEL_CAPTION + PANEL_GAP);
        s.open("g", &Attrs::new().s("transform", &format!("translate({} {})", num(px), num(py))));
        s.rect(0.0, 0.0, pw, ph, fill(SURFACE));
        open_clipped(&mut s, &id);
        draw_base(&mut s, base, &xf, v, &BaseStyle { raster: true, buildings: true, network: net });
        let glyphs: Vec<&Glyph> = panel.glyphs.iter().filter(|g| finite_pt(g.pos) && near(g.pos, reach(g))).collect();
        for pl in panel.places.iter().filter(|p| finite_pt(p.pos) && near(p.pos, 5.0)) {
            let (x, y) = xf.pt(pl.pos);
            let tip = match &pl.label {
                Some(l) => format!("{} · {l}", pl.category),
                None => pl.category.clone(),
            };
            s.rect_tip(x - 3.0, y - 3.0, 6.0, 6.0, fill(PLACE_NEUTRAL).s("stroke", SURFACE).n("stroke-width", 1.0), Some(&tip));
        }
        for q in panel.noticed.iter().filter(|q| finite_pt(**q) && near(**q, 5.0)) {
            let (x, y) = xf.pt(*q);
            s.circle(x, y, 7.5, stroke(NOTICE_RING, 2.0).s("fill", "none"));
        }
        for g in &glyphs {
            let ok = g.gaze.is_finite() && g.focal_range.is_finite() && g.focal_range > 0.0;
            if !(ok && g.focal_angle_deg.is_finite() && g.focal_angle_deg > 0.0) {
                continue;
            }
            let (x, y) = xf.pt(g.pos);
            let r = (g.focal_range * scale).min(1e5);
            let half = (g.focal_angle_deg.min(360.0) / 2.0).to_radians();
            let c = pal.color(&g.group);
            s.path(&wedge_path(x, y, r, g.gaze, half), fill(c).n("fill-opacity", 0.12).s("stroke", c).n("stroke-opacity", 0.35).n("stroke-width", 0.75));
        }
        for g in &glyphs {
            if let Some(t) = g.looking_at.filter(|t| finite_pt(*t)) {
                let (x, y) = xf.pt(g.pos);
                let (tx, ty) = xf.pt(t);
                s.line(x, y, tx, ty, stroke(GAZE_STROKE, 1.0).s("stroke-dasharray", "3 2"));
            }
        }
        for g in &glyphs {
            let (x, y) = xf.pt(g.pos);
            if g.heading.is_finite() {
                let (hx, hy) = (x + 10.0 * g.heading.cos(), y + 10.0 * g.heading.sin());
                s.line(x, y, hx, hy, stroke(HEADING_STROKE, 1.6).s("stroke-linecap", "round"));
            }
            let tip = if g.activity.is_empty() { g.group.clone() } else { format!("{} · {}", g.group, g.activity) };
            s.circle_tip(x, y, 4.0, fill(pal.color(&g.group)).s("stroke", SURFACE).n("stroke-width", 1.5), Some(&tip));
        }
        s.close();
        s.rect(0.0, 0.0, pw, ph, stroke(GRID, 1.0).s("fill", "none"));
        if glyphs.is_empty() {
            no_data(&mut s, pw / 2.0, ph / 2.0);
        }
        if i == 0 {
            scale_bar(&mut s, scale, 12.0, ph - 12.0, pw);
            north_arrow(&mut s, pw - 16.0, 8.0);
        }
        s.text(pw / 2.0, ph + 17.0, &truncate(&panel.caption, 12.0, pw), txt_mid(12.0, INK2));
        s.close();
    }
    legend_col(&mut s, width - PAD - legend_w, top, legend_w, &secs);
    s.finish()
}

// ============================================================================
// p06 100% 누적 가로 막대
// ============================================================================

/// 부분 값을 합 1로 맞춘다. 음수·유한하지 않은 값은 0으로 보고, 합이 0이면 `None`.
pub fn normalize_shares(values: &[f64]) -> Option<Vec<f64>> {
    let clean: Vec<f64> = values.iter().map(|v| if v.is_finite() && *v > 0.0 { *v } else { 0.0 }).collect();
    let sum: f64 = clean.iter().sum();
    if sum.is_finite() && sum > 0.0 {
        Some(clean.iter().map(|v| v / sum).collect())
    } else {
        None
    }
}

/// p06: 100% 누적 가로 막대(예: 집단별 앞길·발밑·장소 보기 시간 비율).
///
/// 행마다 부분 값을 합 1로 맞추고([`normalize_shares`]), 합이 0인 행은 이름과 "자료 없음"을 쓴다.
/// 부분 이름의 색은 처음 나온 순서로 고정해 모든 행에서 같다.
pub fn stacked_share_bars(title: &str, rows: &[StackRow]) -> String {
    if rows.is_empty() {
        return empty_figure(title, 360.0);
    }
    let mut names: Vec<String> = Vec::new();
    for r in rows {
        for (n, _) in &r.parts {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
    }
    let pal = Palette::new(names);
    let items = pal.items(Swatch::Rect, &|_: &str| None);
    let label_w = rows.iter().map(|r| text_width(&r.label, 13.0)).fold(0.0, f64::max).clamp(40.0, 220.0);
    let x0 = PAD + label_w + 12.0;
    let x1 = W - PAD - 16.0;
    let top = LEGEND_Y + legend_row_height(&items, x1 - PAD) + 18.0;
    let band = 40.0;
    let bar_h = 24.0;
    let plot_h = rows.len() as f64 * band;
    let height = (top + plot_h + 56.0).max(320.0);
    let xs = Lin::new(0.0, 1.0, x0, x1);

    let mut s = frame(W, height, title);
    legend_row(&mut s, PAD, LEGEND_Y, x1 - PAD, &items);
    let t = nice_ticks(0.0, 1.0, 6);
    for v in &t.values {
        let x = xs.map(*v);
        s.line(x, top - 4.0, x, top + plot_h, stroke(GRID, 1.0));
        s.text(x, top + plot_h + 18.0, &fmt_tick_pct(*v, t.step), txt_mid(11.5, INK2));
    }
    s.text((x0 + x1) / 2.0, top + plot_h + 40.0, "비율 (행마다 합 100%)", txt_mid(12.0, INK2));
    for (i, r) in rows.iter().enumerate() {
        let cy = top + i as f64 * band + band / 2.0;
        s.text(x0 - 10.0, cy + 4.5, &truncate(&r.label, 13.0, label_w), txt_end(13.0, INK));
        let vals: Vec<f64> = r.parts.iter().map(|p| p.1).collect();
        let Some(sh) = normalize_shares(&vals) else {
            s.text(x0 + 8.0, cy + 4.5, NO_DATA, txt(12.0, INK2));
            continue;
        };
        let first = sh.iter().position(|w| *w > 0.0);
        let last = sh.iter().rposition(|w| *w > 0.0);
        let mut acc = 0.0;
        for (j, (name, raw)) in r.parts.iter().enumerate() {
            let w = sh[j];
            if w <= 0.0 {
                continue;
            }
            let (xa, xb) = (xs.map(acc), xs.map(acc + w));
            acc += w;
            let gl = if first == Some(j) { 0.0 } else { 1.0 };
            let gr = if last == Some(j) { 0.0 } else { 1.0 };
            let (sx, sw) = (xa + gl, xb - xa - gl - gr);
            if sw <= 0.0 {
                continue;
            }
            let color = pal.color(name);
            let tip = format!("{} · {name}: {} ({})", r.label, fmt_pct(w), fmt_value(*raw));
            s.rect_tip(sx, cy - bar_h / 2.0, sw, bar_h, fill(color), Some(&tip));
            let lbl = fmt_pct(w);
            if text_width(&lbl, 11.0) + 8.0 <= sw {
                s.text(sx + sw / 2.0, cy + 4.0, &lbl, txt_mid(11.0, label_ink(color)));
            }
        }
    }
    s.finish()
}

// ============================================================================
// p07·p11 묶음 막대
// ============================================================================

/// p07·p11: 묶음 세로 막대(범주마다 계열 막대). 계열이 둘 이상이면 범례를 단다.
///
/// 범주 이름이 칸보다 길면 -35° 기울여 쓴다. 막대가 12개 이하이면 끝에 값을 쓴다.
pub fn grouped_bars(title: &str, data: &GroupedBars, y_label: &str) -> String {
    let n_ser = data.series.len();
    let n_cat = data.categories.len().max(data.series.iter().map(|s| s.values.len()).max().unwrap_or(0));
    let vals = data.series.iter().flat_map(|s| s.values.iter().copied());
    let has = data.series.iter().flat_map(|s| s.values.iter()).any(|v| v.is_finite());
    let (mn, mx) = min_max(vals).unwrap_or((0.0, 0.0));
    let t = nice_ticks(mn.min(0.0), mx.max(0.0), 6);
    let u = unit_for(&t);
    let mut items: Vec<Item> = Vec::new();
    if n_ser >= 2 {
        for (i, ser) in data.series.iter().enumerate().take(CATEGORICAL.len()) {
            let name = if ser.name.is_empty() { format!("계열 {}", i + 1) } else { ser.name.clone() };
            items.push(item(Swatch::Rect(series_color(i).into()), name));
        }
        if n_ser > CATEGORICAL.len() {
            items.push(item(Swatch::Rect(OTHER_COLOR.into()), format!("기타 {}계열", n_ser - CATEGORICAL.len())));
        }
    }
    let x0 = PAD + 22.0 + tick_label_width(&t, u, 11.5) + 10.0;
    let x1 = W - PAD;
    let y0 = LEGEND_Y + legend_row_height(&items, x1 - x0) + 18.0;
    let band = (x1 - x0) / n_cat.max(1) as f64;
    let lbl_w = data.categories.iter().map(|c| text_width(c, 12.0)).fold(0.0, f64::max);
    let rotate = lbl_w > band - 6.0;
    let lbl_max = if rotate { 140.0 } else { (band - 4.0).max(10.0) };
    let bottom = if rotate { (lbl_w.min(lbl_max) * 0.58 + 30.0).max(56.0) } else { 56.0 };
    let y1 = H - bottom;
    let ys = Lin::new(t.lo, t.hi, y1, y0);

    let mut s = frame(W, H, title);
    legend_row(&mut s, x0, LEGEND_Y, x1 - x0, &items);
    for v in &t.values {
        let y = ys.map(*v);
        s.line(x0, y, x1, y, stroke(GRID, 1.0));
        s.text(x0 - 8.0, y + 4.0, &fmt_tick(*v, t.step, u), txt_end(11.5, INK2));
    }
    y_title(&mut s, PAD + 6.0, (y0 + y1) / 2.0, y_label, y1 - y0, false);
    let yb = ys.map(0.0);
    s.line(x0, yb, x1, yb, stroke(AXIS, 1.0));
    if !has || n_cat == 0 || n_ser == 0 {
        no_data(&mut s, (x0 + x1) / 2.0, (y0 + y1) / 2.0);
        return s.finish();
    }
    let gap = 2.0;
    let group_w = (band * 0.78).min(n_ser as f64 * 24.0 + (n_ser - 1) as f64 * gap);
    let bw = ((group_w - (n_ser - 1) as f64 * gap) / n_ser as f64).max(1.0);
    let n_bars = data.series.iter().map(|s| s.values.iter().take(n_cat).filter(|v| v.is_finite()).count()).sum::<usize>();
    let label_values = n_bars <= 12 && bw >= 12.0;
    for c in 0..n_cat {
        let gx = x0 + band * c as f64;
        let start = gx + (band - group_w) / 2.0;
        let cat = data.categories.get(c).map(String::as_str).unwrap_or("");
        for (si, ser) in data.series.iter().enumerate() {
            let Some(v) = ser.values.get(c).copied().filter(|v| v.is_finite()) else { continue };
            let bx = start + si as f64 * (bw + gap);
            let ye = ys.map(v);
            if (ye - yb).abs() >= 0.01 {
                let tip = format!("{cat} · {}: {}", ser.name, fmt_value(v));
                s.path_tip(&vbar_path(bx, bw, yb, ye, 4.0), fill(series_color(si)), Some(&tip));
            }
            if label_values {
                let ty = if v >= 0.0 { ye - 5.0 } else { ye + 13.0 };
                s.text(bx + bw / 2.0, ty, &fmt_value(v), txt_mid(10.5, INK2));
            }
        }
        let lx = gx + band / 2.0;
        let lbl = truncate(cat, 12.0, lbl_max);
        if rotate {
            let ty = y1 + 14.0;
            let tr = format!("rotate(-35 {} {})", num(lx), num(ty));
            s.text(lx, ty, &lbl, txt_end(12.0, INK2).s("transform", &tr));
        } else {
            s.text(lx, y1 + 18.0, &lbl, txt_mid(12.0, INK2));
        }
    }
    s.finish()
}

// ============================================================================
// p08 가로 막대
// ============================================================================

/// p08: 가로 막대(예: 장소별 알아차림 수 상위 20). 입력 순서를 지킨다.
///
/// `secondary`(0~1 비율)가 있으면 오른쪽 열에 가는 막대와 백분율을 쓴다. 열 제목은 `secondary_label`.
pub fn hbar_chart(title: &str, rows: &[HBar], value_label: &str, secondary_label: Option<&str>) -> String {
    if rows.is_empty() {
        return empty_figure(title, 360.0);
    }
    let has_sec = secondary_label.is_some() || rows.iter().any(|r| r.secondary.is_some());
    let label_w = rows.iter().map(|r| text_width(&r.label, 12.5)).fold(0.0, f64::max).clamp(48.0, 240.0);
    let sec_w = if has_sec { 176.0 } else { 0.0 };
    let x0 = PAD + label_w + 10.0;
    let x1 = W - PAD - sec_w - 60.0;
    let (mn, mx) = min_max(rows.iter().map(|r| r.value)).unwrap_or((0.0, 0.0));
    let has = rows.iter().any(|r| r.value.is_finite());
    let t = nice_ticks(mn.min(0.0), mx.max(0.0), 6);
    let u = unit_for(&t);
    let band = 26.0;
    let bar_h = 16.0;
    let top = 64.0 + if has_sec { 18.0 } else { 0.0 };
    let plot_h = rows.len() as f64 * band;
    let height = (top + plot_h + 64.0).max(360.0);
    let xs = Lin::new(t.lo, t.hi, x0, x1);

    let mut s = frame(W, height, title);
    for v in &t.values {
        let x = xs.map(*v);
        s.line(x, top - 4.0, x, top + plot_h, stroke(GRID, 1.0));
        s.text(x, top + plot_h + 18.0, &fmt_tick(*v, t.step, u), txt_mid(11.5, INK2));
    }
    s.text((x0 + x1) / 2.0, top + plot_h + 40.0, &truncate(value_label, 12.0, x1 - x0), txt_mid(12.0, INK2));
    let xb = xs.map(0.0);
    s.line(xb, top - 4.0, xb, top + plot_h, stroke(AXIS, 1.0));
    let sx = W - PAD - sec_w + 12.0;
    if has_sec {
        let head = secondary_label.unwrap_or("보조 값");
        s.text(sx, top - 12.0, &truncate(head, 12.0, sec_w - 12.0), bold(txt(12.0, INK)));
    }
    for (i, r) in rows.iter().enumerate() {
        let cy = top + i as f64 * band + band / 2.0;
        s.text(x0 - 8.0, cy + 4.5, &truncate(&r.label, 12.5, label_w), txt_end(12.5, INK));
        if r.value.is_finite() {
            let xe = xs.map(r.value);
            if (xe - xb).abs() >= 0.01 {
                let tip = format!("{}: {}", r.label, fmt_value(r.value));
                s.path_tip(&hbar_path(cy - bar_h / 2.0, bar_h, xb, xe, 4.0), fill(CATEGORICAL[0]), Some(&tip));
            }
            if r.value >= 0.0 {
                s.text(xe.max(xb) + 6.0, cy + 4.5, &fmt_value(r.value), txt(11.5, INK2));
            } else {
                s.text(xe - 6.0, cy + 4.5, &fmt_value(r.value), txt_end(11.5, INK2));
            }
        } else {
            s.text(xb + 6.0, cy + 4.5, "—", txt(11.5, INK2));
        }
        if has_sec {
            match r.secondary.filter(|v| v.is_finite()) {
                Some(v) => {
                    s.rect(sx, cy - 3.0, 90.0, 6.0, fill(TRACK_BG).n("rx", 3.0));
                    let fw = 90.0 * v.clamp(0.0, 1.0);
                    if fw > 0.0 {
                        s.rect(sx, cy - 3.0, fw, 6.0, fill(CATEGORICAL[1]).n("rx", 3.0));
                    }
                    s.text(sx + 98.0, cy + 4.5, &fmt_pct(v), txt(11.5, INK2));
                }
                None => s.text(sx, cy + 4.5, "—", txt(11.5, INK2)),
            }
        }
    }
    if !has {
        no_data(&mut s, (x0 + x1) / 2.0, top + plot_h / 2.0);
    }
    s.finish()
}

// ============================================================================
// p09 상자 그림
// ============================================================================

/// 상자 그림 통계. 사분위수는 선형 보간(R type 7, NumPy 기본값)이다.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStats {
    pub n: usize,
    pub min: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub max: f64,
    /// Q1 − 1.5·IQR 이상인 가장 작은 값.
    pub lo_whisker: f64,
    /// Q3 + 1.5·IQR 이하인 가장 큰 값.
    pub hi_whisker: f64,
    /// 수염 밖의 값(오름차순).
    pub outliers: Vec<f64>,
}

/// 정렬된 값의 분위수(선형 보간). 빈 입력이면 NaN.
pub fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.5 };
    let h = (sorted.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

/// 유한한 값만으로 상자 그림 통계를 낸다. 유한한 값이 없으면 `None`.
pub fn box_stats(values: &[f64]) -> Option<BoxStats> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let q1 = quantile_sorted(&v, 0.25);
    let median = quantile_sorted(&v, 0.5);
    let q3 = quantile_sorted(&v, 0.75);
    let iqr = q3 - q1;
    let (lf, hf) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let lo_whisker = v.iter().copied().find(|x| *x >= lf).unwrap_or(q1).min(q1);
    let hi_whisker = v.iter().rev().copied().find(|x| *x <= hf).unwrap_or(q3).max(q3);
    let outliers = v.iter().copied().filter(|x| *x < lf || *x > hf).collect();
    Some(BoxStats { n: v.len(), min: v[0], q1, median, q3, max: v[v.len() - 1], lo_whisker, hi_whisker, outliers })
}

/// p09: 집단별 상자 그림(상자 Q1–Q3, 중앙값 선, 1.5·IQR 안의 수염, 바깥 값 점, 집단 아래 n).
pub fn box_plot(title: &str, dists: &[Dist], y_label: &str) -> String {
    let stats: Vec<Option<BoxStats>> = dists.iter().map(|d| box_stats(&d.values)).collect();
    if dists.is_empty() {
        return empty_figure(title, 400.0);
    }
    let all = stats.iter().flatten().flat_map(|st| {
        let mut v = vec![st.lo_whisker, st.hi_whisker];
        v.extend(st.outliers.iter().copied());
        v
    });
    let (mn, mx) = min_max(all).unwrap_or((0.0, 0.0));
    let t = nice_ticks(zero_based(mn, mx), mx, 6);
    let u = unit_for(&t);
    let x0 = PAD + 22.0 + tick_label_width(&t, u, 11.5) + 10.0;
    let x1 = W - PAD;
    let y0 = 64.0;
    let y1 = H - 64.0;
    let ys = Lin::new(t.lo, t.hi, y1, y0);

    let mut s = frame(W, H, title);
    for v in &t.values {
        let y = ys.map(*v);
        s.line(x0, y, x1, y, stroke(GRID, 1.0));
        s.text(x0 - 8.0, y + 4.0, &fmt_tick(*v, t.step, u), txt_end(11.5, INK2));
    }
    y_title(&mut s, PAD + 6.0, (y0 + y1) / 2.0, y_label, y1 - y0, false);
    s.line(x0, y1, x1, y1, stroke(AXIS, 1.0));
    let band = (x1 - x0) / dists.len() as f64;
    let box_w = (band * 0.45).clamp(6.0, 56.0);
    let c1 = CATEGORICAL[0];
    let box_fill = tint(c1, 0.18);
    for (i, (d, st)) in dists.iter().zip(&stats).enumerate() {
        let cx = x0 + band * (i as f64 + 0.5);
        s.text(cx, y1 + 18.0, &truncate(&d.label, 12.0, band - 4.0), txt_mid(12.0, INK));
        let nn = st.as_ref().map_or(0, |st| st.n);
        s.text(cx, y1 + 34.0, &format!("n={nn}"), txt_mid(11.0, INK2));
        let Some(st) = st else {
            s.text(cx, (y0 + y1) / 2.0, NO_DATA, txt_mid(11.0, INK2));
            continue;
        };
        let (yq1, yq3, ymed) = (ys.map(st.q1), ys.map(st.q3), ys.map(st.median));
        let (ylo, yhi) = (ys.map(st.lo_whisker), ys.map(st.hi_whisker));
        let wa = stroke(INK2, 1.2);
        s.line(cx, yq1, cx, ylo, wa.clone());
        s.line(cx, yq3, cx, yhi, wa.clone());
        s.line(cx - box_w / 4.0, ylo, cx + box_w / 4.0, ylo, wa.clone());
        s.line(cx - box_w / 4.0, yhi, cx + box_w / 4.0, yhi, wa);
        let tip = format!(
            "{}: n={}, 중앙값 {}, Q1 {}, Q3 {}, 수염 {}~{}",
            d.label,
            st.n,
            fmt_value(st.median),
            fmt_value(st.q1),
            fmt_value(st.q3),
            fmt_value(st.lo_whisker),
            fmt_value(st.hi_whisker)
        );
        s.rect_tip(cx - box_w / 2.0, yq3, box_w, (yq1 - yq3).max(1.0), fill(&box_fill).s("stroke", c1).n("stroke-width", 1.5), Some(&tip));
        s.line(cx - box_w / 2.0, ymed, cx + box_w / 2.0, ymed, stroke(INK, 2.0));
        s.text(cx + box_w / 2.0 + 4.0, ymed + 4.0, &fmt_value(st.median), txt(11.0, INK2));
        let mut seen: BTreeSet<i64> = BTreeSet::new();
        s.open("g", &fill(c1).n("fill-opacity", 0.55));
        for o in &st.outliers {
            let y = ys.map(*o);
            if seen.insert((y * 2.0).round() as i64) {
                s.circle(cx, y, 2.6, Attrs::new());
            }
        }
        s.close();
    }
    if stats.iter().all(Option::is_none) {
        no_data(&mut s, (x0 + x1) / 2.0, (y0 + y1) / 2.0 - 24.0);
    }
    s.finish()
}

// ============================================================================
// p10 산점 + 기준선
// ============================================================================

/// p10: 산점(반투명 점)과 기준 곡선(예: 간선별 밀도·속도와 Weidmann 기준선).
///
/// 축 범위는 점과 곡선을 함께 덮는다. 점이 4000개를 넘으면 같은 화면 픽셀의 점을 하나로 줄인다.
pub fn scatter_with_curve(
    title: &str,
    points: &[(f64, f64)],
    curve: &[(f64, f64)],
    x_label: &str,
    y_label: &str,
    curve_label: &str,
) -> String {
    let pts: Vec<(f64, f64)> = points.iter().copied().filter(|p| finite_pt(*p)).collect();
    let curve_n = curve.iter().filter(|p| finite_pt(**p)).count();
    let all = || pts.iter().copied().chain(curve.iter().copied().filter(|p| finite_pt(*p)));
    let (xmn, xmx) = min_max(all().map(|p| p.0)).unwrap_or((0.0, 0.0));
    let (ymn, ymx) = min_max(all().map(|p| p.1)).unwrap_or((0.0, 0.0));
    let tx = nice_ticks(zero_based(xmn, xmx), xmx, 7);
    let ty = nice_ticks(zero_based(ymn, ymx), ymx, 6);
    let (ux, uy) = (unit_for(&tx), unit_for(&ty));
    let mut items = vec![item(Swatch::Dot(CATEGORICAL[0].into()), format!("관측 ({}개)", pts.len()))];
    if curve_n >= 2 {
        let name = if curve_label.is_empty() { "기준선" } else { curve_label };
        items.push(item(Swatch::Line(CATEGORICAL[1].into()), name));
    }
    let x0 = PAD + 22.0 + tick_label_width(&ty, uy, 11.5) + 10.0;
    let x1 = W - PAD - 8.0;
    let y0 = LEGEND_Y + legend_row_height(&items, x1 - x0) + 18.0;
    let y1 = H - 64.0;
    let xs = Lin::new(tx.lo, tx.hi, x0, x1);
    let ys = Lin::new(ty.lo, ty.hi, y1, y0);

    let mut s = frame(W, H, title);
    legend_row(&mut s, x0, LEGEND_Y, x1 - x0, &items);
    for v in &ty.values {
        let y = ys.map(*v);
        s.line(x0, y, x1, y, stroke(GRID, 1.0));
        s.text(x0 - 8.0, y + 4.0, &fmt_tick(*v, ty.step, uy), txt_end(11.5, INK2));
    }
    for v in &tx.values {
        let x = xs.map(*v);
        s.line(x, y0, x, y1, stroke(GRID, 1.0));
        s.text(x, y1 + 18.0, &fmt_tick(*v, tx.step, ux), txt_mid(11.5, INK2));
    }
    s.line(x0, y1, x1, y1, stroke(AXIS, 1.0));
    s.line(x0, y0, x0, y1, stroke(AXIS, 1.0));
    s.text((x0 + x1) / 2.0, y1 + 42.0, &truncate(x_label, 12.0, x1 - x0), txt_mid(12.0, INK2));
    y_title(&mut s, PAD + 6.0, (y0 + y1) / 2.0, y_label, y1 - y0, false);

    s.open("g", &fill(CATEGORICAL[0]).n("fill-opacity", 0.45));
    let dedupe = pts.len() > 4000;
    let mut seen: BTreeSet<(i64, i64)> = BTreeSet::new();
    for p in &pts {
        let (x, y) = (xs.map(p.0), ys.map(p.1));
        if dedupe && !seen.insert((x.round() as i64, y.round() as i64)) {
            continue;
        }
        s.circle(x, y, 2.6, Attrs::new());
    }
    s.close();
    if curve_n >= 2 {
        let mut p = PathData::new();
        let mut pen = false;
        for q in curve {
            if finite_pt(*q) {
                let (x, y) = (xs.map(q.0), ys.map(q.1));
                if pen {
                    p.l(x, y);
                } else {
                    p.m(x, y);
                    pen = true;
                }
            } else {
                pen = false;
            }
        }
        s.path(p.as_str(), stroke(CATEGORICAL[1], 2.0).s("fill", "none").s("stroke-linejoin", "round").s("stroke-linecap", "round"));
    }
    if pts.is_empty() {
        no_data(&mut s, (x0 + x1) / 2.0, (y0 + y1) / 2.0);
    }
    s.finish()
}

// ============================================================================
// p12 온톨로지 지도
// ============================================================================

const MIDDLE_KINDS: [&str; 5] = ["capability", "sensitivity", "interest", "perception", "propensity"];
const COLUMN_NAMES: [&str; 4] = ["근거", "페르소나", "특성", "차원"];
const NODE_FS: f64 = 12.5;
const DETAIL_FS: f64 = 10.5;
const NODE_MIN_W: f64 = 96.0;
const NODE_MAX_W: f64 = 230.0;
const NODE_GAP: f64 = 8.0;
const GROUP_HDR: f64 = 20.0;

/// 마디 종류의 열: evidence 0, persona 1, dimension·interest_dim 3, 그 밖(특성·모르는 종류) 2.
pub fn ontology_column(kind: &str) -> usize {
    match kind {
        "evidence" => 0,
        "persona" => 1,
        "dimension" | "interest_dim" => 3,
        _ => 2,
    }
}

fn kind_name(kind: &str) -> &str {
    match kind {
        "evidence" => "근거",
        "persona" => "페르소나",
        "capability" => "능력",
        "sensitivity" => "민감도",
        "interest" => "관심",
        "perception" => "지각",
        "propensity" => "성향",
        "dimension" => "차원",
        "interest_dim" => "관심 차원",
        other => other,
    }
}

fn kind_color(kind: &str) -> &'static str {
    match kind {
        "evidence" => "#a3651a",
        "persona" => "#6250d6",
        "capability" => "#2a78d6",
        "sensitivity" => "#e34948",
        "interest" => "#eb6834",
        "perception" => "#1baf7a",
        "propensity" => "#e87ba4",
        "dimension" => "#008300",
        "interest_dim" => "#eda100",
        _ => OTHER_COLOR,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NodeBox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// p12: 페르소나 한 명의 근거 → 페르소나 → 특성 → 차원 지도(왼쪽에서 오른쪽 열).
///
/// 가운데 열은 능력·민감도·관심·지각·성향 순으로 묶고(모르는 종류는 처음 나온 순으로 그 뒤),
/// 같은 묶음 안에서는 입력 순서를 지킨다. 마디는 둥근 사각형(이름 + 작은 설명 줄), 연결선은 곡선과
/// 작은 이름이다. id가 겹치면 처음 마디만 쓰고, 없는 id를 가리키는 연결선과 자기 자신 연결선은 그리지 않는다.
pub fn ontology_map(title: &str, nodes: &[GNode], edges: &[GEdge]) -> String {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let nodes: Vec<&GNode> = nodes.iter().filter(|n| seen.insert(n.id.as_str())).collect();
    if nodes.is_empty() {
        return empty_figure(title, 400.0);
    }
    let mut cols: Vec<Vec<usize>> = vec![Vec::new(); 4];
    for (i, n) in nodes.iter().enumerate() {
        cols[ontology_column(&n.kind)].push(i);
    }
    let mut unknown: Vec<&str> = Vec::new();
    for &i in &cols[2] {
        let k = nodes[i].kind.as_str();
        if !MIDDLE_KINDS.contains(&k) && !unknown.contains(&k) {
            unknown.push(k);
        }
    }
    let rank = |k: &str| {
        MIDDLE_KINDS
            .iter()
            .position(|m| *m == k)
            .unwrap_or_else(|| MIDDLE_KINDS.len() + unknown.iter().position(|u| *u == k).unwrap_or(0))
    };
    cols[2].sort_by_key(|&i| rank(&nodes[i].kind));

    let has_detail = |n: &GNode| n.detail.as_deref().is_some_and(|d| !d.is_empty());
    let node_w: Vec<f64> = nodes
        .iter()
        .map(|n| {
            let lw = text_width(&n.label, NODE_FS);
            let dw = n.detail.as_deref().map_or(0.0, |d| text_width(d, DETAIL_FS));
            (lw.max(dw) + 20.0).clamp(NODE_MIN_W, NODE_MAX_W)
        })
        .collect();
    let node_h: Vec<f64> = nodes.iter().map(|n| if has_detail(n) { 40.0 } else { 26.0 }).collect();
    let col_w: Vec<f64> = cols.iter().map(|c| c.iter().map(|&i| node_w[i]).fold(NODE_MIN_W, f64::max)).collect();
    let inner: f64 = col_w.iter().sum();
    let gap = ((W - 2.0 * PAD - inner) / 3.0).max(84.0);
    let width = (2.0 * PAD + inner + 3.0 * gap).max(W);
    let mut col_x = [0.0f64; 4];
    let mut x = PAD;
    for (cx, w) in col_x.iter_mut().zip(&col_w) {
        *cx = x;
        x += w + gap;
    }
    let col_h: Vec<f64> = cols
        .iter()
        .enumerate()
        .map(|(c, ids)| {
            let mut h = 0.0;
            let mut prev: Option<&str> = None;
            for &i in ids {
                let k = nodes[i].kind.as_str();
                if c == 2 && prev != Some(k) {
                    h += GROUP_HDR;
                    prev = Some(k);
                }
                h += node_h[i] + NODE_GAP;
            }
            (h - NODE_GAP).max(0.0)
        })
        .collect();
    let max_h = col_h.iter().copied().fold(0.0, f64::max);
    let top = 84.0;
    let height = (top + max_h + 36.0).max(400.0);
    let mut pos = vec![NodeBox::default(); nodes.len()];
    let mut headers: Vec<(f64, f64, &str, &str)> = Vec::new();
    for (c, ids) in cols.iter().enumerate() {
        let mut y = top + (max_h - col_h[c]) / 2.0;
        let mut prev: Option<&str> = None;
        for &i in ids {
            let k = nodes[i].kind.as_str();
            if c == 2 && prev != Some(k) {
                headers.push((col_x[c], y + 13.0, kind_name(k), k));
                y += GROUP_HDR;
                prev = Some(k);
            }
            pos[i] = NodeBox { x: col_x[c] + (col_w[c] - node_w[i]) / 2.0, y, w: node_w[i], h: node_h[i] };
            y += node_h[i] + NODE_GAP;
        }
    }

    let mut s = frame(width, height, title);
    for c in 0..4 {
        s.text(col_x[c] + col_w[c] / 2.0, 62.0, COLUMN_NAMES[c], bold(txt_mid(13.0, INK2)));
        if c < 3 {
            s.text(col_x[c] + col_w[c] + gap / 2.0, 62.0, "→", txt_mid(13.0, MUTED));
        }
    }
    let idx: BTreeMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut heads = PathData::new();
    let mut labels: Vec<((f64, f64), &str)> = Vec::new();
    s.open("g", &stroke(EDGE_STROKE, 1.2).n("stroke-opacity", 0.8).s("fill", "none"));
    for e in edges {
        let (Some(&a), Some(&b)) = (idx.get(e.from.as_str()), idx.get(e.to.as_str())) else { continue };
        if a == b {
            continue;
        }
        let (ca, cb) = (ontology_column(&nodes[a].kind), ontology_column(&nodes[b].kind));
        let (pa, pb) = (pos[a], pos[b]);
        let (ya, yb) = (pa.y + pa.h / 2.0, pb.y + pb.h / 2.0);
        let (p0, p1, p2, p3) = if ca < cb {
            let (s0, e0) = ((pa.x + pa.w, ya), (pb.x, yb));
            let dx = ((e0.0 - s0.0) * 0.5).max(30.0);
            (s0, (s0.0 + dx, ya), (e0.0 - dx, yb), e0)
        } else if ca > cb {
            let (s0, e0) = ((pa.x, ya), (pb.x + pb.w, yb));
            let dx = ((s0.0 - e0.0) * 0.5).max(30.0);
            (s0, (s0.0 - dx, ya), (e0.0 + dx, yb), e0)
        } else {
            let (s0, e0) = ((pa.x + pa.w, ya), (pb.x + pb.w, yb));
            let bulge = 36.0 + (yb - ya).abs() * 0.12;
            (s0, (s0.0 + bulge, ya), (e0.0 + bulge, yb), e0)
        };
        let mut p = PathData::new();
        p.m(p0.0, p0.1).c(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1);
        s.path(p.as_str(), Attrs::new().s("data-from", &e.from).s("data-to", &e.to));
        let (dx, dy) = (p3.0 - p2.0, p3.1 - p2.1);
        let len = dx.hypot(dy);
        let (ux, uy) = if len > 1e-9 { (dx / len, dy / len) } else { (1.0, 0.0) };
        let (bx, by) = (p3.0 - ux * 7.0, p3.1 - uy * 7.0);
        heads.m(p3.0, p3.1).l(bx - uy * 3.5, by + ux * 3.5).l(bx + uy * 3.5, by - ux * 3.5).z();
        if !e.label.is_empty() {
            let mid = ((p0.0 + 3.0 * p1.0 + 3.0 * p2.0 + p3.0) / 8.0, (p0.1 + 3.0 * p1.1 + 3.0 * p2.1 + p3.1) / 8.0);
            labels.push((mid, e.label.as_str()));
        }
    }
    s.close();
    if !heads.is_empty() {
        s.path(heads.as_str(), fill(EDGE_STROKE));
    }
    for (x, y, name, kind) in &headers {
        s.circle(x + 4.0, y - 4.0, 4.0, fill(kind_color(kind)));
        s.text(x + 13.0, *y, name, bold(txt(11.0, INK2)));
    }
    for (i, n) in nodes.iter().enumerate() {
        let b = pos[i];
        let c = kind_color(&n.kind);
        let mut tip = format!("{} · {}", kind_name(&n.kind), n.label);
        if let Some(d) = n.detail.as_deref().filter(|d| !d.is_empty()) {
            tip.push_str(" · ");
            tip.push_str(d);
        }
        let a = fill(&tint(c, 0.14)).s("stroke", c).n("stroke-width", 1.2).n("rx", 6.0).s("data-id", &n.id);
        s.rect_tip(b.x, b.y, b.w, b.h, a, Some(&tip));
        let label_y = if has_detail(n) { b.y + 17.0 } else { b.y + b.h / 2.0 + 4.5 };
        s.text(b.x + b.w / 2.0, label_y, &truncate(&n.label, NODE_FS, b.w - 16.0), bold(txt_mid(NODE_FS, INK)));
        if let Some(d) = n.detail.as_deref().filter(|d| !d.is_empty()) {
            s.text(b.x + b.w / 2.0, b.y + 32.0, &truncate(d, DETAIL_FS, b.w - 16.0), txt_mid(DETAIL_FS, INK2));
        }
    }
    for ((x, y), l) in &labels {
        s.text(*x, y + 3.5, &truncate(l, 10.0, 120.0), halo(txt_mid(10.0, INK2)));
    }
    s.finish()
}

// ============================================================================
// p13 결정
// ============================================================================

/// p13: 결정 하나. 왼쪽은 대안별 선택 확률 가로 막대(고른 대안 강조), 오른쪽은 부호 있는 기여 항 막대.
///
/// 기여 항은 절댓값이 큰 순으로 최대 [`DECISION_MAX_TERMS`]개를 그린다(같으면 입력 순서).
pub fn decision_chart(title: &str, alts: &[AltBar], terms: &[(String, f64)]) -> String {
    let mut ts: Vec<(&str, f64)> = terms.iter().filter(|t| t.1.is_finite()).map(|(n, v)| (n.as_str(), *v)).collect();
    ts.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    ts.truncate(DECISION_MAX_TERMS);
    let rows = alts.len().max(ts.len()).max(1);
    let band = 30.0;
    let bar_h = 18.0;
    let top = 112.0;
    let plot_h = rows as f64 * band;
    let height = (top + plot_h + 56.0).max(360.0);
    let mid = W / 2.0;

    let mut s = frame(W, height, title);
    // 왼쪽 판: 대안별 확률
    let (lx0, lx1) = (PAD, mid - 16.0);
    let lab_w = alts.iter().map(|a| text_width(&a.label, 12.5)).fold(0.0, f64::max).clamp(48.0, 150.0);
    let (bx0, bx1) = (lx0 + lab_w + 8.0, lx1 - 76.0);
    s.text(lx0, 66.0, "대안별 선택 확률", bold(txt(13.0, INK)));
    let left_items = vec![item(Swatch::Rect(CATEGORICAL[0].into()), "고른 대안"), item(Swatch::Rect(NEUTRAL_BAR.into()), "다른 대안")];
    legend_row(&mut s, lx0, 74.0, lx1 - lx0, &left_items);
    let xs = Lin::new(0.0, 1.0, bx0, bx1);
    let tl = nice_ticks(0.0, 1.0, 6);
    for v in &tl.values {
        let x = xs.map(*v);
        s.line(x, top - 4.0, x, top + plot_h, stroke(GRID, 1.0));
        s.text(x, top + plot_h + 18.0, &fmt_tick_pct(*v, tl.step), txt_mid(11.0, INK2));
    }
    s.line(bx0, top - 4.0, bx0, top + plot_h, stroke(AXIS, 1.0));
    for (i, a) in alts.iter().enumerate() {
        let cy = top + i as f64 * band + band / 2.0;
        let lbl = truncate(&a.label, 12.5, lab_w);
        let la = txt_end(12.5, INK);
        s.text(bx0 - 8.0, cy + 4.5, &lbl, if a.chosen { bold(la) } else { la });
        if !a.prob.is_finite() {
            s.text(bx0 + 6.0, cy + 4.5, "—", txt(11.5, INK2));
            continue;
        }
        let p = a.prob.clamp(0.0, 1.0);
        let xe = xs.map(p);
        let color = if a.chosen { CATEGORICAL[0] } else { NEUTRAL_BAR };
        if xe - bx0 >= 0.01 {
            let tip = format!("{}: {}{}", a.label, fmt_pct(a.prob), if a.chosen { " (고름)" } else { "" });
            s.path_tip(&hbar_path(cy - bar_h / 2.0, bar_h, bx0, xe, 4.0), fill(color), Some(&tip));
        }
        let vt = if a.chosen { format!("{} · 고름", fmt_pct(a.prob)) } else { fmt_pct(a.prob) };
        let va = txt(11.5, if a.chosen { INK } else { INK2 });
        s.text(xe + 6.0, cy + 4.5, &vt, if a.chosen { bold(va) } else { va });
    }
    if alts.is_empty() {
        no_data(&mut s, (lx0 + lx1) / 2.0, top + plot_h / 2.0);
    }

    // 오른쪽 판: 기여 항
    let (rx0, rx1) = (mid + 16.0, W - PAD);
    let tlab_w = ts.iter().map(|t| text_width(t.0, 12.5)).fold(0.0, f64::max).clamp(48.0, 160.0);
    let (cx0, cx1) = (rx0 + tlab_w + 8.0 + 44.0, rx1 - 48.0);
    s.text(rx0, 66.0, &format!("기여 항 (절댓값 상위 {})", ts.len()), bold(txt(13.0, INK)));
    let right_items = vec![item(Swatch::Rect(POSITIVE_COLOR.into()), "양(+) 기여"), item(Swatch::Rect(NEGATIVE_COLOR.into()), "음(−) 기여")];
    legend_row(&mut s, rx0, 74.0, rx1 - rx0, &right_items);
    let m = ts.iter().map(|t| t.1.abs()).fold(0.0, f64::max);
    let tr = nice_ticks(-m, m, 5);
    let u = unit_for(&tr);
    let xr = Lin::new(tr.lo, tr.hi, cx0, cx1);
    for v in &tr.values {
        let x = xr.map(*v);
        s.line(x, top - 4.0, x, top + plot_h, stroke(GRID, 1.0));
        s.text(x, top + plot_h + 18.0, &fmt_tick(*v, tr.step, u), txt_mid(11.0, INK2));
    }
    let xz = xr.map(0.0);
    s.line(xz, top - 4.0, xz, top + plot_h, stroke(AXIS, 1.2));
    for (i, (name, v)) in ts.iter().enumerate() {
        let cy = top + i as f64 * band + band / 2.0;
        s.text(rx0 + tlab_w, cy + 4.5, &truncate(name, 12.5, tlab_w), txt_end(12.5, INK));
        let xe = xr.map(*v);
        let color = if *v >= 0.0 { POSITIVE_COLOR } else { NEGATIVE_COLOR };
        if (xe - xz).abs() >= 0.01 {
            let tip = format!("{name}: {}", fmt_signed(*v));
            s.path_tip(&hbar_path(cy - bar_h / 2.0, bar_h, xz, xe, 4.0), fill(color), Some(&tip));
        }
        if *v >= 0.0 {
            s.text(xe + 6.0, cy + 4.5, &fmt_signed(*v), txt(11.5, INK2));
        } else {
            s.text(xe - 6.0, cy + 4.5, &fmt_signed(*v), txt_end(11.5, INK2));
        }
    }
    if ts.is_empty() {
        no_data(&mut s, (rx0 + rx1) / 2.0, top + plot_h / 2.0);
    }
    s.finish()
}

// ============================================================================
// 단위 시험
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::check_well_formed;

    fn wf(s: &str) {
        if let Err(e) = check_well_formed(s) {
            panic!("정형이 아님: {e}\n{s}");
        }
        assert!(s.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    }

    #[test]
    fn nice_ticks_are_1_2_5() {
        let t = nice_ticks(0.0, 97.0, 6);
        assert_eq!(t.step, 20.0);
        assert_eq!(t.values, vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        let t = nice_ticks(0.0, 1.0, 6);
        assert!((t.step - 0.2).abs() < 1e-12);
        assert_eq!(t.values.len(), 6);
        let t = nice_ticks(-3.0, 7.0, 6);
        assert_eq!(t.step, 2.0);
        assert_eq!(t.lo, -4.0);
        assert_eq!(t.hi, 8.0);
        let t = nice_ticks(5.0, 5.0, 6);
        assert!(t.lo <= 5.0 && t.hi >= 5.0 && t.lo < t.hi);
        let t = nice_ticks(f64::NAN, 1.0, 6);
        assert_eq!((t.lo, t.hi), (0.0, 1.0));
        let t = nice_ticks(0.0, 0.0, 6);
        assert_eq!((t.lo, t.hi), (0.0, 1.0));
        for raw in [0.3, 1.0, 2.2, 7.0, 45.0, 1234.0, 0.004] {
            let st = nice_step(raw);
            let m = st / 10f64.powi(st.log10().floor() as i32);
            assert!([1.0, 2.0, 5.0].iter().any(|x| (m - x).abs() < 1e-9), "{raw} → {st}");
            assert!(st >= raw);
        }
    }

    #[test]
    fn joint_ticks_align_and_cover() {
        let (l, r) = joint_ticks(230.0, 1010.0);
        assert_eq!(l.values.len(), r.values.len());
        assert!(l.hi >= 230.0 && r.hi >= 1010.0);
        assert_eq!((l.step, r.step), (50.0, 200.0));
        let (l, r) = joint_ticks(0.0, f64::NAN);
        assert_eq!(l.values.len(), r.values.len());
    }

    #[test]
    fn tick_and_value_formats() {
        let t = nice_ticks(0.0, 300_000.0, 6);
        let u = unit_for(&t);
        assert_eq!(fmt_tick(100_000.0, t.step, u), "10만");
        let t = nice_ticks(0.0, 4000.0, 6);
        assert_eq!(fmt_tick(2000.0, t.step, unit_for(&t)), "2,000");
        let t = nice_ticks(0.0, 1.0, 6);
        assert_eq!(fmt_tick(0.4, t.step, unit_for(&t)), "0.4");
        assert_eq!(fmt_tick_pct(0.4, t.step), "40%");
        assert_eq!(fmt_value(123_456.0), "12.3만");
        assert_eq!(fmt_value(2.5e8), "2.5억");
        assert_eq!(fmt_value(1234.0), "1,234");
        assert_eq!(fmt_value(12.34), "12.3");
        assert_eq!(fmt_value(1.234), "1.23");
        assert_eq!(fmt_value(-0.001), "0");
        assert_eq!(fmt_value(f64::NAN), "—");
        assert_eq!(fmt_pct(0.1234), "12.3%");
        assert_eq!(fmt_pct(-0.0001), "0.0%");
        assert_eq!(fmt_signed(0.5), "+0.5");
        assert_eq!(fmt_signed(-0.5), "−0.5");
        assert_eq!(fmt_signed(0.0), "0");
        assert_eq!(group_digits("-1234567.5"), "-1,234,567.5");
        assert_eq!(fmt_value(-4.0), "−4");
        assert_eq!(fmt_value(-1234.0), "−1,234");
        let t = nice_ticks(-1.0, 1.0, 5);
        assert_eq!(fmt_tick(-0.5, t.step, unit_for(&t)), "−0.5");
        assert_eq!(fmt_tick_pct(-0.2, 0.1), "−20%");
        assert_eq!(fmt_pct(-0.2), "−20.0%");
    }

    #[test]
    fn map_labels_flip_at_right_edge_and_are_not_clipped() {
        let base = MapBase { bbox: (0.0, 0.0, 100.0, 100.0), ..Default::default() };
        let portal = |x: f64, name: &str| PortalPoint { pos: (x, 50.0), kind: "지하철".into(), name: name.into() };
        let out = space_map("이름", &base, &[], &[portal(99.0, "오른쪽 끝 출구"), portal(10.0, "왼쪽 출구"), portal(500.0, "지도 밖")]);
        wf(&out);
        let right = out.lines().find(|l| l.ends_with(">오른쪽 끝 출구</text>")).unwrap();
        assert!(right.contains("text-anchor=\"end\""));
        let left = out.lines().find(|l| l.ends_with(">왼쪽 출구</text>")).unwrap();
        assert!(!left.contains("text-anchor"));
        assert!(!out.contains(">지도 밖</text>"), "지도 밖 점은 이름을 쓰지 않는다");
        // 이름은 자르기 묶음이 닫힌 뒤에 쓴다.
        let clip_end = out.find("</g>\n<text").unwrap();
        assert!(out.find(right).unwrap() > clip_end);
        // 축척 막대와 북쪽 표시는 지도 아래에 있다.
        assert!(out.contains(">북</text>"));
    }

    #[test]
    fn ordinal_colors_match_ramp_ends() {
        assert_eq!(ordinal_color(0, 6), ORDINAL_RAMP[0]);
        assert_eq!(ordinal_color(5, 6), ORDINAL_RAMP[9]);
        assert_eq!(ordinal_color(9, 6), ORDINAL_RAMP[9]);
        assert_eq!(ordinal_color(0, 1), ORDINAL_RAMP[4]);
        let six: Vec<String> = (0..6).map(|k| ordinal_color(k, 6)).collect();
        assert_eq!(six, vec!["#86b6ef", "#5a9be8", "#307edc", "#2164b7", "#164c91", "#0d366b"]);
    }

    #[test]
    fn palette_folds_beyond_ten() {
        let pal = Palette::new((0..12).map(|i| format!("c{i}")));
        assert_eq!(pal.color("c0"), CATEGORICAL[0]);
        assert_eq!(pal.color("c9"), CATEGORICAL[9]);
        assert_eq!(pal.color("c10"), OTHER_COLOR);
        assert_eq!(pal.color("없음"), OTHER_COLOR);
        let items = pal.items(Swatch::Dot, &|_| Some(1));
        assert_eq!(items.len(), 11);
        assert_eq!(items[10].label, "기타 2종 (2)");
    }

    #[test]
    fn density_class_thresholds() {
        let th = [0.31, 0.43, 0.72, 1.08, 2.17];
        assert_eq!(density_class(0.0, &th), 0);
        assert_eq!(density_class(-1.0, &th), 0);
        assert_eq!(density_class(0.3099, &th), 0);
        assert_eq!(density_class(0.31, &th), 1);
        assert_eq!(density_class(0.43, &th), 2);
        assert_eq!(density_class(0.4299, &th), 1);
        assert_eq!(density_class(1.08, &th), 4);
        assert_eq!(density_class(2.1699, &th), 4);
        assert_eq!(density_class(2.17, &th), 5);
        assert_eq!(density_class(100.0, &th), 5);
        assert_eq!(density_class(1.0, &[]), 0);
        assert_eq!(density_class(1.0, &[f64::NAN, 0.5]), 1);
        assert_eq!(class_range(0, &th), "< 0.31");
        assert_eq!(class_range(2, &th), "0.43–0.72");
        assert_eq!(class_range(5, &th), "≥ 2.17");
    }

    #[test]
    fn density_map_strokes_by_class() {
        let th = [0.31, 0.43, 0.72, 1.08, 2.17];
        let edge = |x: f64, v: f64| EdgeValue { pts: vec![(x, 0.0), (x, 10.0)], value: v };
        let edges = vec![edge(1.0, 0.30), edge(2.0, 0.31), edge(3.0, 0.31), edge(4.0, 2.17), edge(5.0, f64::NAN)];
        let base = MapBase { bbox: (0.0, 0.0, 10.0, 10.0), ..Default::default() };
        let out = density_map("밀도", &base, &edges, &th, &["A", "B", "C", "D", "E", "F"]);
        wf(&out);
        let class_paths = |k: usize| {
            let tag = format!("data-class=\"{k}\"");
            out.lines().filter(|l| l.contains(&tag)).map(|l| l.matches('M').count()).sum::<usize>()
        };
        assert_eq!(class_paths(0), 1);
        assert_eq!(class_paths(1), 2);
        assert_eq!(class_paths(5), 1);
        assert_eq!(class_paths(2) + class_paths(3) + class_paths(4), 0);
        assert!(out.contains("A  &lt; 0.31  (1)"));
        assert!(out.contains("B  0.31–0.43  (2)"));
        assert!(out.contains("F  ≥ 2.17  (1)"));
        assert!(out.contains(&format!("stroke=\"{}\"", ordinal_color(5, 6))));
    }

    #[test]
    fn box_stats_known_dataset() {
        let st = box_stats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]).unwrap();
        assert_eq!((st.q1, st.median, st.q3), (3.0, 5.0, 7.0));
        assert_eq!((st.lo_whisker, st.hi_whisker), (1.0, 9.0));
        assert!(st.outliers.is_empty());
        let st = box_stats(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!((st.q1, st.median, st.q3), (1.75, 2.5, 3.25));
        // 바깥 값: IQR = 2, 상한 7 + 3 = 10 → 100은 바깥
        let st = box_stats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0, f64::NAN]).unwrap();
        assert_eq!(st.n, 10);
        assert_eq!(st.q1, 3.25);
        assert_eq!(st.median, 5.5);
        assert_eq!(st.q3, 7.75);
        assert_eq!(st.hi_whisker, 9.0);
        assert_eq!(st.outliers, vec![100.0]);
        assert_eq!(st.max, 100.0);
        assert!(box_stats(&[]).is_none());
        assert!(box_stats(&[f64::INFINITY]).is_none());
        let st = box_stats(&[7.0]).unwrap();
        assert_eq!((st.q1, st.median, st.q3, st.lo_whisker, st.hi_whisker), (7.0, 7.0, 7.0, 7.0, 7.0));
    }

    #[test]
    fn shares_normalize_to_one() {
        let sh = normalize_shares(&[1.0, 3.0]).unwrap();
        assert_eq!(sh, vec![0.25, 0.75]);
        let sh = normalize_shares(&[2.0, -1.0, f64::NAN, 2.0]).unwrap();
        assert_eq!(sh, vec![0.5, 0.0, 0.0, 0.5]);
        assert!(normalize_shares(&[0.0, 0.0]).is_none());
        assert!(normalize_shares(&[]).is_none());
        let rows = vec![
            StackRow { label: "20대".into(), parts: vec![("앞길".into(), 1.0), ("장소".into(), 3.0)] },
            StackRow { label: "빈 행".into(), parts: vec![("앞길".into(), 0.0)] },
        ];
        let out = stacked_share_bars("비율", &rows);
        wf(&out);
        assert!(out.contains(">25.0%<") && out.contains(">75.0%<"));
        assert!(out.contains("자료 없음"));
        // 같은 부분 이름은 같은 색
        assert_eq!(out.matches(&format!("fill=\"{}\"><title>20대 · 앞길", CATEGORICAL[0])).count(), 1);
    }

    #[test]
    fn simplify_keeps_corners() {
        let pts = vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (2.0, 2.0)];
        assert_eq!(simplify(&pts, 0.1), vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0)]);
        assert_eq!(decimate(&[(0.0, 0.0), (0.1, 0.0), (5.0, 0.0)], 1.0), vec![(0.0, 0.0), (5.0, 0.0)]);
        let runs = finite_runs(&[(0.0, 0.0), (f64::NAN, 0.0), (1.0, 1.0), (2.0, 2.0)]);
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn raster_blocks_merge_into_rects() {
        // 4×4 칸: 왼쪽 두 열은 1, 오른쪽 위 2×2는 2
        let data = vec![1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 0, 0, 1, 1, 0, 0];
        let r = Raster { origin: (0.0, 0.0), cell: 10.0, w: 4, h: 4, data };
        let base = MapBase { bbox: (0.0, 0.0, 40.0, 40.0), walk: Some(r), ..Default::default() };
        let out = space_map("격자", &base, &[], &[]);
        wf(&out);
        let walk = out.lines().find(|l| l.contains(&format!("fill=\"{WALK_FILL}\" shape-rendering"))).unwrap();
        assert_eq!(walk.matches('M').count(), 1, "세로로 합쳐 사각형 하나");
        let side = out.lines().find(|l| l.contains(&format!("fill=\"{SIDEWALK_FILL}\" shape-rendering"))).unwrap();
        assert_eq!(side.matches('M').count(), 1);
    }

    #[test]
    fn raster_with_short_data_is_safe() {
        let r = Raster { origin: (0.0, 0.0), cell: 0.5, w: 1_000_000, h: 1_000_000, data: vec![1; 10] };
        let base = MapBase { bbox: (0.0, 0.0, 0.0, 0.0), walk: Some(r), ..Default::default() };
        wf(&space_map("큰 격자", &base, &[], &[]));
    }

    #[test]
    fn ontology_columns_and_order() {
        assert_eq!(ontology_column("evidence"), 0);
        assert_eq!(ontology_column("persona"), 1);
        assert_eq!(ontology_column("capability"), 2);
        assert_eq!(ontology_column("모름"), 2);
        assert_eq!(ontology_column("interest_dim"), 3);
        let node = |id: &str, kind: &str| GNode { id: id.into(), label: id.into(), kind: kind.into(), detail: None };
        let nodes = vec![node("p1", "propensity"), node("c1", "capability"), node("u1", "custom"), node("i1", "interest")];
        let out = ontology_map("순서", &nodes, &[]);
        wf(&out);
        let y_of = |id: &str| {
            let line = out.lines().find(|l| l.contains(&format!("data-id=\"{id}\""))).unwrap();
            let y = line.split(" y=\"").nth(1).unwrap().split('"').next().unwrap();
            y.parse::<f64>().unwrap()
        };
        assert!(y_of("c1") < y_of("i1"));
        assert!(y_of("i1") < y_of("p1"));
        assert!(y_of("p1") < y_of("u1"));
    }

    #[test]
    fn wedge_geometry() {
        let w = wedge_path(0.0, 0.0, 10.0, 0.0, PI / 4.0);
        assert!(w.starts_with("M0 0L7.07 -7.07A10 10 0 0 1 7.07 7.07Z"), "{w}");
        let full = wedge_path(0.0, 0.0, 10.0, 0.0, PI);
        assert_eq!(full.matches('A').count(), 2);
    }

    #[test]
    fn bar_path_rounds_only_data_end() {
        let p = vbar_path(0.0, 10.0, 100.0, 50.0, 4.0);
        assert_eq!(p, "M0 100V54Q0 50 4 50H6Q10 50 10 54V100Z");
        let p = hbar_path(0.0, 10.0, 0.0, -20.0, 4.0);
        assert_eq!(p, "M0 0H-16Q-20 0 -20 4V6Q-20 10 -16 10H0Z");
    }
}
