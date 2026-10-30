//! 시험·예시용 합성 자료와 그림 13장·보고서 생성.
//!
//! 02d I-228-b 합성 고정 자료의 공간을 닮은 3×3 블록(블록 60m, 골목 8m, 동쪽 간선도로 26m,
//! 보도 3m)에 장소 40곳·출입구 4곳·몸 60명을 결정적 의사 난수로 놓는다.
//! `tests/plots.rs`와 `examples/demo.rs`가 함께 쓴다(예시는 `#[path]`로 포함).

#![allow(dead_code)]

use mdsim_plot::*;
use std::f64::consts::PI;

/// 결정적 의사 난수(선형 합동 생성기). 시험 자료를 만들 때만 쓴다.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    /// [0, 1) 균등값.
    pub fn f(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }

    pub fn pick(&mut self, n: usize) -> usize {
        ((self.f() * n as f64) as usize).min(n.saturating_sub(1))
    }

    /// 표준 정규값(Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.f().max(1e-12);
        let u2 = self.f();
        (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
    }
}

/// 점 목록.
pub type Pts = Vec<(f64, f64)>;

pub const BLOCK: f64 = 60.0;
pub const ALLEY: f64 = 8.0;
pub const ROAD: f64 = 26.0;
/// 블록 구역의 한 변(골목 4줄 + 블록 3개).
pub const SPAN: f64 = ALLEY + 3.0 * (BLOCK + ALLEY);
/// 동쪽 간선도로를 더한 동서 폭.
pub const WIDTH: f64 = SPAN + ROAD;

pub const CATEGORIES: [&str; 9] = ["음식점", "카페", "화장품", "의류", "잡화", "편의점", "노점", "벤치", "행사"];
pub const AGE_BANDS: [&str; 7] = ["10대", "20대", "30대", "40대", "50대", "60대", "70대 이상"];
pub const LOS_THRESHOLDS: [f64; 5] = [0.31, 0.43, 0.72, 1.08, 2.17];
pub const LOS_LABELS: [&str; 6] = ["LOS A", "LOS B", "LOS C", "LOS D", "LOS E", "LOS F"];

/// i번째(0~3) 골목 중심선 좌표.
pub fn alley_center(i: usize) -> f64 {
    ALLEY / 2.0 + i as f64 * (BLOCK + ALLEY)
}

/// 보행 간선(교차점 사이 구간, 동쪽 보도, 횡단).
pub fn segments() -> Vec<Vec<(f64, f64)>> {
    let mut v = Vec::new();
    for i in 0..4 {
        for j in 0..3 {
            let (a, b, c) = (alley_center(i), alley_center(j), alley_center(j + 1));
            v.push(vec![(a, b), (a, c)]);
            v.push(vec![(b, a), (c, a)]);
        }
    }
    for (z0, z1) in [(0.0, 103.0), (103.0, SPAN)] {
        v.push(vec![(SPAN + 1.5, z0), (SPAN + 1.5, z1)]);
        v.push(vec![(SPAN + ROAD - 1.5, z0), (SPAN + ROAD - 1.5, z1)]);
    }
    v.push(vec![(alley_center(3), 103.0), (SPAN + 1.5, 103.0)]);
    v.push(vec![(SPAN + 1.5, 103.0), (SPAN + ROAD - 1.5, 103.0)]);
    v
}

pub fn base() -> MapBase {
    let cell = 0.5;
    let w = (WIDTH / cell) as usize;
    let h = (SPAN / cell) as usize;
    let in_alley = |v: f64| v.rem_euclid(BLOCK + ALLEY) < ALLEY;
    let mut data = vec![0u8; w * h];
    for r in 0..h {
        for c in 0..w {
            let x = (c as f64 + 0.5) * cell;
            let z = (r as f64 + 0.5) * cell;
            let v = if x >= SPAN {
                let rx = x - SPAN;
                if !(3.0..=ROAD - 3.0).contains(&rx) || (100.0..106.0).contains(&z) {
                    2
                } else {
                    0
                }
            } else if in_alley(x) || in_alley(z) {
                1
            } else {
                0
            };
            data[r * w + c] = v;
        }
    }
    let mut buildings = Vec::new();
    for bi in 0..3 {
        for bj in 0..3 {
            let x0 = ALLEY + bi as f64 * (BLOCK + ALLEY);
            let z0 = ALLEY + bj as f64 * (BLOCK + ALLEY);
            for (dx, dz) in [(0.0, 0.0), (31.0, 0.0), (0.0, 31.0), (31.0, 31.0)] {
                let (a, b) = (x0 + dx + 1.0, z0 + dz + 1.0);
                buildings.push(vec![(a, b), (a + 27.0, b), (a + 27.0, b + 27.0), (a, b + 27.0)]);
            }
        }
    }
    MapBase {
        bbox: (0.0, 0.0, WIDTH, SPAN),
        buildings,
        walk: Some(Raster { origin: (0.0, 0.0), cell, w, h, data }),
        edges: segments(),
    }
}

