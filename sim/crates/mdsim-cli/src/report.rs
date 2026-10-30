//! 실행 결과 → 그림 13장과 report.html (02d I-228-a, C-228).
//!
//! 런타임의 요약(SummaryAcc)·몸 상태·사건·결정·번들을 mdsim-plot의 단순 자료로 옮겨 그린다.
//! 같은 입력·시드는 바이트가 같은 파일을 낸다(실행 시각·절대 경로를 넣지 않는다).

use mdsim_core::bundle::Profile;
use mdsim_core::dims::{Dim, Interest};
use mdsim_core::scenario::fmt_hhmm;
use mdsim_core::{Result, SimError};
use mdsim_plot as plot;
use mdsim_plot::{AltBar, Dist, EdgeValue, GEdge, GNode, Glyph, GroupedBars, HBar, MapBase, PlacePoint, PortalPoint, Raster, ReportSection, Series, SnapshotPanel, StackRow, Track};
use mdsim_runtime::record::{restore_state, FileSink, RowsPart};

/// 그림에 쓰는 기록: 사건 전부와 대표 결정 하나(대안 3개 이상인 첫 NEXT, 없으면 첫 결정).
#[derive(Default)]
pub struct PlotRows {
    rows: RowsPart,
    picked: bool,
}

impl PlotRows {
    pub fn absorb(&mut self, part: RowsPart) {
        self.rows.events.extend(part.events);
        if self.picked {
            return;
        }
        let pick = part.decisions.iter().find(|d| d.context == "NEXT" && d.n_alts >= 3).or(if self.rows.decisions.is_empty() { part.decisions.first() } else { None });
        if let Some(d) = pick {
            self.picked = d.context == "NEXT" && d.n_alts >= 3;
            self.rows.decisions = vec![d.clone()];
            self.rows.alts = part.alts.iter().filter(|a| a.decision == d.id).cloned().collect();
        }
    }

    pub fn into_rows(self) -> RowsPart {
        self.rows
    }
}
use mdsim_runtime::state::Body;
use mdsim_runtime::step::{SimState, World};
use mdsim_runtime::visit::{visit_decisions_streamed, VisitDecision, VisitKind};
use std::collections::BTreeMap;
use std::path::Path;

/// 그림 파일 이름과 제목.
pub const FIGURES: [(&str, &str); 13] = [
    ("p01_space", "보행 공간·장소·출입구"),
    ("p02_concurrency", "10분 도착과 동시 체류"),
    ("p03_density_peak", "가장 붐빈 10분의 간선 밀도"),
    ("p04_trajectories", "몸 궤적"),
    ("p05_snapshots", "가장 붐빈 곳의 연속 렌더 프레임"),
    ("p06_gaze_share", "집단별 시선 시간 비율"),
    ("p07_category", "업종별 방문·구매·매출"),
    ("p08_notice", "장소별 알아차림과 방문"),
    ("p09_dwell", "집단별 체류 시간"),
    ("p10_speed_density", "간선 밀도와 속도"),
    ("p11_visit_choice", "방문지 선택 확률 합"),
    ("p12_ontology_map", "페르소나 온톨로지 지도"),
    ("p13_decision", "결정 하나의 대안과 기여 항"),
];

fn category_ko(c: &str) -> String {
    match c {
        "restaurant" => "음식점",
        "cafe" => "카페",
        "fast_food" => "패스트푸드",
        "bar" => "주점",
        "bakery" => "제과",
        "convenience" => "편의점",
        "clothes" => "의류",
        "department_store" => "백화점",
        "cosmetics" => "화장품",
        "beauty_service" => "미용",
        "souvenir" => "기념품",
        "electronics" => "전자",
        "culture_venue" => "문화 시설",
        "attraction" => "명소",
        "viewpoint" => "전망",
        "worship" => "종교",
        "park" => "공원",
        "lodging" => "숙박",
        "bench" => "벤치",
        "stall" => "노점",
        "event" => "행사",
        other => other,
    }
    .to_string()
}

