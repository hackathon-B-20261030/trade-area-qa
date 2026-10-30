//! 낮은 수준의 SVG 작성 도구.
//!
//! 요소 쓰기([`Svg`], [`Attrs`], [`PathData`]), XML 이스케이프([`esc`]),
//! 고정 숫자 서식([`num`], [`num1`], [`fixed`]), 글자 폭 추정([`text_width`], [`truncate`])을 담는다.
//! 숫자는 고정 자릿수로 반올림하고 끝의 0을 지우므로 같은 값은 언제나 같은 글자열이 된다.

use std::fmt::Write as _;

/// SVG 이름공간 URI. 출력에 들어가는 유일한 `http://` 문자열이다.
pub const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// 모든 글자에 쓰는 글꼴 목록. 외부 글꼴을 불러오지 않고 설치된 한글 글꼴을 쓴다.
pub const FONT_FAMILY: &str =
    "\"Apple SD Gothic Neo\",\"Noto Sans KR\",\"Malgun Gothic\",sans-serif";

/// 그림 바탕색(거의 흰색).
pub const SURFACE: &str = "#fcfcfb";

/// 기본 글자색.
pub const INK: &str = "#0b0b0b";

/// 숫자 서식의 절댓값 상한. 이보다 큰 값은 이 값으로 자른다.
const NUM_LIMIT: f64 = 1.0e9;