/// 장소 40곳: 벤치 4, 노점 6, 행사 1, 나머지 29곳은 여섯 업종에 고르게.
pub fn places() -> Vec<PlacePoint> {
    let mut cats: Vec<&str> = Vec::new();
    for i in 0..29 {
        cats.push(CATEGORIES[i % 6]);
    }
    cats.extend(["노점"; 6]);
    cats.extend(["벤치"; 4]);
    cats.push("행사");
    let mut rng = Lcg::new(7);
    cats.iter()
        .enumerate()
        .map(|(i, cat)| {
            let pos = if *cat == "행사" {
                (alley_center(1) + 2.0, alley_center(2) + 2.0)
            } else {
                let line = alley_center(rng.pick(4));
                let along = rng.range(alley_center(0) + 6.0, alley_center(3) - 6.0);
                let side = if matches!(*cat, "노점" | "벤치") { rng.range(-1.0, 1.0) } else if rng.f() < 0.5 { -3.4 } else { 3.4 };
                if rng.f() < 0.5 {
                    (line + side, along)
                } else {
                    (along, line + side)
                }
            };
            let label = match *cat {
                "행사" => Some("거리 공연".to_string()),
                _ if i % 8 == 0 => Some(format!("{cat} {:02}", i + 1)),
                _ => None,
            };
            PlacePoint { pos, category: (*cat).to_string(), label }
        })
        .collect()
}

pub fn portals() -> Vec<PortalPoint> {
    let p = |x: f64, z: f64, kind: &str, name: &str| PortalPoint { pos: (x, z), kind: kind.into(), name: name.into() };
    vec![
        p(SPAN + ROAD - 1.5, 20.0, "지하철", "명동역 6번 출구"),
        p(alley_center(1), 2.0, "지하철", "을지로입구역 5번 출구"),
        p(2.0, alley_center(2), "경계", "서쪽 경계"),
        p(alley_center(2), SPAN - 2.0, "숙소", "호텔 앞"),
    ]
}

/// 10분 칸 이름과 도착 수·동시 체류 인원(14:00~16:50).
pub fn concurrency() -> (Vec<String>, Series, Series) {
    let mut rng = Lcg::new(3);
    let labels: Vec<String> = (0..18).map(|i| format!("{}:{:02}", 14 + i / 6, (i % 6) * 10)).collect();
    let arrivals: Vec<f64> =
        (0..18).map(|i| (140.0 + 90.0 * (PI * i as f64 / 17.0).sin() + 30.0 * (rng.f() - 0.5)).round()).collect();
    let mut conc = Vec::with_capacity(18);
    let mut c = 300.0;
    for a in &arrivals {
        c = (c * 0.8 + a * 1.5_f64).round();
        conc.push(c);
    }
    (labels, Series { name: "10분 도착".into(), values: arrivals }, Series { name: "동시 체류".into(), values: conc })
}

/// 가장 붐비는 교차점.
pub fn hotspot() -> (f64, f64) {
    (alley_center(1), alley_center(2))
}

pub fn edge_density() -> Vec<EdgeValue> {
    let (hx, hz) = hotspot();
    let mut rng = Lcg::new(5);
    segments()
        .into_iter()
        .map(|pts| {
            let (mx, mz) = ((pts[0].0 + pts[1].0) / 2.0, (pts[0].1 + pts[1].1) / 2.0);
            let d2 = (mx - hx).powi(2) + (mz - hz).powi(2);
            let value = 2.6 * (-d2 / (2.0 * 55.0f64.powi(2))).exp() + 0.15 * rng.f();
            EdgeValue { pts, value }
        })
        .collect()
}