fn interest_ko(i: Interest) -> &'static str {
    match i {
        Interest::Food => "음식",
        Interest::Beauty => "뷰티",
        Interest::Fashion => "패션",
        Interest::Souvenir => "기념품",
        Interest::Culture => "문화",
        Interest::Faith => "신앙",
        Interest::Nature => "자연",
    }
}

fn dim_ko(d: Dim) -> &'static str {
    match d {
        Dim::Climb => "오르막",
        Dim::Fall => "낙상",
        Dim::Step => "단차",
        Dim::Walk => "보행 거리",
        Dim::Time => "시간",
        Dim::Money => "금전",
        Dim::Way => "길찾기",
        Dim::Sense => "감각 자극",
        Dim::Crowd => "혼잡",
        Dim::Safe => "차량 위험",
    }
}

/// 기록의 항 이름(purpose, interest:A-…, cost:D-…, time, money) → 한국어 표시.
fn term_ko(t: &str) -> String {
    if let Some(a) = t.strip_prefix("interest:") {
        return format!("관심 {}", Interest::from_id(a).map(interest_ko).unwrap_or(a));
    }
    if let Some(d) = t.strip_prefix("cost:") {
        return format!("부담 {}", Dim::from_id(d).map(dim_ko).unwrap_or(d));
    }
    match t {
        "purpose" => "목적".into(),
        "time" => "시간".into(),
        "money" => "돈".into(),
        o => o.into(),
    }
}

fn action_ko(a: &str) -> &str {
    match a {
        "GO_TO" => "가기",
        "WATCH" => "구경",
        "EXPLORE" => "둘러보기",
        "LEAVE" => "떠나기",
        "PROCEED" => "계속 가기",
        "PACE" => "쉬엄쉬엄",
        "ABANDON" => "포기",
        "REST_SEAT" => "앉아 쉬기",
        "REST_PAID" => "카페에서 쉬기",
        "CONTINUE" => "계속",
        "BUY" => "구매",
        "NO_BUY" => "구경만",
        "KEEP" => "유지",
        "PRICE_UP" => "가격 올림",
        "PRICE_DOWN" => "가격 내림",
        "HOURS_UP" => "영업 늘림",
        "HOURS_DOWN" => "영업 줄임",
        "STAFF_UP" => "인력 늘림",
        "STAFF_DOWN" => "인력 줄임",
        "SWITCH_CATEGORY" => "업종 전환",
        "CLOSE" => "폐업",
        "DEFER" => "미루기",
        "DETOUR" => "우회",
        other => other,
    }
}

/// 몸의 집단 이름: 속성(연령대)이 있으면 연령대, 아니면 내국인·관광객, 배경 보행자는 "배경".
fn group_of(w: &World, b: &Body, bands: &BTreeMap<&str, String>) -> String {
    match b.profile {
        Some(i) => {
            let pr = &w.bundle.profiles[i as usize];
            bands.get(pr.persona_id.as_str()).cloned().unwrap_or_else(|| match pr.group {
                mdsim_core::bundle::Group::Domestic => "내국인".into(),
                mdsim_core::bundle::Group::Tourist => "관광객".into(),
            })
        }
        None => "배경".into(),
    }
}

fn map_base(w: &World) -> MapBase {
    let s = w.space;
    let (o, cell, rw, rh, data) = s.raster_view(2);
    MapBase {
        bbox: (s.bbox.min.x, s.bbox.min.z, s.bbox.max.x, s.bbox.max.z),
        buildings: s.buildings.iter().map(|b| b.iter().map(|p| (p.x, p.z)).collect()).collect(),
        walk: Some(Raster { origin: (o.x, o.z), cell, w: rw, h: rh, data }),
        edges: s.edges.iter().map(|e| e.poly.iter().map(|p| (p.x, p.z)).collect()).collect(),
    }
}

