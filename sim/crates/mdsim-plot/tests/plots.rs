//! mdsim-plot 통합 시험: 그림 13장과 보고서(02d §5의 E228-3·4·5·6 중 plot 크레이트 몫).

#[path = "../src/testutil.rs"]
mod testutil;

mod common;

use common::{figures, report, report_labels, report_summary, Fig};
use mdsim_plot::*;
use testutil::check_well_formed;

const NS: &str = "http://www.w3.org/2000/svg";
const NASTY: &str = "a<b&c\"d'e>";
const NASTY_ESC: &str = "a&lt;b&amp;c&quot;d&apos;e&gt;";

fn assert_svg_contract(name: &str, s: &str) {
    if let Err(e) = check_well_formed(s) {
        panic!("{name}: 정형이 아님: {e}");
    }
    assert!(s.starts_with(&format!("<svg xmlns=\"{NS}\"")), "{name}: 루트");
    let head = s.lines().next().unwrap_or("");
    for attr in [" width=\"", " height=\"", " viewBox=\"0 0 ", " font-family=\""] {
        assert!(head.contains(attr), "{name}: 루트에 {attr} 없음");
    }
    assert!(s.lines().nth(1).unwrap_or("").starts_with("<title>"), "{name}: <title>이 첫 자식이 아님");
    assert!(s.lines().nth(2).unwrap_or("").starts_with("<rect width="), "{name}: 바탕 사각형 없음");
    assert!(s.trim_end().ends_with("</svg>"), "{name}: 끝");
}

/// 루트의 width·height.
fn svg_size(s: &str) -> (f64, f64) {
    let head = s.lines().next().unwrap();
    let attr = |k: &str| -> f64 {
        let v = head.split(&format!(" {k}=\"")).nth(1).unwrap().split('"').next().unwrap();
        v.parse().unwrap()
    };
    (attr("width"), attr("height"))
}

fn attr_of(line: &str, key: &str) -> Option<String> {
    let pat = format!(" {key}=\"");
    let start = line.find(&pat)? + pat.len();
    let end = line[start..].find('"')?;
    Some(line[start..start + end].to_string())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// 글자 요소가 그림 안에 있는지(추정 폭 포함) 검사한다. `translate` 묶음은 누적해서 본다.
/// 회전한 글자는 기준점만 본다.
fn assert_text_inside(name: &str, s: &str) {
    let (w, h) = svg_size(s);
    let mut offsets: Vec<(f64, f64)> = Vec::new();
    for line in s.lines() {
        if line.starts_with("<g") && !line.ends_with("/>") {
            let off = attr_of(line, "transform")
                .and_then(|t| t.strip_prefix("translate(").map(|r| r.trim_end_matches(')').to_string()))
                .map(|r| {
                    let mut it = r.split(' ').map(|v| v.parse::<f64>().unwrap());
                    (it.next().unwrap(), it.next().unwrap())
                })
                .unwrap_or((0.0, 0.0));
            offsets.push(off);
            continue;
        }
        if line == "</g>" {
            offsets.pop();
            continue;
        }
        if !line.starts_with("<text ") {
            continue;
        }
        let (dx, dy) = offsets.iter().fold((0.0, 0.0), |a, o| (a.0 + o.0, a.1 + o.1));
        let x = attr_of(line, "x").unwrap().parse::<f64>().unwrap() + dx;
        let y = attr_of(line, "y").unwrap().parse::<f64>().unwrap() + dy;
        let size: f64 = attr_of(line, "font-size").map_or(12.0, |v| v.parse().unwrap());
        let content = unescape(line.split('>').nth(1).unwrap().split("</text").next().unwrap());
        let tw = mdsim_plot::svg::text_width(&content, size);
        assert!(y >= size * 0.6 - 0.5 && y <= h + 0.5, "{name}: 글자 y={y} 밖: {line}");
        if attr_of(line, "transform").is_some() {
            assert!(x >= -0.5 && x <= w + 0.5, "{name}: 회전 글자 x={x} 밖: {line}");
            continue;
        }
        let (left, right) = match attr_of(line, "text-anchor").as_deref() {
            Some("middle") => (x - tw / 2.0, x + tw / 2.0),
            Some("end") => (x - tw, x),
            _ => (x, x + tw),
        };
        assert!(left >= -1.0 && right <= w + 1.0, "{name}: 글자가 가로로 넘침 [{left:.1}, {right:.1}] / {w}: {line}");
    }
}

#[test]
fn all_figures_meet_svg_contract() {
    let figs = figures();
    assert_eq!(figs.len(), 13);
    for f in &figs {
        assert_svg_contract(f.stem, &f.svg);
        let (w, h) = svg_size(&f.svg);
        assert!((600.0..=1400.0).contains(&w) && (300.0..=1600.0).contains(&h), "{}: 크기 {w}×{h}", f.stem);
    }
}

#[test]
fn text_stays_inside_figures() {
    for f in figures() {
        assert_text_inside(f.stem, &f.svg);
    }
}

#[test]
fn e228_3_plots_byte_identical() {
    let a = figures();
    let b = figures();
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.svg, y.svg, "{}", x.stem);
    }
    assert_eq!(report(&a), report(&b));
}