fn push_leg(pts: &mut Vec<(f64, f64)>, a: (f64, f64), b: (f64, f64), off: f64, rng: &mut Lcg) {
    let d = (b.0 - a.0).hypot(b.1 - a.1);
    if d < 1e-9 {
        return;
    }
    let n = (d / 0.65).ceil().max(1.0) as usize;
    let (ux, uz) = ((b.0 - a.0) / d, (b.1 - a.1) / d);
    for s in 1..=n {
        let t = s as f64 / n as f64;
        let o = off + (rng.f() - 0.5) * 0.3;
        pts.push((a.0 + (b.0 - a.0) * t - uz * o, a.1 + (b.1 - a.1) * t + ux * o));
    }
}

/// 몸 60명의 궤적(출입구에서 출발해 골목 교차점을 6~11번 옮겨 다님, 0.65m 간격 점).
pub fn tracks() -> Vec<Track> {
    let mut rng = Lcg::new(11);
    let ports = portals();
    (0..60)
        .map(|k| {
            let start = ports[k % ports.len()].pos;
            let grid = |v: f64| (((v - ALLEY / 2.0) / (BLOCK + ALLEY)).round().clamp(0.0, 3.0)) as usize;
            let (mut ix, mut iz) = (grid(start.0), grid(start.1));
            let off = rng.range(-2.5, 2.5);
            let mut pts = vec![start];
            let mut cur = (alley_center(ix), alley_center(iz));
            push_leg(&mut pts, start, cur, off, &mut rng);
            for _ in 0..(6 + rng.pick(6)) {
                let (nx, nz) = match rng.pick(4) {
                    0 if ix < 3 => (ix + 1, iz),
                    1 if ix > 0 => (ix - 1, iz),
                    2 if iz < 3 => (ix, iz + 1),
                    3 if iz > 0 => (ix, iz - 1),
                    _ => continue,
                };
                let next = (alley_center(nx), alley_center(nz));
                push_leg(&mut pts, cur, next, off, &mut rng);
                (ix, iz, cur) = (nx, nz, next);
            }
            Track { pts, group: AGE_BANDS[rng.pick(AGE_BANDS.len())].to_string() }
        })
        .collect()
}

pub fn snapshot_clip() -> (f64, f64, f64, f64) {
    let (x, z) = hotspot();
    (x - 40.0, z - 40.0, x + 40.0, z + 40.0)
}

/// 가장 붐빈 칸 둘레 40m의 연속 렌더 프레임 4장(0.5초 간격).
pub fn snapshots() -> Vec<SnapshotPanel> {
    let (hx, hz) = hotspot();
    let near: Vec<PlacePoint> =
        places().into_iter().filter(|p| (p.pos.0 - hx).abs() < 40.0 && (p.pos.1 - hz).abs() < 40.0).collect();
    let mut rng = Lcg::new(23);
    let bodies: Vec<(f64, f64, f64, &str, bool)> = (0..24)
        .map(|i| {
            let along = rng.range(-36.0, 36.0);
            let off = rng.range(-3.0, 3.0);
            let back = rng.f() < 0.5;
            let band = AGE_BANDS[rng.pick(AGE_BANDS.len())];
            let browsing = rng.f() < 0.35;
            if i % 2 == 0 {
                (hx + along, hz + off, if back { PI } else { 0.0 }, band, browsing)
            } else {
                (hx + off, hz + along, if back { -PI / 2.0 } else { PI / 2.0 }, band, browsing)
            }
        })
        .collect();
    (0..4)
        .map(|f| {
            let t = f as f64 * 0.5;
            let mut noticed = Vec::new();
            let glyphs = bodies
                .iter()
                .enumerate()
                .map(|(i, &(x, z, heading, band, browsing))| {
                    let speed = if browsing { 0.7 } else { 1.3 };
                    let pos = (x + heading.cos() * speed * t, z + heading.sin() * speed * t);
                    let target = if browsing && !near.is_empty() { Some(near[i % near.len()].pos) } else { None };
                    let gaze = match target {
                        Some(p) => (p.1 - pos.1).atan2(p.0 - pos.0),
                        None => heading + 0.2 * ((i + f) as f64).sin(),
                    };
                    if let Some(p) = target {
                        if (i + f) % 3 == 0 && !noticed.contains(&p) {
                            noticed.push(p);
                        }
                    }
                    Glyph {
                        pos,
                        heading,
                        gaze,
                        focal_range: if browsing { 8.0 } else { 12.0 },
                        focal_angle_deg: if browsing { 50.0 } else { 70.0 },
                        group: band.to_string(),
                        looking_at: target,
                        activity: if browsing { "둘러보기".into() } else { "걷기".into() },
                    }
                })
                .collect();
            SnapshotPanel { caption: format!("14:32:{:04.1} · 프레임 {}", t, f + 1), glyphs, noticed, places: near.clone() }
        })
        .collect()
}