fn places(w: &World, label_top: usize) -> Vec<PlacePoint> {
    let mut v: Vec<PlacePoint> = w.space.places.iter().map(|p| PlacePoint { pos: (p.anchor.x, p.anchor.z), category: category_ko(&p.category), label: None }).collect();
    // 시계성 상위 몇 곳만 이름을 쓴다
    let mut ix: Vec<usize> = (0..w.space.places.len()).filter(|&i| w.space.places[i].go_to).collect();
    ix.sort_by(|&a, &b| w.space.places[b].visibility.total_cmp(&w.space.places[a].visibility).then(a.cmp(&b)));
    for &i in ix.iter().take(label_top) {
        v[i].label = Some(w.space.places[i].name.clone());
    }
    v
}

/// p12: 근거가 있는 첫 인물(없으면 첫 인물)의 근거 → 특성 → 차원 지도.
fn ontology_map(w: &World) -> String {
    let profs = &w.bundle.profiles;
    let Some(pr): Option<&Profile> = profs.iter().find(|p| !p.evidence.is_empty()).or(profs.first()) else {
        return plot::ontology_map("페르소나 온톨로지 지도", &[], &[]);
    };
    let mut nodes = vec![GNode { id: "persona".into(), label: pr.persona_id.clone(), kind: "persona".into(), detail: Some(format!("집단 {:?}", pr.group)) }];
    let mut edges = Vec::new();
    for (k, ev) in pr.evidence.iter().enumerate() {
        let id = format!("ev{k}");
        nodes.push(GNode { id: id.clone(), label: format!("{} [{}..{}]", ev.field, ev.start, ev.end), kind: "evidence".into(), detail: Some(format!("{} · {}", ev.rule_id, ev.label)) });
        // 근거 대상(cap_limit.climb·interest.food 등)의 차원으로 잇는다
        let tail = ev.target.rsplit('.').next().unwrap_or("");
        let to = Dim::ALL.iter().find(|d| d.key() == tail).map(|d| format!("cap:{}", d.id())).or_else(|| Interest::ALL.iter().find(|i| i.key() == tail).map(|i| format!("int:{}", i.id())));
        if let Some(to) = to {
            edges.push(GEdge { from: id, to, label: "근거".into() });
        }
    }
    for d in Dim::ALL {
        if d == Dim::Time {
            continue;
        }
        let i = d.ix();
        let cid = format!("cap:{}", d.id());
        nodes.push(GNode { id: cid.clone(), label: format!("{} 능력", dim_ko(d)), kind: "capability".into(), detail: Some(format!("편안 {:.3} · 한계 {:.3}", pr.comfort[i], pr.limit[i])) });
        nodes.push(GNode { id: format!("sens:{}", d.id()), label: format!("{} 민감도", dim_ko(d)), kind: "sensitivity".into(), detail: Some(format!("{:.2}", pr.sens[i])) });
        nodes.push(GNode { id: format!("dim:{}", d.id()), label: format!("{} {}", d.id(), dim_ko(d)), kind: "dimension".into(), detail: Some(format!("{:?}·{:?}", w.defs.dims[i].aggregate, w.defs.dims[i].limit)) });
        edges.push(GEdge { from: "persona".into(), to: cid.clone(), label: String::new() });
        edges.push(GEdge { from: cid, to: format!("dim:{}", d.id()), label: "부담 위치".into() });
        edges.push(GEdge { from: format!("sens:{}", d.id()), to: format!("dim:{}", d.id()), label: "가중".into() });
        edges.push(GEdge { from: "persona".into(), to: format!("sens:{}", d.id()), label: String::new() });
    }
    for it in Interest::ALL {
        let v = pr.interest[it.ix()];
        if v <= 0.0 {
            continue;
        }
        let iid = format!("int:{}", it.id());
        nodes.push(GNode { id: iid.clone(), label: format!("{} 관심", interest_ko(it)), kind: "interest".into(), detail: Some(format!("{v:.2}")) });
        nodes.push(GNode { id: format!("idim:{}", it.id()), label: it.id().to_string(), kind: "interest_dim".into(), detail: None });
        edges.push(GEdge { from: "persona".into(), to: iid.clone(), label: String::new() });
        edges.push(GEdge { from: iid, to: format!("idim:{}", it.id()), label: "제공과 곱".into() });
    }
    for (id, label, v) in [("prop:risk", "무리 감수", pr.risk), ("prop:punct", "시간 엄수", pr.punctuality), ("prop:expl", "탐색", pr.exploration), ("prop:att", "주목", pr.attention)] {
        nodes.push(GNode { id: id.into(), label: label.into(), kind: "propensity".into(), detail: Some(format!("{v:.2}")) });
        edges.push(GEdge { from: "persona".into(), to: id.into(), label: String::new() });
    }
    nodes.push(GNode { id: "perc".into(), label: "시야".into(), kind: "perception".into(), detail: Some(format!("초점 {:.0}m·{:.0}° / 주변 {:.0}m·{:.0}°", pr.perc.focal_range_m, pr.perc.focal_angle_deg, pr.perc.peripheral_range_m, pr.perc.peripheral_angle_deg)) });
    edges.push(GEdge { from: "persona".into(), to: "perc".into(), label: String::new() });
    plot::ontology_map(&format!("페르소나 온톨로지 지도 — {}", pr.persona_id), &nodes, &edges)
}

