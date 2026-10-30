import { decodeFrameChunk, type Frame, type FrameChunk } from './frames.ts';

export type FrameRef = { path: string; sha256: string; tick_start: number; tick_end: number };
export type Alignment = {
  glb_sha256: string;
  center_fit_model_x: { slope: number; offset_m: number };
  center_fit_model_z: { slope: number; offset_m: number };
  vertices_max_m: number;
};
export type RunStatus = {
  schema_version: 1; status: 'running' | 'complete' | 'failed';
  window_start_s: number; window_end_s: number; dt_s: number; render_every_ticks: number;
  planned_bodies: number; spawned_bodies: number; completed_tick: number;
  frames: FrameRef[]; alignment: Alignment; limitations: string[]; error?: string;
};

const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const nonnegativeInt = (v: unknown): v is number => finite(v) && Number.isInteger(v) && v >= 0;
const hash = (v: unknown): v is string => typeof v === 'string' && /^[a-f0-9]{64}$/.test(v);

export function validateStatus(value: unknown): RunStatus {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('실행 상태 형식이 잘못되었습니다');
  const s = value as RunStatus;
  if (s.schema_version !== 1 || !['running', 'complete', 'failed'].includes(s.status) ||
    !finite(s.window_start_s) || !finite(s.window_end_s) || s.window_end_s <= s.window_start_s ||
    !finite(s.dt_s) || s.dt_s <= 0 || !nonnegativeInt(s.render_every_ticks) || s.render_every_ticks === 0 ||
    !nonnegativeInt(s.planned_bodies) || s.planned_bodies === 0 || !nonnegativeInt(s.spawned_bodies) ||
    s.spawned_bodies > s.planned_bodies || !nonnegativeInt(s.completed_tick) || !Array.isArray(s.frames) ||
    !Array.isArray(s.limitations) || s.limitations.some((x) => typeof x !== 'string')) {
    throw new Error('실행 상태 값이 올바르지 않습니다');
  }
  const a = s.alignment;
  if (!a || !hash(a.glb_sha256) || !a.center_fit_model_x || !a.center_fit_model_z ||
    !finite(a.center_fit_model_x.slope) || !finite(a.center_fit_model_x.offset_m) ||
    !finite(a.center_fit_model_z.slope) || !finite(a.center_fit_model_z.offset_m) ||
    !finite(a.vertices_max_m) || a.vertices_max_m > 0.2) throw new Error('지도 좌표 정합 정보가 올바르지 않습니다');
  let end = 0;
  for (const f of s.frames) {
    if (!f || typeof f.path !== 'string' || !/^frames\/frame_\d{4}\.bin$/.test(f.path) ||
      !hash(f.sha256) || !nonnegativeInt(f.tick_start) || !nonnegativeInt(f.tick_end) ||
      f.tick_start !== end || f.tick_end <= f.tick_start) throw new Error('프레임 목록이 올바르지 않습니다');
    end = f.tick_end;
  }
  if (end !== s.completed_tick || s.completed_tick * s.dt_s > s.window_end_s - s.window_start_s + 1e-6) {
    throw new Error('완료된 프레임 구간이 일치하지 않습니다');
  }
  return s;
}

export async function sha256(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest), (x) => x.toString(16).padStart(2, '0')).join('');
}

export async function loadRun(base: string): Promise<RunStatus> {
  if (!/^\/runs\/[A-Za-z0-9._-]+$/.test(base)) throw new Error('실행 경로가 지정되지 않았습니다');
  const response = await fetch(`${base}/run_status.json`, { cache: 'no-store' });
  if (!response.ok) throw new Error(`실행 상태를 불러오지 못했습니다 (${response.status})`);
  return validateStatus(await response.json());
}

export async function verifyMap(alignment: Alignment): Promise<void> {
  const response = await fetch('/myeongdong.glb');
  if (!response.ok) throw new Error('명동 3D 지도를 불러오지 못했습니다');
  if (await sha256(await response.arrayBuffer()) !== alignment.glb_sha256) throw new Error('지도 파일이 실행 좌표 정합의 원본과 다릅니다');
}

/** A newer seek invalidates every older asynchronous response. */
export class RequestGate {
  private generation = 0;
  issue(): number { return ++this.generation; }
  current(generation: number): boolean { return generation === this.generation; }
}

/** The frame is captured before arrivals scheduled for the same tick. */
export function arrivalsAt(tick: number, totalTicks: number, planned: number): number {
  if (!nonnegativeInt(tick) || !nonnegativeInt(totalTicks) || totalTicks === 0 || !nonnegativeInt(planned)) {
    throw new Error('입장 시각이 올바르지 않습니다');
  }
  return Math.min(planned, Math.ceil(tick * planned / totalTicks));
}

export class ReplayReader {
  private cache = new Map<string, FrameChunk>();
  private pending = new Map<string, Promise<FrameChunk>>();
  readonly base: string;
  constructor(base: string) { this.base = base; }

  async loadChunk(status: RunStatus, index: number): Promise<FrameChunk> {
    const ref = status.frames[index];
    if (!ref) throw new Error('아직 생성되지 않은 구간입니다');
    const key = `${ref.path}:${ref.sha256}`;
    const cached = this.cache.get(key);
    if (cached) { this.cache.delete(key); this.cache.set(key, cached); return cached; }
    const existing = this.pending.get(key);
    if (existing) return existing;
    const promise = (async () => {
      const response = await fetch(`${this.base}/${ref.path}`);
      if (!response.ok) throw new Error(`프레임을 불러오지 못했습니다 (${response.status})`);
      const bytes = await response.arrayBuffer();
      if (await sha256(bytes) !== ref.sha256) throw new Error('프레임 해시가 일치하지 않습니다');
      const chunk = decodeFrameChunk(bytes);
      if (chunk.header.tick_start !== ref.tick_start || chunk.header.tick_end !== ref.tick_end ||
        chunk.header.dt_s !== status.dt_s || chunk.header.render_every !== status.render_every_ticks) {
        throw new Error('프레임과 실행 상태의 시간 정보가 다릅니다');
      }
      this.cache.set(key, chunk);
      while (this.cache.size > 2) this.cache.delete(this.cache.keys().next().value!);
      return chunk;
    })();
    this.pending.set(key, promise);
    try { return await promise; } finally { this.pending.delete(key); }
  }

  async frameAt(status: RunStatus, seconds: number): Promise<Frame> {
    if (!finite(seconds) || seconds < 0) throw new Error('재생 시간이 올바르지 않습니다');
    const tick = Math.floor(seconds / status.dt_s);
    const index = status.frames.findIndex((f) => f.tick_start <= tick && tick < f.tick_end);
    if (index < 0) throw new Error('아직 생성되지 않은 구간입니다');
    const chunk = await this.loadChunk(status, index);
    const firstTick = Math.ceil(chunk.header.tick_start / chunk.header.render_every) * chunk.header.render_every;
    const frameIndex = Math.floor((tick - firstTick) / chunk.header.render_every);
    return chunk.readFrame(Math.max(0, Math.min(frameIndex, chunk.frameCount - 1)));
  }
}
