//! 기록 (C-226, 02d I-226-r, S-E15).
//!
//! 렌더 프레임은 10분 묶음의 이진 파일, 사건·결정·대안·몸 표는 Parquet, 체크포인트는 json.gz다.

use crate::state::{Activity, Body, GazeTarget};
use crate::step::SimState;
use mdsim_core::defs::hex;
use mdsim_core::dims::ND;
use mdsim_core::geom::V2;
use mdsim_core::scenario::fmt_hhmm;
use mdsim_core::{Result, SimError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const RECORD_BYTES: usize = 14;
pub const POS_SCALE_M: f64 = 0.05;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventRow {
    pub tick: u64,
    pub t_s: f64,
    pub body: i64,
    pub kind: String,
    /// 장소 인덱스(기록할 때 place_id로 바꾼다).
    pub place: Option<u32>,
    pub value: f64,
    pub info: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRow {
    pub id: u64,
    pub tick: u64,
    pub body: i64,
    pub context: String,
    pub trigger: String,
    pub n_alts: u32,
    pub chosen: u32,
    pub forced: bool,
    pub top: Vec<(String, f64)>,
    pub base_min: f64,
    pub wait_min: f64,
    pub exit_walk_min: f64,
    /// 결정에 쓴 키드 난수의 정규 JSON 키(02c §2.7 (1)).
    pub rng_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AltRow {
    pub decision: u64,
    pub alt: u32,
    pub action: String,
    pub target: String,
    pub prob: f64,
    pub utility: f64,
    pub purpose: f64,
    pub interest: f64,
    pub cost: f64,
    pub time: f64,
    pub money: f64,
    pub cost_dims: [f64; ND],
}

/// 한 몸의 렌더 레코드(14바이트).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRec {
    pub body: u32,
    pub x: i16,
    pub z: i16,
    pub heading: u8,
    pub gaze: u8,
    pub focal_range: u8,
    pub focal_angle: u8,
    pub activity: u8,
    pub flags: u8,
}

impl FrameRec {
    pub fn pos(&self) -> V2 {
        V2::new(self.x as f64 * POS_SCALE_M, self.z as f64 * POS_SCALE_M)
    }

    pub fn heading_rad(&self) -> f64 {
        self.heading as f64 / 256.0 * std::f64::consts::TAU
    }

    pub fn gaze_rad(&self) -> f64 {
        self.gaze as f64 / 256.0 * std::f64::consts::TAU
    }
}

pub const FLAG_BACKGROUND: u8 = 1;
pub const FLAG_LOOK_PLACE: u8 = 2;
pub const FLAG_FOOTING: u8 = 4;
pub const FLAG_NOTICED: u8 = 8;
pub const FLAG_STROLLER: u8 = 16;

fn angle_u8(a: f64) -> u8 {
    let t = a.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU * 256.0;
    (t.round() as u32 % 256) as u8
}

fn q16(v: f64) -> i16 {
    (v / POS_SCALE_M).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
}

pub fn frame_records(bodies: &[Body], focal: &dyn Fn(&Body) -> (f64, f64)) -> Vec<FrameRec> {
    bodies
        .iter()
        .filter(|b| b.alive)
        .map(|b| {
            let (fr, fa) = focal(b);
            let mut flags = 0u8;
            if b.is_background() {
                flags |= FLAG_BACKGROUND;
            }
            match b.gaze_target {
                GazeTarget::Place(_) => flags |= FLAG_LOOK_PLACE,
                GazeTarget::Footing => flags |= FLAG_FOOTING,
                GazeTarget::Path => {}
            }
            if !b.new_notices.is_empty() {
                flags |= FLAG_NOTICED;
            }
            if b.stroller {
                flags |= FLAG_STROLLER;
            }
            FrameRec { body: b.id, x: q16(b.pos.x), z: q16(b.pos.z), heading: angle_u8(b.heading), gaze: angle_u8(b.gaze), focal_range: fr.round().clamp(0.0, 255.0) as u8, focal_angle: fa.round().clamp(0.0, 255.0) as u8, activity: b.activity.code(), flags }
        })
        .collect()
}

pub fn encode_frame(tick: u64, recs: &[FrameRec], out: &mut Vec<u8>) {
    out.extend_from_slice(&(tick as u32).to_le_bytes());
    out.extend_from_slice(&(recs.len() as u32).to_le_bytes());
    for r in recs {
        out.extend_from_slice(&r.body.to_le_bytes());
        out.extend_from_slice(&r.x.to_le_bytes());
        out.extend_from_slice(&r.z.to_le_bytes());
        out.extend_from_slice(&[r.heading, r.gaze, r.focal_range, r.focal_angle, r.activity, r.flags]);
    }
}

/// 묶음 파일 하나를 읽는다(재생·plot용). (머리말, [(틱, 레코드들)])
pub fn decode_chunk(bytes: &[u8]) -> Result<(serde_json::Value, Vec<(u64, Vec<FrameRec>)>)> {
    let nl = bytes.iter().position(|&b| b == b'\n').ok_or_else(|| SimError::Io("렌더 묶음에 머리말이 없음".into()))?;
    let header: serde_json::Value = serde_json::from_slice(&bytes[..nl]).map_err(|e| SimError::Io(e.to_string()))?;
    let mut i = nl + 1;
    let mut frames = Vec::new();
    let rd_u32 = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    while i + 8 <= bytes.len() {
        let tick = rd_u32(bytes, i) as u64;
        let n = rd_u32(bytes, i + 4) as usize;
        i += 8;
        if i + n * RECORD_BYTES > bytes.len() {
            return Err(SimError::Io("렌더 묶음이 잘렸음".into()));
        }
        let mut recs = Vec::with_capacity(n);
        for _ in 0..n {
            let b = &bytes[i..i + RECORD_BYTES];
            recs.push(FrameRec {
                body: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                x: i16::from_le_bytes([b[4], b[5]]),
                z: i16::from_le_bytes([b[6], b[7]]),
                heading: b[8],
                gaze: b[9],
                focal_range: b[10],
                focal_angle: b[11],
                activity: b[12],
                flags: b[13],
            });
            i += RECORD_BYTES;
        }
        frames.push((tick, recs));
    }
    Ok((header, frames))
}

pub fn chunk_header(tick_start: u64, tick_end: u64, dt_s: f64, render_every: u32, n_frames: usize) -> String {
    serde_json::json!({
        "format": "mdsim-frames-v1",
        "tick_start": tick_start,
        "tick_end": tick_end,
        "dt_s": dt_s,
        "render_every": render_every,
        "n_frames": n_frames,
        "record_bytes": RECORD_BYTES,
        "origin": [0.0, 0.0],
        "pos_scale_m": POS_SCALE_M,
        "fields": ["body:u32", "x:i16", "z:i16", "heading:u8", "gaze:u8", "focal_range_m:u8", "focal_angle_deg:u8", "activity:u8", "flags:u8"]
    })
    .to_string()
}

/// 기록 표의 인덱스 → 문자열 ID.
#[derive(Clone, Debug, Default)]
pub struct IdMaps {
    pub body_id: Vec<String>,
    pub place_id: Vec<String>,
}

impl IdMaps {
    pub fn body(&self, i: i64) -> Option<String> {
        if i < 0 {
            None
        } else {
            self.body_id.get(i as usize).cloned()
        }
    }

    pub fn place(&self, i: Option<u32>) -> Option<String> {
        i.and_then(|i| self.place_id.get(i as usize).cloned())
    }
}

/// 기록을 받는 쪽. 파일(FileSink)과 메모리(MemSink, 테스트)가 같은 바이트를 만든다.
pub trait Sink {
    fn frame(&mut self, tick: u64, recs: &[FrameRec]);
    fn event(&mut self, e: EventRow);
    fn decision(&mut self, d: DecisionRow, alts: Vec<AltRow>);
    /// 10분 묶음이 끝났을 때(체크포인트 시점). state는 묶음 끝 틱의 스냅샷이다.
    fn chunk_end(&mut self, chunk_tick_start: u64, chunk_tick_end: u64, state: &SimState, dt_s: f64, render_every: u32) -> Result<()>;
}

/// 메모리 기록(테스트·probe). 묶음 바이트의 해시와 사건을 모은다.
#[derive(Default)]
pub struct MemSink {
    pub keep_frames: bool,
    pub frames: Vec<(u64, Vec<FrameRec>)>,
    pub chunk_bytes: Vec<u8>,
    pub chunk_hashes: Vec<(u64, String)>,
    pub chunk_frames: usize,
    pub events: Vec<EventRow>,
    pub decisions: Vec<DecisionRow>,
    pub alts: Vec<AltRow>,
    pub checkpoints: Vec<(u64, Vec<u8>)>,
    pub keep_checkpoints: bool,
}

impl Sink for MemSink {
    fn frame(&mut self, tick: u64, recs: &[FrameRec]) {
        encode_frame(tick, recs, &mut self.chunk_bytes);
        self.chunk_frames += 1;
        if self.keep_frames {
            self.frames.push((tick, recs.to_vec()));
        }
    }

    fn event(&mut self, e: EventRow) {
        self.events.push(e);
    }

    fn decision(&mut self, d: DecisionRow, alts: Vec<AltRow>) {
        self.decisions.push(d);
        self.alts.extend(alts);
    }

    fn chunk_end(&mut self, s: u64, e: u64, state: &SimState, dt_s: f64, render_every: u32) -> Result<()> {
        let mut bytes = chunk_header(s, e, dt_s, render_every, self.chunk_frames).into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(&self.chunk_bytes);
        self.chunk_hashes.push((s, hex(&Sha256::digest(&bytes))));
        self.chunk_bytes.clear();
        self.chunk_frames = 0;
        if self.keep_checkpoints {
            self.checkpoints.push((e, checkpoint_bytes(state)?));
        }
        Ok(())
    }
}

pub fn checkpoint_bytes(state: &SimState) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(state).map_err(|e| SimError::Io(e.to_string()))?;
    let mut enc = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::default());
    enc.write_all(&json)?;
    Ok(enc.finish()?)
}