/// 그림과 보고서를 쓰고 (파일 이름 → 바이트 수)를 돌려준다.
pub fn write_report(out: &Path, w: &World, state: &SimState, visit: &[VisitDecision], rows: &RowsPart) -> Result<BTreeMap<String, usize>> {
    write_report_with(out, w, state, visit, rows, &[])
}

/// `extra`(예: 월 고리의 p14)를 보고서 끝에 덧붙인다. 각 항목은 (파일 이름, 제목, 설명, svg).
pub fn write_report_with(out: &Path, w: &World, state: &SimState, visit: &[VisitDecision], rows: &RowsPart, extra: &[(String, String, String, String)]) -> Result<BTreeMap<String, usize>> {
    let dir = out.join("plots");
    std::fs::create_dir_all(&dir)?;
    let space = w.space;
    let acc = &state.acc;
    let bands: BTreeMap<&str, String> = w.bundle.attrs.as_ref().map(|v| v.iter().filter_map(|a| a.age_band.clone().map(|b| (a.persona_id.as_str(), b))).collect()).unwrap_or_default();
    let base = map_base(w);
    let mut svgs: Vec<(usize, String, String)> = Vec::new(); // (그림 번호, svg, 설명)

    // p01
    let portals: Vec<PortalPoint> = space.portals.iter().map(|p| PortalPoint { pos: (p.pos.x, p.pos.z), kind: p.kind.as_str().into(), name: p.name.clone() }).collect();
    svgs.push((0, plot::space_map(FIGURES[0].1, &base, &places(w, 8), &portals), format!("걷기 칸(0.5m 격자), 건물 {}동, 장소 {}곳(제외 {}곳), 출입구 {}곳.", space.buildings.len(), space.places.len(), space.excluded_places.len(), space.portals.len())));
    // p02
    let labels: Vec<String> = acc.frames.iter().map(|f| fmt_hhmm(f.start_min)).collect();
    let arrivals = Series { name: "도착(명)".into(), values: acc.frames.iter().map(|f| f.arrivals as f64).collect() };
    let conc = Series { name: "동시 체류(평균, 명)".into(), values: acc.frames.iter().map(|f| f.conc_sum / f.samples.max(1) as f64).collect() };
    svgs.push((1, plot::bar_line_chart(FIGURES[1].1, &labels, &arrivals, &conc, "도착(명/10분)", "동시 체류(명)"), "막대는 10분마다 도착한 몸 수, 선은 렌더 프레임 평균 동시 체류 인원이다.".into()));
    // p03
    let peak = (0..acc.frames.len()).max_by(|&a, &b| acc.frames[a].conc_max.cmp(&acc.frames[b].conc_max).then(b.cmp(&a))).unwrap_or(0);
    let pf = &acc.frames[peak];
    let ev: Vec<EdgeValue> = space.edges.iter().enumerate().map(|(e, es)| EdgeValue { pts: es.poly.iter().map(|p| (p.x, p.z)).collect(), value: pf.edge_density_sum.get(e).copied().unwrap_or(0.0) / pf.samples.max(1) as f64 }).collect();
    let th = w.defs.params.v("crowd.los_thresholds").unwrap_or_else(|_| vec![0.31, 0.43, 0.72, 1.08, 2.17]);
    let legend = ["A", "B", "C", "D", "E", "F"];
    svgs.push((2, plot::density_map(&format!("{} ({}~)", FIGURES[2].1, fmt_hhmm(pf.start_min)), &base, &ev, &th, &legend[..(th.len() + 1).min(6)]), "간선 밀도(인/㎡)를 Fruin 보도 서비스 수준 경계로 나눠 칠했다.".into()));
    // p04
    let tracks: Vec<Track> = acc.tracks.values().map(|t| Track { pts: t.pts.iter().map(|&(x, z)| (x as f64, z as f64)).collect(), group: state.bodies.get(t.body as usize).map(|b| group_of(w, b, &bands)).unwrap_or_else(|| "배경".into()) }).collect();
    svgs.push((3, plot::trajectories(FIGURES[3].1, &base, &tracks), format!("몸 {}명의 궤적(5초 간격). 색은 연령대(속성이 없으면 집단).", tracks.len())));
    // p05
    let snap_places: Vec<PlacePoint> = places(w, 0);
    let notice_at: BTreeMap<u64, Vec<(f64, f64)>> = rows.events.iter().filter(|e| e.kind == "notice").filter_map(|e| e.place.map(|p| (e.tick, (space.places[p as usize].anchor.x, space.places[p as usize].anchor.z)))).fold(BTreeMap::new(), |mut m, (t, p)| {
        m.entry(t).or_insert_with(Vec::new).push(p);
        m
    });
    let panels: Vec<SnapshotPanel> = acc
        .snaps
        .iter()
        .map(|s| SnapshotPanel {
            caption: fmt_hhmm(s.t_s / 60.0) + &format!(":{:02}", (s.t_s % 60.0).round() as u32),
            glyphs: s
                .glyphs
                .iter()
                .map(|g| Glyph {
                    pos: (g.x, g.z),
                    heading: g.heading,
                    gaze: g.gaze,
                    focal_range: g.focal_range,
                    focal_angle_deg: g.focal_angle,
                    group: state.bodies.get(g.body as usize).map(|b| group_of(w, b, &bands)).unwrap_or_default(),
                    looking_at: g.looking_at,
                    activity: mdsim_runtime::record::activity_name(g.activity).into(),
                })
                .collect(),
            noticed: notice_at.get(&s.tick).cloned().unwrap_or_default(),
            places: snap_places.clone(),
        })
        .collect();
    let (cx, cz) = acc.snap_center.unwrap_or(((space.bbox.min.x + space.bbox.max.x) / 2.0, (space.bbox.min.z + space.bbox.max.z) / 2.0));
    let r = mdsim_runtime::summary::SNAP_RADIUS_M;
    svgs.push((4, plot::snapshot_strip(FIGURES[4].1, &base, (cx - r, cz - r, cx + r, cz + r), &panels), "2초 간격 렌더 프레임. 부채꼴은 초점 시야, 점선은 시선이 머문 장소, 원은 그 틱에 알아차린 장소.".into()));
    // p06
    let mut gaze: BTreeMap<String, [f64; 3]> = BTreeMap::new();
    for b in state.bodies.iter().filter(|b| b.profile.is_some()) {
        let g = gaze.entry(group_of(w, b, &bands)).or_insert([0.0; 3]);
        g[0] += b.stats.gaze_path_s;
        g[1] += b.stats.gaze_footing_s;
        g[2] += b.stats.gaze_place_s;
    }
    let stack: Vec<StackRow> = gaze.iter().map(|(k, v)| StackRow { label: k.clone(), parts: vec![("앞길".into(), v[0]), ("발밑".into(), v[1]), ("장소".into(), v[2])] }).collect();
    svgs.push((5, plot::stacked_share_bars(FIGURES[5].1, &stack), "LOOK 결정이 정한 시선 대상별 누적 시간의 비율.".into()));
    // p07
    let mut cat: BTreeMap<String, [f64; 3]> = BTreeMap::new();
    for e in &rows.events {
        let Some(p) = e.place else { continue };
        let c = category_ko(&space.places[p as usize].category);
        match e.kind.as_str() {
            "enter" => cat.entry(c).or_insert([0.0; 3])[0] += 1.0,
            "purchase" => {
                let v = cat.entry(c).or_insert([0.0; 3]);
                v[1] += 1.0;
                v[2] += e.value / 10_000.0;
            }
            _ => {}
        }
    }
    let gb = GroupedBars {
        categories: cat.keys().cloned().collect(),
        series: vec![
            Series { name: "방문(건)".into(), values: cat.values().map(|v| v[0]).collect() },
            Series { name: "구매(건)".into(), values: cat.values().map(|v| v[1]).collect() },
            Series { name: "매출(만원)".into(), values: cat.values().map(|v| v[2]).collect() },
        ],
    };
    svgs.push((6, plot::grouped_bars(FIGURES[6].1, &gb, "건 / 만원"), "입장·구매 사건을 장소 업종으로 모았다.".into()));
    // p08
    let mut noticed: BTreeMap<u32, (f64, BTreeMap<i64, ()>)> = BTreeMap::new();
    let mut entered: BTreeMap<(u32, i64), ()> = BTreeMap::new();
    for e in &rows.events {
        if let Some(p) = e.place {
            if e.kind == "notice" {
                let v = noticed.entry(p).or_insert((0.0, BTreeMap::new()));
                v.0 += 1.0;
                v.1.insert(e.body, ());
            } else if e.kind == "enter" {
                entered.insert((p, e.body), ());
            }
        }
    }
    let mut hb: Vec<HBar> = noticed
        .iter()
        .map(|(p, (n, bodies))| {
            let visited = bodies.keys().filter(|b| entered.contains_key(&(*p, **b))).count() as f64;
            HBar { label: space.places[*p as usize].name.clone(), value: *n, secondary: Some(visited / bodies.len().max(1) as f64) }
        })
        .collect();
    hb.sort_by(|a, b| b.value.total_cmp(&a.value).then(a.label.cmp(&b.label)));
    hb.truncate(20);
    svgs.push((7, plot::hbar_chart(FIGURES[7].1, &hb, "알아차림(건)", Some("알아차린 몸 중 들어간 비율")), "알아차림 사건 수 상위 20곳.".into()));
    // p09
    let mut dwell: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for b in state.bodies.iter().filter(|b| b.profile.is_some()) {
        if let Some(d) = b.departed_tick {
            dwell.entry(group_of(w, b, &bands)).or_default().push((d - b.arrival_tick) as f64 * w.scn.dt_s / 60.0);
        }
    }
    let dists: Vec<Dist> = dwell.into_iter().map(|(k, v)| Dist { label: k, values: v }).collect();
    svgs.push((8, plot::box_plot(FIGURES[8].1, &dists, "체류(분)"), "창 안에서 떠난 몸의 도착~떠남 시간. 끝까지 남은 몸은 빠진다.".into()));
    // p10
    let mut pts = Vec::new();
    for f in &acc.frames {
        for e in 0..space.edges.len() {
            if f.edge_speed_n.get(e).copied().unwrap_or(0) >= 5 && f.samples > 0 {
                let rho = f.edge_density_sum[e] / f.samples as f64;
                pts.push((rho, f.edge_speed_sum[e] / f.edge_speed_n[e] as f64));
            }
        }
    }
    let curve: Vec<(f64, f64)> = (0..=40).map(|k| {
        let rho = k as f64 * 0.1;
        (rho, mdsim_runtime::params::weidmann_speed(rho))
    }).collect();
    svgs.push((9, plot::scatter_with_curve(FIGURES[9].1, &pts, &curve, "밀도(인/㎡)", "속도(m/s)", "Weidmann 1993 기준"), "10분 프레임 × 간선(이동 표본 5개 이상)의 평균 밀도와 속도.".into()));
    // p11
    let (mut pm, mut pc, mut ps) = (0.0, BTreeMap::<String, f64>::new(), 0.0);
    for d in visit {
        for a in &d.alts {
            match &a.kind {
                VisitKind::Myeongdong { .. } => pm += a.prob,
                VisitKind::Competitor { id, .. } => *pc.entry(id.clone()).or_default() += a.prob,
                VisitKind::Skip => ps += a.prob,
            }
        }
    }
    let n = visit.len().max(1) as f64;
    let mut vb = vec![HBar { label: "명동".into(), value: pm, secondary: Some(pm / n) }];
    for (id, v) in &pc {
        let name = w.bundle.competitors.iter().find(|c| &c.id == id).map(|c| c.name.clone()).unwrap_or(id.clone());
        vb.push(HBar { label: name, value: *v, secondary: Some(v / n) });
    }
    vb.push(HBar { label: "안 감".into(), value: ps, secondary: Some(ps / n) });
    svgs.push((10, plot::hbar_chart(FIGURES[10].1, &vb, "확률 합(기대 인원)", Some("인물 중 비율")), format!("인물 {}명의 VISIT 결정 확률 합. 몸은 대표 비율로 실현된다.", visit.len())));
    // p12
    svgs.push((11, ontology_map(w), "근거(원문 범위) → 능력·민감도·관심·성향·시야 → 공통 차원. 나이·성별·국적은 반응 입력에 없다.".into()));
    // p13: 대안 3개 이상인 첫 NEXT 결정
    let dec = rows.decisions.iter().find(|d| d.context == "NEXT" && d.n_alts >= 3).or(rows.decisions.first());
    let (alts, terms, cap) = match dec {
        Some(d) => {
            let alts: Vec<AltBar> = rows.alts.iter().filter(|a| a.decision == d.id).map(|a| AltBar { label: format!("{} {}", action_ko(&a.action), target_label(w, &a.target)), prob: a.prob, chosen: a.alt == d.chosen }).collect();
            let terms: Vec<(String, f64)> = d.top.iter().map(|(t, v)| (term_ko(t), *v)).collect();
            (alts, terms, format!("결정 D:{} (틱 {}, 계기 {}). 오른쪽은 선택과 차선의 효용 차이 상위 3항.", d.id, d.tick, d.trigger))
        }
        None => (Vec::new(), Vec::new(), "결정 기록이 없다.".into()),
    };
    svgs.push((12, plot::decision_chart(FIGURES[12].1, &alts, &terms), cap));

    let mut written = BTreeMap::new();
    let mut sections = Vec::new();
    for (k, svg, caption) in svgs {
        let (name, title) = FIGURES[k];
        let file = format!("{name}.svg");
        std::fs::write(dir.join(&file), &svg)?;
        written.insert(format!("plots/{file}"), svg.len());
        sections.push(ReportSection { title: title.into(), caption, svg });
    }
    for (file, title, caption, svg) in extra {
        std::fs::write(dir.join(file), svg)?;
        written.insert(format!("plots/{file}"), svg.len());
        sections.push(ReportSection { title: title.clone(), caption: caption.clone(), svg: svg.clone() });
    }
    let m = &w.bundle.manifest;
    let summary: Vec<(String, String)> = vec![
        ("시나리오".into(), format!("{} ({}, {} {}~{})", w.scn.id, w.scn.day_type, w.scn.date, w.scn.window.start, w.scn.window.end)),
        ("번들".into(), m.bundle_id.clone()),
        ("정의 해시".into(), w.defs.sha256.clone()),
        ("차원 버전".into(), w.defs.dims_version.clone()),
        ("시나리오 해시".into(), w.scenario_sha256.clone()),
        ("시드".into(), w.scn.seed.to_string()),
        ("틱".into(), format!("{} × {}초", state.tick, w.scn.dt_s)),
        ("몸".into(), format!("{}명(배경 {}명)", state.bodies.len(), state.bodies.iter().filter(|b| b.profile.is_none()).count())),
        ("결정".into(), format!("VISIT {} · 실행 중 {}", visit.len(), rows.decisions.len())),
        ("사건".into(), rows.events.len().to_string()),
        ("상호작용".into(), if w.scn.interactions { "켬".into() } else { "끔".into() }),
    ];
    let mut labels: Vec<String> = m.assumption_flags.to_vec();
    labels.extend(m.pending_inputs.iter().map(|p| format!("대기 자료 {p}")));
    let html = plot::report_html(&format!("명동 보행 시뮬레이션 — {}", w.scn.id), &summary, &labels, &sections);
    std::fs::write(out.join("report.html"), &html)?;
    written.insert("report.html".into(), html.len());
    Ok(written)
}

