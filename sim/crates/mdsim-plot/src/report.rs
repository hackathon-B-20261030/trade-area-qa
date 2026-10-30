//! 단일 HTML5 보고서(02d I-228-a `report.html`).
//!
//! 제목, 요약 표(키·값), 가정 라벨 배지, 그림 목차, 그림마다 제목·인라인 SVG·설명을 담는다.
//! 스크립트와 외부 링크·자원은 넣지 않는다(E228-5). 같은 입력은 바이트가 같은 출력을 낸다.

use crate::svg::esc;
use std::fmt::Write as _;

/// 보고서의 그림 한 절. `svg`는 이 크레이트의 그림 함수가 만든 SVG 문자열을 그대로 넣는다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportSection {
    pub title: String,
    pub caption: String,
    pub svg: String,
}

const CSS: &str = "\
:root{color-scheme:light}
*{box-sizing:border-box}
body{margin:0;background:#f9f9f7;color:#0b0b0b;font-family:-apple-system,BlinkMacSystemFont,\"Apple SD Gothic Neo\",\"Noto Sans KR\",\"Malgun Gothic\",\"Segoe UI\",sans-serif;line-height:1.55}
main{max-width:1000px;margin:0 auto;padding:32px 16px 64px}
h1{font-size:26px;line-height:1.3;margin:0 0 12px}
h2{font-size:19px;line-height:1.35;margin:0 0 10px}
.labels{display:flex;flex-wrap:wrap;gap:6px;list-style:none;padding:0;margin:0 0 20px}
.label{display:inline-block;padding:2px 10px;border-radius:999px;background:#fff4d6;border:1px solid #e3c477;color:#5a4300;font-size:13px}
table.summary{border-collapse:collapse;width:100%;margin:0 0 24px;background:#fcfcfb;font-size:14px}
table.summary th,table.summary td{border:1px solid #e1e0d9;padding:6px 10px;text-align:left;vertical-align:top}
table.summary th{width:32%;background:#f3f2ee;font-weight:600}
table.summary td{overflow-wrap:anywhere;font-variant-numeric:tabular-nums}
nav.toc{margin:0 0 28px;font-size:14px}
nav.toc ol{margin:6px 0 0;padding-left:22px}
nav.toc a{color:#1c5cab}
section.fig{background:#fcfcfb;border:1px solid #e1e0d9;border-radius:8px;padding:16px;margin:0 0 24px}
figure{margin:0}
figure>svg{display:block;max-width:100%;height:auto;margin:0 auto}
figcaption{margin-top:10px;color:#52514e;font-size:14px}
.empty{color:#52514e}
";

/// 보고서 HTML을 만든다.
///
/// - `summary`: 실행 설정·입력 해시·몸 수 같은 키·값 표
/// - `labels`: 가정 라벨 배지(예: "합성 고정 자료", "상점 자료 대기")
/// - `sections`: 그림 절(제목, 인라인 SVG, 설명). 제목·설명·키·값·라벨은 이스케이프한다.
pub fn report_html(title: &str, summary: &[(String, String)], labels: &[String], sections: &[ReportSection]) -> String {
    let svg_len: usize = sections.iter().map(|s| s.svg.len()).sum();
    let mut h = String::with_capacity(svg_len + 8192);
    let t = esc(title);
    h.push_str("<!doctype html>\n<html lang=\"ko\">\n<head>\n<meta charset=\"utf-8\" />\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n");
    let _ = writeln!(h, "<title>{t}</title>");
    let _ = writeln!(h, "<style>\n{CSS}</style>");
    h.push_str("</head>\n<body>\n<main>\n");
    let _ = writeln!(h, "<h1>{t}</h1>");
    if !labels.is_empty() {
        h.push_str("<ul class=\"labels\" aria-label=\"가정 라벨\">\n");
        for l in labels {
            let _ = writeln!(h, "<li class=\"label\">{}</li>", esc(l));
        }
        h.push_str("</ul>\n");
    }
    if !summary.is_empty() {
        h.push_str("<table class=\"summary\">\n<tbody>\n");
        for (k, v) in summary {
            let _ = writeln!(h, "<tr><th scope=\"row\">{}</th><td>{}</td></tr>", esc(k), esc(v));
        }
        h.push_str("</tbody>\n</table>\n");
    }
    if sections.is_empty() {
        h.push_str("<p class=\"empty\">그림 자료 없음</p>\n");
    } else {
        h.push_str("<nav class=\"toc\" aria-label=\"그림 목차\"><strong>그림</strong>\n<ol>\n");
        for (i, s) in sections.iter().enumerate() {
            let _ = writeln!(h, "<li><a href=\"#fig-{:02}\">{}</a></li>", i + 1, esc(&s.title));
        }
        h.push_str("</ol>\n</nav>\n");
        for (i, s) in sections.iter().enumerate() {
            let _ = writeln!(h, "<section class=\"fig\" id=\"fig-{:02}\">", i + 1);
            let _ = writeln!(h, "<h2>{}</h2>", esc(&s.title));
            h.push_str("<figure>\n");
            h.push_str(s.svg.trim());
            h.push('\n');
            if !s.caption.is_empty() {
                let _ = writeln!(h, "<figcaption>{}</figcaption>", esc(&s.caption));
            }
            h.push_str("</figure>\n</section>\n");
        }
    }
    h.push_str("</main>\n</body>\n</html>\n");
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::check_well_formed;

    fn sample() -> String {
        let svg = crate::charts::grouped_bars("막대", &crate::GroupedBars::default(), "값");
        report_html(
            "실행 <보고서>",
            &[("시드".into(), "42".into()), ("입력 해시".into(), "ab&cd".into())],
            &["합성 고정 자료".into(), "상점 자료 대기".into()],
            &[ReportSection { title: "p07 업종".into(), caption: "설명 \"따옴표\"".into(), svg }],
        )
    }

    #[test]
    fn report_structure() {
        let h = sample();
        assert!(h.starts_with("<!doctype html>\n<html lang=\"ko\">"));
        assert!(h.contains("<meta charset=\"utf-8\" />"));
        assert!(h.contains("<meta name=\"viewport\""));
        assert!(h.contains("<title>실행 &lt;보고서&gt;</title>"));
        assert!(h.contains("<h1>실행 &lt;보고서&gt;</h1>"));
        assert!(h.contains("<li class=\"label\">합성 고정 자료</li>"));
        assert!(h.contains("<li class=\"label\">상점 자료 대기</li>"));
        assert!(h.contains("<th scope=\"row\">입력 해시</th><td>ab&amp;cd</td>"));
        assert!(h.contains("<section class=\"fig\" id=\"fig-01\">"));
        assert!(h.contains("<a href=\"#fig-01\">p07 업종</a>"));
        assert!(h.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(h.contains("<figcaption>설명 &quot;따옴표&quot;</figcaption>"));
        assert!(!h.contains("<script"));
        check_well_formed(&h).unwrap();
    }

    #[test]
    fn report_without_sections() {
        let h = report_html("빈 보고서", &[], &[], &[]);
        assert!(h.contains("그림 자료 없음"));
        check_well_formed(&h).unwrap();
    }

    #[test]
    fn report_is_deterministic() {
        assert_eq!(sample(), sample());
    }
}