#[test]
fn e228_4_report_contents() {
    let figs = figures();
    let r = report(&figs);
    check_well_formed(&r).unwrap();
    assert!(r.starts_with("<!doctype html>"));
    assert!(r.contains("<meta charset=\"utf-8\" />"));
    assert!(r.contains("name=\"viewport\""));
    assert!(r.contains("<style>"));
    assert!(r.contains("max-width:1000px"));
    for (k, v) in report_summary() {
        assert!(r.contains(&format!("<th scope=\"row\">{k}</th><td>{v}</td>")), "요약 {k}");
    }
    for l in report_labels() {
        assert!(r.contains(&format!("<li class=\"label\">{l}</li>")), "라벨 {l}");
    }
    assert_eq!(r.matches(&format!("<svg xmlns=\"{NS}\"")).count(), 13);
    for (i, f) in figs.iter().enumerate() {
        assert!(r.contains(&format!("<section class=\"fig\" id=\"fig-{:02}\">\n<h2>{}</h2>", i + 1, f.title)));
        assert!(r.contains(&format!("<figcaption>{}</figcaption>", f.caption)));
        assert!(r.contains(f.svg.trim()), "{} 인라인 SVG", f.stem);
    }
}

#[test]
fn e228_5_no_external_refs() {
    let figs = figures();
    let mut docs: Vec<(String, String)> = figs.iter().map(|f| (format!("{}.svg", f.stem), f.svg.clone())).collect();
    docs.push(("report.html".into(), report(&figs)));
    assert_eq!(docs.len(), 14);
    for (name, d) in &docs {
        let stripped = d.replace(NS, "");
        assert!(!stripped.contains("http://"), "{name}: http://");
        assert!(!stripped.contains("https://"), "{name}: https://");
        assert!(!d.contains("url(http"), "{name}: url(http");
        assert!(!d.contains("<image"), "{name}: <image");
        assert!(!d.contains("<script"), "{name}: <script");
        assert!(!d.contains("<link"), "{name}: <link");
        assert!(!d.contains("@import"), "{name}: @import");
        assert!(!d.contains("xlink:"), "{name}: xlink");
        for (i, _) in d.match_indices("url(") {
            assert!(d[i..].starts_with("url(#"), "{name}: 내부가 아닌 url(");
        }
        for (i, _) in d.match_indices("href=\"") {
            assert!(d[i + 6..].starts_with('#'), "{name}: 내부가 아닌 href");
        }
    }
}

#[test]
fn e228_6_ontology_map_chain() {
    let node = |id: &str, kind: &str| GNode { id: id.into(), label: format!("{id} 이름"), kind: kind.into(), detail: Some("설명".into()) };
    let nodes = vec![node("dim", "dimension"), node("trait", "sensitivity"), node("per", "persona"), node("ev", "evidence")];
    let e = |a: &str, b: &str| GEdge { from: a.into(), to: b.into(), label: format!("{a}→{b}") };
    let edges = vec![e("ev", "per"), e("per", "trait"), e("trait", "dim"), e("ev", "없음"), e("dim", "dim")];
    let out = ontology_map("사슬", &nodes, &edges);
    assert_svg_contract("p12", &out);
    let x_of = |id: &str| -> f64 {
        let line = out.lines().find(|l| l.contains(&format!("data-id=\"{id}\""))).unwrap();
        attr_of(line, "x").unwrap().parse().unwrap()
    };
    assert!(x_of("ev") < x_of("per"));
    assert!(x_of("per") < x_of("trait"));
    assert!(x_of("trait") < x_of("dim"));
    // 유효한 연결선 3개만 그린다(없는 마디·자기 자신 제외).
    assert_eq!(out.matches(" data-from=\"").count(), 3);
    for (a, b) in [("ev", "per"), ("per", "trait"), ("trait", "dim")] {
        assert!(out.contains(&format!("data-from=\"{a}\" data-to=\"{b}\"")));
        assert!(out.contains(&format!("{a}→{b}")));
    }
    for title in ["근거", "페르소나", "특성", "차원", "민감도"] {
        assert!(out.contains(&format!(">{title}</text>")), "{title}");
    }
}