/// 월 고리: 마지막 달·시나리오 하루 유형의 하루로 p01~p13을 그리고 p14 월별 상점 행동을 더한다.
pub fn write_report_monthly(out: &Path, w: &World, last: &mdsim_runtime::monthly::LastDay, rows: &[mdsim_runtime::output::StoreMonthRow]) -> Result<BTreeMap<String, usize>> {
    let months: std::collections::BTreeSet<u32> = rows.iter().map(|r| r.month).collect();
    let stack: Vec<StackRow> = months
        .iter()
        .map(|m| {
            let mut counts: BTreeMap<String, f64> = BTreeMap::new();
            for r in rows.iter().filter(|r| r.month == *m) {
                *counts.entry(action_ko(&r.action).to_string()).or_default() += 1.0;
            }
            StackRow { label: format!("{m}달"), parts: counts.into_iter().collect() }
        })
        .collect();
    let svg = plot::stacked_share_bars("월별 상점 행동", &stack);
    let closed = rows.iter().filter(|r| r.month == *months.iter().last().unwrap_or(&0) && !r.open).count();
    let caption = format!("상점 행위자의 달 끝 결정(02d I-211-r). 마지막 달 폐업 상점 {closed}곳. 재무는 가정값(첫 달 매출 비율)이면 store_months.assumed = true.");
    write_report_with(out, w, &last.state, &last.visit, &last.sample, &[("p14_store_months.svg".into(), "월별 상점 행동".into(), caption, svg)])
}