pub fn restore_state(bytes: &[u8]) -> Result<SimState> {
    let mut dec = flate2::read::GzDecoder::new(bytes);
    let mut json = Vec::new();
    std::io::Read::read_to_end(&mut dec, &mut json)?;
    serde_json::from_slice(&json).map_err(|e| SimError::Io(format!("체크포인트를 읽을 수 없음: {e}")))
}

/// 파일 기록. 10분 묶음마다 렌더 묶음·체크포인트·기록 줄 조각(parts/rows_HHMM.json.gz)을 쓰고,
/// 실행 끝에 조각을 모아 표를 만든다(체크포인트에서 이어 달리기가 가능하고 메모리가 묶음 크기로 묶인다).
pub struct FileSink {
    pub dir: PathBuf,
    chunk_bytes: Vec<u8>,
    chunk_frames: usize,
    pub chunk_files: BTreeMap<String, String>,
    pub events: Vec<EventRow>,
    pub decisions: Vec<DecisionRow>,
    pub alts: Vec<AltRow>,
    pub window_start_s: f64,
    pub dt_s: f64,
    pub write_checkpoints: bool,
    /// 렌더 묶음을 쓸지(월 고리는 쓰지 않는다, S-E40).
    pub write_frames: bool,
}

/// 기록 줄 조각 하나.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RowsPart {
    pub events: Vec<EventRow>,
    pub decisions: Vec<DecisionRow>,
    pub alts: Vec<AltRow>,
}