/// 모든 그림을 같은 짓궂은 이름으로 만든다.
fn nasty_figures() -> Vec<(&'static str, String)> {
    let base = MapBase { bbox: (0.0, 0.0, 100.0, 80.0), ..Default::default() };
    let place = PlacePoint { pos: (10.0, 10.0), category: NASTY.into(), label: Some(NASTY.into()) };
    let portal = PortalPoint { pos: (20.0, 20.0), kind: NASTY.into(), name: NASTY.into() };
    let glyph = Glyph {
        pos: (50.0, 40.0),
        heading: 0.0,
        gaze: 0.5,
        focal_range: 10.0,
        focal_angle_deg: 60.0,
        group: NASTY.into(),
        looking_at: Some((60.0, 45.0)),
        activity: NASTY.into(),
    };
    let panel = SnapshotPanel { caption: NASTY.into(), glyphs: vec![glyph], noticed: vec![(60.0, 45.0)], places: vec![place.clone()] };
    let ser = |name: &str, v: Vec<f64>| Series { name: name.into(), values: v };
    vec![
        ("p01", space_map(NASTY, &base, std::slice::from_ref(&place), &[portal])),
        ("p02", bar_line_chart(NASTY, &[NASTY.into(), "b".into(), "c".into()], &ser(NASTY, vec![1.0, 2.0, 3.0]), &ser(NASTY, vec![3.0, 2.0, 1.0]), NASTY, NASTY)),
        ("p03", density_map(NASTY, &base, &[EdgeValue { pts: vec![(0.0, 0.0), (50.0, 50.0)], value: 1.0 }], &[0.5], &[NASTY, NASTY])),
        ("p04", trajectories(NASTY, &base, &[Track { pts: vec![(0.0, 0.0), (50.0, 50.0)], group: NASTY.into() }])),
        ("p05", snapshot_strip(NASTY, &base, (30.0, 20.0, 70.0, 60.0), &[panel])),
        ("p06", stacked_share_bars(NASTY, &[StackRow { label: NASTY.into(), parts: vec![(NASTY.into(), 1.0), ("b".into(), 1.0)] }])),
        ("p07", grouped_bars(NASTY, &GroupedBars { categories: vec![NASTY.into(), "b".into()], series: vec![ser(NASTY, vec![1.0, 2.0]), ser("b", vec![2.0, 1.0])] }, NASTY)),
        ("p08", hbar_chart(NASTY, &[HBar { label: NASTY.into(), value: 3.0, secondary: Some(0.5) }], NASTY, Some(NASTY))),
        ("p09", box_plot(NASTY, &[Dist { label: NASTY.into(), values: vec![1.0, 2.0, 3.0, 4.0] }], NASTY)),
        ("p10", scatter_with_curve(NASTY, &[(1.0, 1.0), (2.0, 2.0)], &[(0.0, 0.0), (3.0, 3.0)], NASTY, NASTY, NASTY)),
        (
            "p12",
            ontology_map(
                NASTY,
                &[
                    GNode { id: NASTY.into(), label: NASTY.into(), kind: "persona".into(), detail: Some(NASTY.into()) },
                    GNode { id: "d".into(), label: "d".into(), kind: NASTY.into(), detail: None },
                ],
                &[GEdge { from: NASTY.into(), to: "d".into(), label: NASTY.into() }],
            ),
        ),
        ("p13", decision_chart(NASTY, &[AltBar { label: NASTY.into(), prob: 0.6, chosen: true }], &[(NASTY.into(), -0.3)])),
    ]
}

#[test]
fn labels_are_escaped_everywhere() {
    let mut docs = nasty_figures();
    docs.push((
        "report",
        report_html(
            NASTY,
            &[(NASTY.into(), NASTY.into())],
            &[NASTY.into()],
            &[ReportSection { title: NASTY.into(), caption: NASTY.into(), svg: docs[0].1.clone() }],
        ),
    ));
    for (name, d) in &docs {
        check_well_formed(d).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(d.contains(NASTY_ESC), "{name}: 이스케이프된 이름 없음");
        assert!(!d.contains(NASTY), "{name}: 날 이름이 남음");
    }
    for (name, d) in nasty_figures() {
        assert!(d.contains(&format!("<title>{NASTY_ESC}</title>")), "{name}: <title>");
    }
}