/// 실행 폴더의 마지막 체크포인트와 기록 조각으로 그림을 다시 만든다(`mdsim plot`).
pub fn rebuild_report(run: &Path, w: &World) -> Result<BTreeMap<String, usize>> {
    let mut names: Vec<String> = std::fs::read_dir(run.join("checkpoints"))?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    let last = names.last().ok_or_else(|| SimError::Run(format!("{}에 체크포인트가 없다(--no-checkpoints로 돈 실행은 다시 그릴 수 없다)", run.display())))?;
    let state = restore_state(&std::fs::read(run.join("checkpoints").join(last))?)?;
    let mut plot_rows = PlotRows::default();
    FileSink::for_each_part(run, &mut |part| {
        plot_rows.absorb(part);
        Ok(())
    })?;
    let visit = visit_decisions_streamed(w.defs, w.space, w.bundle, w.scn, &w.p, &w.rng, 4096, &mut |_| Ok(()))?;
    write_report(run, w, &state, &visit, &plot_rows.into_rows())
}

/// 대안 대상 표기(place:…, portal:…, node:…, route:k)를 사람이 읽는 이름으로.
fn target_label(w: &World, t: &str) -> String {
    if let Some(id) = t.strip_prefix("place:") {
        return w.space.place_ix(id).map(|i| w.space.places[i].name.clone()).unwrap_or_else(|| id.to_string());
    }
    if let Some(id) = t.strip_prefix("portal:") {
        return w.space.portal_ix(id).map(|i| w.space.portals[i].name.clone()).unwrap_or_else(|| id.to_string());
    }
    if let Some(id) = t.strip_prefix("node:") {
        return format!("교차점 {id}");
    }
    if let Some(k) = t.strip_prefix("route:") {
        return format!("경로 {k}");
    }
    t.to_string()
}