fn gz_json<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(v).map_err(|e| SimError::Io(e.to_string()))?;
    let mut enc = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::default());
    enc.write_all(&json)?;
    Ok(enc.finish()?)
}

fn un_gz_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let mut dec = flate2::read::GzDecoder::new(bytes);
    let mut json = Vec::new();
    std::io::Read::read_to_end(&mut dec, &mut json)?;
    serde_json::from_slice(&json).map_err(|e| SimError::Io(e.to_string()))
}

impl FileSink {
    pub fn new(dir: &Path, window_start_s: f64, dt_s: f64) -> Result<FileSink> {
        for sub in ["frames", "checkpoints", "parts"] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        Ok(FileSink { dir: dir.to_path_buf(), chunk_bytes: Vec::new(), chunk_frames: 0, chunk_files: BTreeMap::new(), events: Vec::new(), decisions: Vec::new(), alts: Vec::new(), window_start_s, dt_s, write_checkpoints: true, write_frames: true })
    }

    pub fn hhmm(&self, tick: u64) -> String {
        fmt_hhmm((self.window_start_s + tick as f64 * self.dt_s) / 60.0).replace(':', "")
    }

    /// 체크포인트 틱에서 이어 달리기: 그 뒤의 렌더 묶음·조각·체크포인트를 지우고 앞의 렌더 묶음 해시를 다시 모은다.
    pub fn prepare_resume(&mut self, from_tick: u64) -> Result<()> {
        let cut = self.hhmm(from_tick);
        for sub in ["frames", "parts", "checkpoints"] {
            for e in std::fs::read_dir(self.dir.join(sub))? {
                let e = e?;
                let name = e.file_name().to_string_lossy().to_string();
                let stamp: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
                let later = if sub == "checkpoints" { stamp > cut } else { stamp >= cut };
                if later {
                    std::fs::remove_file(e.path())?;
                }
            }
        }
        self.chunk_files.clear();
        let mut names: Vec<String> = std::fs::read_dir(self.dir.join("frames"))?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        for n in names {
            let rel = format!("frames/{n}");
            self.chunk_files.insert(rel.clone(), mdsim_core::pq::file_sha256(&self.dir.join(&rel))?);
        }
        Ok(())
    }