fn all_empty() -> Vec<(&'static str, String)> {
    let b = MapBase::default();
    let s = Series::default();
    vec![
        ("p01", space_map("", &b, &[], &[])),
        ("p02", bar_line_chart("", &[], &s, &s, "", "")),
        ("p03", density_map("", &b, &[], &[], &[])),
        ("p04", trajectories("", &b, &[])),
        ("p05", snapshot_strip("", &b, (0.0, 0.0, 0.0, 0.0), &[])),
        ("p06", stacked_share_bars("", &[])),
        ("p07", grouped_bars("", &GroupedBars::default(), "")),
        ("p08", hbar_chart("", &[], "", None)),
        ("p09", box_plot("", &[], "")),
        ("p10", scatter_with_curve("", &[], &[], "", "", "")),
        ("p11", grouped_bars("", &GroupedBars { categories: vec!["명동".into()], series: vec![] }, "")),
        ("p12", ontology_map("", &[], &[])),
        ("p13", decision_chart("", &[], &[])),
        ("report", report_html("", &[], &[], &[])),
    ]
}

#[test]
fn empty_inputs_render_no_data() {
    for (name, d) in all_empty() {
        check_well_formed(&d).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(d.contains("자료 없음"), "{name}: '자료 없음' 없음");
    }
}

#[test]
fn base_without_data_renders_no_data() {
    let b = common::base();
    let panel = SnapshotPanel { caption: "빈 장".into(), ..Default::default() };
    let docs = [
        ("p01", space_map("장소 없음", &b, &[], &[])),
        ("p03", density_map("간선 없음", &b, &[], &common::LOS_THRESHOLDS, &common::LOS_LABELS)),
        ("p04", trajectories("궤적 없음", &b, &[])),
        ("p05", snapshot_strip("몸 없음", &b, common::snapshot_clip(), &[panel])),
    ];
    for (name, d) in docs {
        assert_svg_contract(name, &d);
        assert!(d.contains("자료 없음"), "{name}");
    }
}