pub fn gaze_share() -> Vec<StackRow> {
    AGE_BANDS
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let i = i as f64;
            StackRow {
                label: (*b).to_string(),
                parts: vec![
                    ("앞길".into(), (0.55 - 0.03 * i) * 3600.0),
                    ("발밑".into(), (0.12 + 0.05 * i) * 3600.0),
                    ("장소".into(), (0.33 - 0.02 * i) * 3600.0),
                ],
            }
        })
        .collect()
}

pub fn category_bars() -> GroupedBars {
    let cats = ["음식점", "카페", "화장품", "의류", "잡화", "편의점", "노점"];
    let visits = [420.0, 380.0, 350.0, 210.0, 120.0, 160.0, 260.0];
    let buys: Vec<f64> = visits.iter().enumerate().map(|(i, v)| (v * (0.45 + 0.04 * i as f64)).round()).collect();
    GroupedBars {
        categories: cats.iter().map(|s| (*s).to_string()).collect(),
        series: vec![Series { name: "방문".into(), values: visits.to_vec() }, Series { name: "구매".into(), values: buys }],
    }
}

pub fn notice_rows() -> Vec<HBar> {
    let mut rng = Lcg::new(31);
    (0..20)
        .map(|i| {
            let value = (520.0 * (-(i as f64) / 9.0).exp() + 20.0 * rng.f()).round();
            HBar { label: format!("{} {:02}", CATEGORIES[i % 6], i + 1), value, secondary: Some(0.05 + 0.3 * rng.f()) }
        })
        .collect()
}

pub fn dwell() -> Vec<Dist> {
    let mut rng = Lcg::new(41);
    AGE_BANDS
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let n = 30 + 10 * i;
            let median = 42.0 - 3.0 * i as f64;
            let values = (0..n).map(|_| (median * (0.45 * rng.normal()).exp()).max(1.0)).collect();
            Dist { label: (*b).to_string(), values }
        })
        .collect()
}

/// Weidmann(1993) 기준식 v(ρ) = 1.34·(1 − exp(−1.913·(1/ρ − 1/5.4))).
pub fn weidmann(rho: f64) -> f64 {
    1.34 * (1.0 - (-1.913 * (1.0 / rho - 1.0 / 5.4)).exp())
}

pub fn speed_density() -> (Pts, Pts) {
    let mut rng = Lcg::new(53);
    let pts = (0..300)
        .map(|_| {
            let rho = rng.range(0.05, 2.4);
            (rho, weidmann(rho) * (1.0 + 0.24 * (rng.f() - 0.5)))
        })
        .collect();
    let curve = (1..=54).map(|k| k as f64 * 0.1).map(|rho| (rho, weidmann(rho).max(0.0))).collect();
    (pts, curve)
}

pub fn visit_choice() -> GroupedBars {
    GroupedBars {
        categories: vec!["소득 하".into(), "소득 중".into(), "소득 상".into(), "속성 없음".into()],
        series: vec![
            Series { name: "명동".into(), values: vec![0.42, 0.51, 0.58, 0.49] },
            Series { name: "경쟁 상권".into(), values: vec![0.31, 0.29, 0.27, 0.30] },
            Series { name: "안 감".into(), values: vec![0.27, 0.20, 0.15, 0.21] },
        ],
    }
}