    /// 조각을 시각 순으로 하나씩 넘긴다(메모리는 조각 하나 크기).
    pub fn for_each_part(dir: &Path, f: &mut dyn FnMut(RowsPart) -> Result<()>) -> Result<()> {
        let mut names: Vec<String> = std::fs::read_dir(dir.join("parts"))?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".json.gz")).collect();
        names.sort();
        for n in names {
            f(un_gz_json(&std::fs::read(dir.join("parts").join(n))?)?)?;
        }
        Ok(())
    }

    /// 조각을 시각 순으로 모은다.
    pub fn collect_parts(dir: &Path) -> Result<RowsPart> {
        let mut names: Vec<String> = std::fs::read_dir(dir.join("parts"))?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".json.gz")).collect();
        names.sort();
        let mut all = RowsPart::default();
        for n in names {
            let part: RowsPart = un_gz_json(&std::fs::read(dir.join("parts").join(n))?)?;
            all.events.extend(part.events);
            all.decisions.extend(part.decisions);
            all.alts.extend(part.alts);
        }
        Ok(all)
    }
}

impl Sink for FileSink {
    fn frame(&mut self, tick: u64, recs: &[FrameRec]) {
        if !self.write_frames {
            return;
        }
        encode_frame(tick, recs, &mut self.chunk_bytes);
        self.chunk_frames += 1;
    }

    fn event(&mut self, e: EventRow) {
        self.events.push(e);
    }

    fn decision(&mut self, d: DecisionRow, alts: Vec<AltRow>) {
        self.decisions.push(d);
        self.alts.extend(alts);
    }

    fn chunk_end(&mut self, s: u64, e: u64, state: &SimState, dt_s: f64, render_every: u32) -> Result<()> {
        let mut bytes = chunk_header(s, e, dt_s, render_every, self.chunk_frames).into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(&self.chunk_bytes);
        if self.write_frames {
            let name = format!("frames/frame_{}.bin", self.hhmm(s));
            std::fs::write(self.dir.join(&name), &bytes)?;
            self.chunk_files.insert(name, hex(&Sha256::digest(&bytes)));
        }
        self.chunk_bytes.clear();
        self.chunk_frames = 0;
        let part = RowsPart { events: std::mem::take(&mut self.events), decisions: std::mem::take(&mut self.decisions), alts: std::mem::take(&mut self.alts) };
        std::fs::write(self.dir.join(format!("parts/rows_{}.json.gz", self.hhmm(s))), gz_json(&part)?)?;
        if self.write_checkpoints {
            let name = format!("checkpoints/ckpt_{}.json.gz", self.hhmm(e));
            std::fs::write(self.dir.join(&name), checkpoint_bytes(state)?)?;
        }
        Ok(())
    }
}

pub fn activity_name(code: u8) -> &'static str {
    match code {
        1 => Activity::Walk.name(),
        2 => Activity::Browse.name(),
        3 => Activity::Queue.name(),
        4 => Activity::Inside.name(),
        5 => Activity::Rest.name(),
        6 => Activity::Watch.name(),
        7 => Activity::Exit.name(),
        _ => Activity::Gone.name(),
    }
}