#[test]
fn degenerate_inputs_do_not_panic() {
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    let zero_box = MapBase { bbox: (5.0, 5.0, 5.0, 5.0), ..Default::default() };
    let nan_box = MapBase { bbox: (nan, 0.0, inf, 1.0), buildings: vec![vec![(nan, 1.0), (2.0, 2.0)]], edges: vec![vec![(inf, 0.0)]], walk: None };
    let one_place = [PlacePoint { pos: (5.0, 5.0), category: "카페".into(), label: None }];
    let bad_place = [PlacePoint { pos: (nan, 5.0), category: "카페".into(), label: None }];
    let glyph = |pos: (f64, f64)| Glyph { pos, heading: nan, gaze: inf, focal_range: nan, focal_angle_deg: -5.0, group: "g".into(), looking_at: Some((inf, 0.0)), activity: String::new() };
    let full_circle = Glyph { pos: (1.0, 1.0), heading: 0.0, gaze: 0.0, focal_range: 3.0, focal_angle_deg: 720.0, group: "g".into(), looking_at: None, activity: "a".into() };
    let ser = |v: Vec<f64>| Series { name: "s".into(), values: v };
    let many_cats: Vec<PlacePoint> = (0..15).map(|i| PlacePoint { pos: (i as f64, i as f64), category: format!("업종{i}"), label: None }).collect();
    let docs: Vec<(&str, String)> = vec![
        ("p01 zero bbox", space_map("t", &zero_box, &one_place, &[])),
        ("p01 nan bbox", space_map("t", &nan_box, &bad_place, &[])),
        ("p01 15 categories", space_map("t", &MapBase::default(), &many_cats, &[])),
        ("p02 nan", bar_line_chart("t", &["a".into()], &ser(vec![nan, inf]), &ser(vec![nan]), "", "")),
        ("p02 zeros", bar_line_chart("t", &["a".into(), "b".into()], &ser(vec![0.0, 0.0]), &ser(vec![0.0, 0.0]), "", "")),
        ("p02 negative", bar_line_chart("t", &[], &ser(vec![-3.0, 2.0, nan, 5.0]), &ser(vec![1.0, -1.0, nan, 4.0]), "", "")),
        ("p02 huge", bar_line_chart("t", &[], &ser(vec![1e300, 1.0]), &ser(vec![-1e300]), "", "")),
        ("p03 nan", density_map("t", &zero_box, &[EdgeValue { pts: vec![(nan, 0.0), (1.0, 1.0), (2.0, 3.0)], value: inf }, EdgeValue { pts: vec![(1.0, 1.0)], value: 1.0 }], &[nan, 1.0], &[])),
        ("p03 one point", density_map("t", &MapBase::default(), &[EdgeValue { pts: vec![(3.0, 3.0), (3.0, 3.0)], value: 0.5 }], &[], &[])),
        ("p04 nan", trajectories("t", &zero_box, &[Track { pts: vec![(nan, nan), (1.0, 1.0), (inf, 2.0), (2.0, 2.0), (3.0, 3.0)], group: String::new() }])),
        ("p05 degenerate", snapshot_strip("t", &MapBase::default(), (nan, 0.0, 0.0, 0.0), &[SnapshotPanel { caption: String::new(), glyphs: vec![glyph((1.0, 1.0)), glyph((nan, 1.0)), full_circle], noticed: vec![(nan, nan)], places: vec![] }])),
        ("p05 seven panels", snapshot_strip("t", &common::base(), common::snapshot_clip(), &vec![common::snapshots()[0].clone(); 7])),
        ("p05 only nan", snapshot_strip("t", &MapBase::default(), (0.0, 0.0, 0.0, 0.0), &[SnapshotPanel { glyphs: vec![glyph((nan, 1.0))], ..Default::default() }])),
        ("p06 zeros", stacked_share_bars("t", &[StackRow { label: "a".into(), parts: vec![("x".into(), 0.0), ("y".into(), nan), ("z".into(), -1.0)] }, StackRow { label: "b".into(), parts: vec![] }])),
        ("p07 single", grouped_bars("t", &GroupedBars { categories: vec!["하나".into()], series: vec![ser(vec![0.0])] }, "")),
        ("p07 nan", grouped_bars("t", &GroupedBars { categories: vec![], series: vec![ser(vec![nan, inf, -2.0]), Series::default()] }, "")),
        ("p07 eleven series", grouped_bars("t", &GroupedBars { categories: vec!["a".into()], series: (0..11).map(|i| ser(vec![i as f64])).collect() }, "")),
        ("p08 nan", hbar_chart("t", &[HBar { label: "a".into(), value: nan, secondary: Some(inf) }, HBar { label: "b".into(), value: -4.0, secondary: Some(1.7) }], "", None)),
        ("p09 single", box_plot("t", &[Dist { label: "a".into(), values: vec![3.0] }, Dist { label: "b".into(), values: vec![nan, inf] }], "")),
        ("p09 all nan", box_plot("t", &[Dist { label: "a".into(), values: vec![nan] }], "")),
        ("p10 single", scatter_with_curve("t", &[(1.0, 1.0), (nan, 2.0)], &[(0.0, 0.0), (nan, 1.0), (2.0, inf)], "", "", "")),
        ("p10 negative", scatter_with_curve("t", &[(-1.0, -5.0), (3.0, 2.0)], &[], "", "", "")),
        (
            "p12 duplicates",
            ontology_map(
                "t",
                &[GNode { id: "a".into(), label: "A".into(), kind: "evidence".into(), detail: Some(String::new()) }, GNode { id: "a".into(), label: "중복".into(), kind: "persona".into(), detail: None }],
                &[GEdge { from: "a".into(), to: "a".into(), label: "자기".into() }, GEdge { from: "a".into(), to: "z".into(), label: String::new() }],
            ),
        ),
        ("p13 odd", decision_chart("t", &[AltBar { label: "a".into(), prob: nan, chosen: true }, AltBar { label: "b".into(), prob: 1.7, chosen: false }, AltBar { label: "c".into(), prob: -0.2, chosen: false }], &[("x".into(), nan), ("y".into(), 0.0)])),
    ];
    for (name, d) in &docs {
        assert_svg_contract(name, d);
    }
    // 1점 상자도 범위를 넓혀 그린다.
    assert!(docs.iter().any(|(n, d)| *n == "p09 single" && d.contains("n=1") && d.contains("n=0")));
    // 15업종은 앞 10개 + '기타 5종'
    assert!(docs.iter().any(|(n, d)| *n == "p01 15 categories" && d.contains("기타 5종 (5)")));
}