pub fn ontology() -> (Vec<GNode>, Vec<GEdge>) {
    let n = |id: &str, label: &str, kind: &str, detail: Option<&str>| GNode {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        detail: detail.map(str::to_string),
    };
    let nodes = vec![
        n("ev1", "원문: 무릎이 안 좋음", "evidence", Some("건강 서술")),
        n("ev2", "원문: 구경을 좋아함", "evidence", Some("취향 서술")),
        n("ev3", "관광 통계", "evidence", Some("연령·목적 분포")),
        n("p", "페르소나 #0137", "persona", Some("60대 · 여행 목적")),
        n("cap1", "오르막 능력 낮음", "capability", Some("D-CLIMB 0.42")),
        n("cap2", "보행 지속 보통", "capability", Some("D-FATIGUE 0.61")),
        n("sen1", "혼잡 민감 높음", "sensitivity", Some("D-CROWD 0.78")),
        n("sen2", "소음 민감", "sensitivity", Some("D-SENSE 0.55")),
        n("int1", "길거리 음식 관심", "interest", Some("A-FOOD 0.83")),
        n("int2", "쇼핑 관심", "interest", Some("A-SHOP 0.47")),
        n("per1", "주목 성향 높음", "perception", Some("알아차림 ×1.3")),
        n("pro1", "무리 감수 낮음", "propensity", Some("τ 0.8")),
        n("D-CLIMB", "D-CLIMB 경사", "dimension", None),
        n("D-FATIGUE", "D-FATIGUE 피로", "dimension", None),
        n("D-CROWD", "D-CROWD 혼잡", "dimension", None),
        n("D-SENSE", "D-SENSE 감각 부하", "dimension", None),
        n("A-FOOD", "A-FOOD 음식", "interest_dim", None),
        n("A-SHOP", "A-SHOP 쇼핑", "interest_dim", None),
    ];
    let e = |a: &str, b: &str, l: &str| GEdge { from: a.into(), to: b.into(), label: l.into() };
    let edges = vec![
        e("ev1", "p", "근거"),
        e("ev2", "p", "근거"),
        e("ev3", "p", "기본값"),
        e("p", "cap1", "파생"),
        e("p", "cap2", "파생"),
        e("p", "sen1", "파생"),
        e("p", "sen2", "파생"),
        e("p", "int1", "파생"),
        e("p", "int2", "파생"),
        e("p", "per1", "파생"),
        e("p", "pro1", "파생"),
        e("cap1", "D-CLIMB", "능력"),
        e("cap2", "D-FATIGUE", "능력"),
        e("sen1", "D-CROWD", "민감도"),
        e("sen2", "D-SENSE", "민감도"),
        e("int1", "A-FOOD", "관심"),
        e("int2", "A-SHOP", "관심"),
        e("per1", "A-SHOP", "알아차림"),
        e("pro1", "D-FATIGUE", "무리 감수"),
    ];
    (nodes, edges)
}

pub fn decision() -> (Vec<AltBar>, Vec<(String, f64)>) {
    let a = |label: &str, prob: f64, chosen: bool| AltBar { label: label.into(), prob, chosen };
    let alts = vec![
        a("카페 08 방문", 0.41, true),
        a("노점 31 들르기", 0.27, false),
        a("계속 걷기", 0.22, false),
        a("명동 떠나기", 0.10, false),
    ];
    let terms = vec![
        ("A-FOOD 끌림".to_string(), 0.82),
        ("D-CROWD 혼잡 비용".to_string(), -0.47),
        ("D-TIME 시간 비용".to_string(), -0.21),
        ("D-MONEY 가격".to_string(), -0.15),
        ("동반자 선호".to_string(), 0.12),
    ];
    (alts, terms)
}

/// 그림 한 장(파일 이름 줄기, 보고서 절 제목, 설명, SVG).
pub struct Fig {
    pub stem: &'static str,
    pub title: String,
    pub caption: String,
    pub svg: String,
}

fn fig(stem: &'static str, title: &str, caption: &str, svg: String) -> Fig {
    Fig { stem, title: title.into(), caption: caption.into(), svg }
}