/// XML 특수 문자(`& < > " '`)를 문자 참조로 바꾼다.
///
/// XML 1.0이 허용하지 않는 제어 문자는 버리고, 탭·줄바꿈은 공백으로 바꾼다.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(' '),
            '\u{FFFE}' | '\u{FFFF}' => {}
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// `decimals` 자리로 반올림해 쓰고 끝의 0과 소수점을 지운다.
///
/// 유한하지 않은 값은 "0"이고, "-0"은 "0"으로 쓴다. 절댓값은 10억에서 자른다.
pub fn fixed(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let v = v.clamp(-NUM_LIMIT, NUM_LIMIT);
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

/// 좌표·크기용 숫자(소수 둘째 자리).
pub fn num(v: f64) -> String {
    fixed(v, 2)
}

/// 조밀한 도형(격자·건물·궤적)용 숫자(소수 첫째 자리).
pub fn num1(v: f64) -> String {
    fixed(v, 1)
}

/// 속성 목록 작성기. 문자열 값은 항상 이스케이프하고 숫자 값은 [`num`]으로 쓴다.
///
/// 속성 이름은 코드 안의 상수만 쓴다(사용자 입력을 이름으로 쓰지 않는다).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attrs(String);

impl Attrs {
    pub fn new() -> Attrs {
        Attrs(String::new())
    }

    /// 문자열 속성(값을 이스케이프한다).
    pub fn s(mut self, key: &str, val: &str) -> Attrs {
        let _ = write!(self.0, " {key}=\"{}\"", esc(val));
        self
    }

    /// 숫자 속성(소수 둘째 자리).
    pub fn n(mut self, key: &str, val: f64) -> Attrs {
        let _ = write!(self.0, " {key}=\"{}\"", num(val));
        self
    }

    /// 다른 속성 목록을 뒤에 붙인다.
    pub fn join(mut self, other: &Attrs) -> Attrs {
        self.0.push_str(&other.0);
        self
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// SVG 문서 작성기.
///
/// [`Svg::new`]가 루트 요소, `<title>`, 바탕 사각형을 쓰고 [`Svg::finish`]가 열린 요소를 모두 닫는다.
/// 요소마다 줄을 바꿔 써서 사람이 읽고 비교하기 쉽다.
#[derive(Debug)]
pub struct Svg {
    buf: String,
    width: f64,
    height: f64,
    open: Vec<&'static str>,
}

impl Svg {
    /// 크기가 `width`×`height`인 문서를 연다. 크기가 유효하지 않으면 960×600을 쓴다.
    pub fn new(width: f64, height: f64, title: &str) -> Svg {
        let width = if width.is_finite() && width > 0.0 { width } else { 960.0 };
        let height = if height.is_finite() && height > 0.0 { height } else { 600.0 };
        let (w, h) = (num(width), num(height));
        let mut buf = String::with_capacity(32 * 1024);
        let _ = writeln!(
            buf,
            "<svg xmlns=\"{SVG_NS}\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" role=\"img\" font-family=\"{}\" font-size=\"12\" fill=\"{INK}\">",
            esc(FONT_FAMILY)
        );
        let _ = writeln!(buf, "<title>{}</title>", esc(title));
        let _ = writeln!(buf, "<rect width=\"{w}\" height=\"{h}\" fill=\"{SURFACE}\"/>");
        Svg { buf, width, height, open: Vec::new() }
    }

    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    /// 빈 요소 `<tag .../>`.
    pub fn el(&mut self, tag: &str, a: &Attrs) {
        let _ = writeln!(self.buf, "<{tag}{}/>", a.as_str());
    }

    /// 글자를 담은 요소 `<tag ...>글자</tag>`(글자를 이스케이프한다).
    pub fn el_text(&mut self, tag: &str, a: &Attrs, text: &str) {
        let _ = writeln!(self.buf, "<{tag}{}>{}</{tag}>", a.as_str(), esc(text));
    }

    /// 도움말(`<title>`)이 붙은 요소. 브라우저에서 마우스를 올리면 보인다.
    pub fn el_tip(&mut self, tag: &str, a: &Attrs, tip: Option<&str>) {
        match tip {
            Some(t) => {
                let _ = writeln!(self.buf, "<{tag}{}><title>{}</title></{tag}>", a.as_str(), esc(t));
            }
            None => self.el(tag, a),
        }
    }

    /// 여는 태그. [`Svg::close`]로 닫는다.
    pub fn open(&mut self, tag: &'static str, a: &Attrs) {
        let _ = writeln!(self.buf, "<{tag}{}>", a.as_str());
        self.open.push(tag);
    }

    /// 가장 최근에 연 태그를 닫는다.
    pub fn close(&mut self) {
        if let Some(tag) = self.open.pop() {
            let _ = writeln!(self.buf, "</{tag}>");
        }
    }

    /// 이미 이스케이프된 신뢰된 표시를 그대로 붙인다(이 크레이트가 만든 표시만 넣는다).
    pub fn raw(&mut self, markup: &str) {
        self.buf.push_str(markup);
    }

    /// 열린 요소를 모두 닫고 문서를 돌려준다.
    pub fn finish(mut self) -> String {
        while let Some(tag) = self.open.pop() {
            let _ = writeln!(self.buf, "</{tag}>");
        }
        self.buf.push_str("</svg>\n");
        self.buf
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, a: Attrs) {
        self.rect_tip(x, y, w, h, a, None);
    }

    pub fn rect_tip(&mut self, x: f64, y: f64, w: f64, h: f64, a: Attrs, tip: Option<&str>) {
        let g = Attrs::new()
            .n("x", x)
            .n("y", y)
            .n("width", w.max(0.0))
            .n("height", h.max(0.0))
            .join(&a);
        self.el_tip("rect", &g, tip);
    }

    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, a: Attrs) {
        let g = Attrs::new().n("x1", x1).n("y1", y1).n("x2", x2).n("y2", y2).join(&a);
        self.el("line", &g);
    }

    pub fn circle(&mut self, cx: f64, cy: f64, r: f64, a: Attrs) {
        self.circle_tip(cx, cy, r, a, None);
    }

    pub fn circle_tip(&mut self, cx: f64, cy: f64, r: f64, a: Attrs, tip: Option<&str>) {
        let g = Attrs::new().n("cx", cx).n("cy", cy).n("r", r.max(0.0)).join(&a);
        self.el_tip("circle", &g, tip);
    }

    pub fn path(&mut self, d: &str, a: Attrs) {
        self.path_tip(d, a, None);
    }

    pub fn path_tip(&mut self, d: &str, a: Attrs, tip: Option<&str>) {
        let g = Attrs::new().s("d", d).join(&a);
        self.el_tip("path", &g, tip);
    }

    /// 꺾은선(좌표는 소수 첫째 자리).
    pub fn polyline(&mut self, pts: &[(f64, f64)], a: Attrs) {
        let mut p = String::with_capacity(pts.len() * 12);
        for (i, (x, y)) in pts.iter().enumerate() {
            if i > 0 {
                p.push(' ');
            }
            let _ = write!(p, "{},{}", num1(*x), num1(*y));
        }
        let g = Attrs::new().s("points", &p).join(&a);
        self.el("polyline", &g);
    }

    /// 글자(이스케이프한다).
    pub fn text(&mut self, x: f64, y: f64, s: &str, a: Attrs) {
        let g = Attrs::new().n("x", x).n("y", y).join(&a);
        self.el_text("text", &g, s);
    }
}

/// 경로 `d` 속성 작성기.
///
/// [`PathData::coarse`]는 소수 첫째 자리로 써서 조밀한 도형(격자·건물·간선)의 크기를 줄인다.
#[derive(Clone, Debug, Default)]
pub struct PathData {
    d: String,
    coarse: bool,
}

impl PathData {
    pub fn new() -> PathData {
        PathData::default()
    }

    pub fn coarse() -> PathData {
        PathData { d: String::new(), coarse: true }
    }

    fn f(&self, v: f64) -> String {
        if self.coarse {
            num1(v)
        } else {
            num(v)
        }
    }

    pub fn m(&mut self, x: f64, y: f64) -> &mut PathData {
        let (a, b) = (self.f(x), self.f(y));
        let _ = write!(self.d, "M{a} {b}");
        self
    }

    pub fn l(&mut self, x: f64, y: f64) -> &mut PathData {
        let (a, b) = (self.f(x), self.f(y));
        let _ = write!(self.d, "L{a} {b}");
        self
    }

    /// 절대 가로 이동(`H`).
    pub fn h_abs(&mut self, x: f64) -> &mut PathData {
        let a = self.f(x);
        let _ = write!(self.d, "H{a}");
        self
    }

    /// 절대 세로 이동(`V`).
    pub fn v_abs(&mut self, y: f64) -> &mut PathData {
        let a = self.f(y);
        let _ = write!(self.d, "V{a}");
        self
    }

    /// 2차 베지어(`Q`).
    pub fn q(&mut self, cx: f64, cy: f64, x: f64, y: f64) -> &mut PathData {
        let (a, b, c, d) = (self.f(cx), self.f(cy), self.f(x), self.f(y));
        let _ = write!(self.d, "Q{a} {b} {c} {d}");
        self
    }

    /// 3차 베지어(`C`).
    pub fn c(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) -> &mut PathData {
        let (a, b, c, d, e, f) = (self.f(x1), self.f(y1), self.f(x2), self.f(y2), self.f(x), self.f(y));
        let _ = write!(self.d, "C{a} {b} {c} {d} {e} {f}");
        self
    }

    /// 원호(`A`, 가로·세로 반지름이 같고 회전 0).
    pub fn arc(&mut self, r: f64, large: bool, sweep: bool, x: f64, y: f64) -> &mut PathData {
        let (rr, a, b) = (self.f(r), self.f(x), self.f(y));
        let (lf, sf) = (u8::from(large), u8::from(sweep));
        let _ = write!(self.d, "A{rr} {rr} 0 {lf} {sf} {a} {b}");
        self
    }

    pub fn z(&mut self) -> &mut PathData {
        self.d.push('Z');
        self
    }

    pub fn is_empty(&self) -> bool {
        self.d.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.d
    }
}

/// 글자 하나의 폭 추정값(em). 배치 계산용 근사이며 한글은 거의 정사각형으로 본다.
pub fn char_em(c: char) -> f64 {
    match c {
        ' ' => 0.3,
        'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' => 0.3,
        '0'..='9' => 0.58,
        'm' | 'w' | 'M' | 'W' => 0.85,
        'A'..='Z' => 0.68,
        'a'..='z' => 0.55,
        '(' | ')' | '[' | ']' | '{' | '}' | '-' | '/' => 0.38,
        '%' => 0.85,
        '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}' => 0.95,
        '\u{2E80}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{FF00}'..='\u{FFEF}' => 1.0,
        '…' | '—' | '·' | '→' | '≥' | '−' | '–' => 0.9,
        c if c.is_ascii() => 0.6,
        _ => 0.9,
    }
}

/// 글자열 폭 추정값(픽셀). `size`는 글자 크기(px)다.
pub fn text_width(s: &str, size: f64) -> f64 {
    s.chars().map(char_em).sum::<f64>() * size
}

/// 폭이 `max_w`를 넘으면 뒤를 잘라 '…'을 붙인다. 넘지 않으면 그대로 돌려준다.
pub fn truncate(s: &str, size: f64, max_w: f64) -> String {
    if text_width(s, size) <= max_w {
        return s.to_string();
    }
    let ell = char_em('…') * size;
    let mut w = 0.0;
    let mut out = String::new();
    for c in s.chars() {
        let cw = char_em(c) * size;
        if w + cw + ell > max_w {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// FNV-1a 32비트 해시. 그림 안 `id`의 접두어를 결정적으로 만드는 데 쓴다.
pub fn fnv1a32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::check_well_formed;

    #[test]
    fn esc_replaces_all_specials() {
        assert_eq!(esc("a<b&c>\"d'"), "a&lt;b&amp;c&gt;&quot;d&apos;");
        assert_eq!(esc("줄\n바꿈\u{1}"), "줄 바꿈");
        assert_eq!(esc("명동"), "명동");
    }

    #[test]
    fn num_is_fixed_and_trimmed() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(1.256), "1.26");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(f64::INFINITY), "0");
        assert_eq!(num(-12.3), "-12.3");
        assert_eq!(num1(7.654), "7.7");
        assert_eq!(num(1e30), "1000000000");
        assert_eq!(fixed(12.0, 0), "12");
    }

    #[test]
    fn attrs_escape_values() {
        let a = Attrs::new().s("fill", "#fff").s("data-x", "<&\">").n("x", 1.234);
        assert_eq!(a.as_str(), " fill=\"#fff\" data-x=\"&lt;&amp;&quot;&gt;\" x=\"1.23\"");
    }

    #[test]
    fn svg_document_is_well_formed() {
        let mut s = Svg::new(100.0, 50.0, "제목 <&>");
        s.open("g", &Attrs::new().s("fill", "none"));
        s.rect(0.0, 0.0, 10.0, 10.0, Attrs::new());
        s.rect_tip(0.0, 0.0, -5.0, 10.0, Attrs::new(), Some("도움말 \"따옴표\""));
        s.text(5.0, 5.0, "a < b", Attrs::new());
        let mut p = PathData::new();
        p.m(0.0, 0.0).l(1.0, 1.0).q(1.0, 2.0, 3.0, 4.0).c(1.0, 2.0, 3.0, 4.0, 5.0, 6.0).arc(5.0, true, false, 1.0, 1.0).z();
        s.path(p.as_str(), Attrs::new());
        s.polyline(&[(0.0, 0.0), (1.26, 2.5)], Attrs::new());
        // 닫지 않은 g는 finish가 닫는다.
        let out = s.finish();
        assert!(out.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"50\" viewBox=\"0 0 100 50\""));
        assert!(out.contains("<title>제목 &lt;&amp;&gt;</title>"));
        assert!(out.contains("width=\"0\""), "음수 폭은 0으로 쓴다");
        assert!(out.contains("points=\"0,0 1.3,2.5\""));
        check_well_formed(&out).unwrap();
    }

    #[test]
    fn invalid_size_falls_back() {
        let s = Svg::new(f64::NAN, -1.0, "");
        assert_eq!(s.width(), 960.0);
        assert_eq!(s.height(), 600.0);
        check_well_formed(&s.finish()).unwrap();
    }

    #[test]
    fn text_width_and_truncate() {
        assert!(text_width("명동", 10.0) > text_width("ab", 10.0));
        assert_eq!(truncate("짧은", 12.0, 100.0), "짧은");
        let t = truncate("아주 긴 장소 이름이 들어갑니다", 12.0, 60.0);
        assert!(t.ends_with('…'));
        assert!(text_width(&t, 12.0) <= 60.0 + 1e-9);
        assert_eq!(truncate("가나다", 12.0, 0.0), "…");
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        assert_eq!(fnv1a32("a"), 0xe40c_292c);
    }
}