#[test]
fn density_classes_on_thresholds() {
    let th = common::LOS_THRESHOLDS;
    let cases = [(0.0, 0), (0.30, 0), (0.31, 1), (0.42, 1), (0.43, 2), (0.72, 3), (1.08, 4), (2.16, 4), (2.17, 5), (9.0, 5)];
    for (v, k) in cases {
        assert_eq!(density_class(v, &th), k, "값 {v}");
    }
    // 그림에서도 같은 등급 색으로 묶인다.
    let edges: Vec<EdgeValue> = cases.iter().enumerate().map(|(i, (v, _))| EdgeValue { pts: vec![(i as f64, 0.0), (i as f64, 5.0)], value: *v }).collect();
    let base = MapBase { bbox: (0.0, 0.0, 10.0, 5.0), ..Default::default() };
    let out = density_map("등급", &base, &edges, &th, &common::LOS_LABELS);
    assert_svg_contract("p03", &out);
    for k in 0..6 {
        let expected = cases.iter().filter(|(_, c)| *c == k).count();
        let tag = format!("data-class=\"{k}\"");
        let drawn: usize = out.lines().filter(|l| l.contains(&tag)).map(|l| l.matches('M').count()).sum();
        assert_eq!(drawn, expected, "등급 {k}");
        if expected > 0 {
            let line = out.lines().find(|l| l.contains(&tag)).unwrap();
            assert!(line.contains(&format!("stroke=\"{}\"", ordinal_color(k, 6))));
        }
    }
}

#[test]
fn box_plot_quartiles_known_dataset() {
    let values: Vec<f64> = (1..=9).map(f64::from).chain([30.0]).collect();
    let st = box_stats(&values).unwrap();
    assert_eq!(st.n, 10);
    assert_eq!((st.q1, st.median, st.q3), (3.25, 5.5, 7.75));
    assert_eq!((st.lo_whisker, st.hi_whisker), (1.0, 9.0));
    assert_eq!(st.outliers, vec![30.0]);
    let out = box_plot("분위수", &[Dist { label: "가".into(), values }], "값");
    assert_svg_contract("p09", &out);
    assert!(out.contains("중앙값 5.5, Q1 3.25, Q3 7.75, 수염 1~9"));
    assert!(out.contains("n=10"));
    assert_eq!(out.matches("<circle ").count(), 1, "바깥 값 점 하나");
}

#[test]
fn stacked_share_normalization() {
    let rows = vec![
        StackRow { label: "가".into(), parts: vec![("앞길".into(), 2.0), ("발밑".into(), 2.0), ("장소".into(), 4.0)] },
        StackRow { label: "나".into(), parts: vec![("장소".into(), 10.0), ("앞길".into(), 30.0)] },
        StackRow { label: "다".into(), parts: vec![("앞길".into(), 0.0)] },
    ];
    let out = stacked_share_bars("정규화", &rows);
    assert_svg_contract("p06", &out);
    for pct in ["25.0%", "50.0%", "75.0%"] {
        assert!(out.contains(&format!(">{pct}</text>")), "{pct}");
    }
    assert!(out.contains("자료 없음"));
    // 부분 이름의 색은 행이 달라도 같다.
    let color_of = |row: &str, part: &str| -> String {
        let key = format!("<title>{row} · {part}:");
        let line = out.lines().find(|l| l.contains(&key)).unwrap();
        attr_of(line, "fill").unwrap()
    };
    assert_eq!(color_of("가", "앞길"), color_of("나", "앞길"));
    assert_eq!(color_of("가", "장소"), color_of("나", "장소"));
    assert_ne!(color_of("가", "앞길"), color_of("가", "장소"));
    // 폭: 가 행의 '장소'는 막대 전체의 절반
    let width_of = |row: &str, part: &str| -> f64 {
        let key = format!("<title>{row} · {part}:");
        let line = out.lines().find(|l| l.contains(&key)).unwrap();
        attr_of(line, "width").unwrap().parse().unwrap()
    };
    let total = width_of("가", "앞길") + width_of("가", "발밑") + width_of("가", "장소") + 4.0;
    assert!((width_of("가", "장소") + 1.0 - total / 2.0).abs() < 0.05);
}

#[test]
fn figure_sizes_are_reasonable() {
    let figs: Vec<Fig> = figures();
    let total: usize = figs.iter().map(|f| f.svg.len()).sum();
    for f in &figs {
        assert!(f.svg.len() < 1_500_000, "{}: {} 바이트", f.stem, f.svg.len());
    }
    assert!(total < 4_000_000, "합계 {total} 바이트");
}