/// 그림 13장(p01~p13, 02d I-228-a의 파일 이름 순서).
pub fn figures() -> Vec<Fig> {
    let b = base();
    let (labels, arrivals, conc) = concurrency();
    let (pts, curve) = speed_density();
    let (nodes, edges) = ontology();
    let (alts, terms) = decision();
    vec![
        fig(
            "p01_space",
            "p01 공간",
            "보행 격자·건물·장소(업종 색)·출입구(종류별 모양).",
            space_map("p01 공간: 보행 격자·건물·장소·출입구", &b, &places(), &portals()),
        ),
        fig(
            "p02_concurrency",
            "p02 도착과 동시 체류",
            "10분 도착 막대(왼쪽 축)와 동시 체류 선(오른쪽 축).",
            bar_line_chart("p02 10분 도착과 동시 체류", &labels, &arrivals, &conc, "도착 (명/10분)", "동시 체류 (명)"),
        ),
        fig(
            "p03_density_peak",
            "p03 최대 프레임 밀도",
            "동시 인원 최대 10분 프레임의 간선 밀도(Fruin LOS, 인/㎡).",
            density_map("p03 간선 밀도 (최대 프레임)", &b, &edge_density(), &LOS_THRESHOLDS, &LOS_LABELS),
        ),
        fig(
            "p04_trajectories",
            "p04 궤적",
            "몸 60명의 궤적(연령대 색).",
            trajectories("p04 몸 60명의 궤적", &b, &tracks()),
        ),
        fig(
            "p05_snapshots",
            "p05 스냅샷",
            "가장 붐빈 칸 둘레 40m의 연속 렌더 프레임 4장.",
            snapshot_strip("p05 가장 붐빈 칸 둘레 스냅샷", &b, snapshot_clip(), &snapshots()),
        ),
        fig(
            "p06_gaze_share",
            "p06 시선 비율",
            "연령대별 앞길·발밑·장소 보기 시간 비율.",
            stacked_share_bars("p06 연령대별 시선 시간 비율", &gaze_share()),
        ),
        fig(
            "p07_category",
            "p07 업종",
            "업종별 방문·구매 수.",
            grouped_bars("p07 업종별 방문·구매", &category_bars(), "건수"),
        ),
        fig(
            "p08_notice",
            "p08 알아차림",
            "장소별 알아차림 수 상위 20과 알아차린 뒤 방문 비율.",
            hbar_chart("p08 장소별 알아차림 상위 20", &notice_rows(), "알아차림 수", Some("알아차린 뒤 방문")),
        ),
        fig("p09_dwell", "p09 체류 시간", "연령대별 체류 시간 분포.", box_plot("p09 연령대별 체류 시간", &dwell(), "체류 시간 (분)")),
        fig(
            "p10_speed_density",
            "p10 속도-밀도",
            "10분 간선별 평균 속도·밀도 산점과 Weidmann 기준선.",
            scatter_with_curve("p10 간선 속도와 밀도", &pts, &curve, "밀도 (인/㎡)", "속도 (m/s)", "Weidmann (1993)"),
        ),
        fig(
            "p11_visit_choice",
            "p11 방문 선택",
            "명동·경쟁 상권·안 감 확률 합(방문객 소득 구간별).",
            grouped_bars("p11 방문 선택 확률 합", &visit_choice(), "확률 합 (1명당)"),
        ),
        fig(
            "p12_ontology_map",
            "p12 온톨로지 지도",
            "페르소나 1명의 근거 → 특성 → 차원.",
            ontology_map("p12 페르소나 #0137 온톨로지 지도", &nodes, &edges),
        ),
        fig(
            "p13_decision",
            "p13 결정",
            "결정 하나의 대안별 확률과 기여 항.",
            decision_chart("p13 결정 하나: 다음 행선지", &alts, &terms),
        ),
    ]
}

pub fn report_summary() -> Vec<(String, String)> {
    [
        ("실행", "기본 시나리오 · 주말 2026-11-07(토) 14:00~17:00"),
        ("시드", "20261107"),
        ("입력 번들 해시", "sha256:0f3a…(합성 예시)"),
        ("정의 해시", "sha256:7c21…(합성 예시)"),
        ("몸 수", "1,284"),
        ("결정 수", "38,512"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect()
}

pub fn report_labels() -> Vec<String> {
    vec!["합성 고정 자료".into(), "가정 기반".into(), "상점 자료 대기".into()]
}

pub fn report(figs: &[Fig]) -> String {
    let sections: Vec<ReportSection> = figs
        .iter()
        .map(|f| ReportSection { title: f.title.clone(), caption: f.caption.clone(), svg: f.svg.clone() })
        .collect();
    report_html("명동 보행 시뮬레이션 결과 (합성 고정 자료)", &report_summary(), &report_labels(), &sections)
}
