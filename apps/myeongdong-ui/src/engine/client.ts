export type BuildingTypeOverride = 'original' | 'cafe' | 'restaurant' | 'fashion' | 'convenience' | 'medical';
export type BuildingOverride = {
  type: BuildingTypeOverride; discount_pct: 0 | 10 | 20 | null;
  price_multiplier: .9 | 1 | 1.1 | null; hours_extension: 0 | 1 | 2 | null;
};
export type RunOptions = {
  pedestrian_hours: 0 | 4 | 8; width_delta_m: 0 | .5 | 1; stall_scale: .8 | 1 | 1.2;
  bench_count: 0 | 10 | 20; discount_pct: 0 | 10 | 20; hours_extension: 0 | 1 | 2;
  income_multiplier: .9 | 1 | 1.1; price_multiplier: .9 | 1 | 1.1;
  visibility_level: 'low' | 'base' | 'high'; building_overrides: Record<string, BuildingOverride>;
};
export const DEFAULT_RUN_OPTIONS: RunOptions = {
  pedestrian_hours: 0, width_delta_m: 0, stall_scale: 1, bench_count: 0,
  discount_pct: 0, hours_extension: 0, income_multiplier: 1, price_multiplier: 1,
  visibility_level: 'base', building_overrides: {},
};
export const DEFAULT_BUILDING_OVERRIDE: BuildingOverride = {
  type: 'original', discount_pct: null, price_multiplier: null, hours_extension: null,
};
export type Job = {
  run_id: string; replay_base: string; state: 'preparing' | 'building' | 'queued' | 'running' | 'complete' | 'failed';
  options: RunOptions; legacy_options?: boolean; planned_bodies: number; error: string | null; status_available?: boolean;
  completed_tick?: number; total_ticks?: number; spawned_bodies?: number;
};
export type BuildingMetrics = { enters: number; purchases: number; revenue_krw: number };
export type Metrics = {
  schema_version: 1; run_id: string;
  totals: { arrivals: number; enters: number; purchases: number; revenue_krw: number; peak_active: number };
  buildings: Record<string, BuildingMetrics>; options: RunOptions; legacy_options?: boolean; limitations: string[];
};
export type BuildingCatalog = Record<string, { eligible: boolean; basis: 'reference_map' | 'glb_center_assumed'; original_store: boolean; distance_m: number }>;

// Old runs only recorded two settings; defaults describe the next run, not the old result.
export function normalizeOptions(raw: Partial<RunOptions> & { pedestrian_only?: boolean } = {}): RunOptions {
  const { pedestrian_only, ...current } = raw;
  return { ...DEFAULT_RUN_OPTIONS, ...current,
    pedestrian_hours: current.pedestrian_hours ?? (pedestrian_only ? 8 : 0),
    building_overrides: current.building_overrides ?? {} };
}
function normalizeRecord<T extends { options: RunOptions; legacy_options?: boolean }>(value: T): T {
  const raw = (value.options ?? {}) as Partial<RunOptions> & { pedestrian_only?: boolean };
  return { ...value, options: normalizeOptions(raw), legacy_options: !Object.prototype.hasOwnProperty.call(raw, 'stall_scale') };
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try { response = await fetch(path, { cache: 'no-store', ...init }); }
  catch { throw new Error('로컬 엔진 서버에 연결할 수 없습니다'); }
  let data: unknown;
  try { data = await response.json(); } catch { throw new Error('엔진 서버 응답이 올바르지 않습니다'); }
  if (!response.ok) {
    const message = data && typeof data === 'object' && 'error' in data ? String(data.error) : `요청 실패 (${response.status})`;
    throw new Error(message);
  }
  return data as T;
}

export function getHealth(): Promise<{ ready: boolean; error?: string; planned_bodies?: number }> {
  return request('/api/health');
}
export function getBuildings(): Promise<{ buildings: BuildingCatalog }> { return request('/api/buildings'); }
export async function startRun(options: RunOptions): Promise<Job> {
  return normalizeRecord(await request<Job>('/api/runs', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(options) }));
}
export function getJob(id: string): Promise<Job> {
  if (!/^ui-[a-f0-9]{32}$/.test(id)) throw new Error('실행 ID가 올바르지 않습니다');
  return request<Job>(`/api/runs/${id}`).then(normalizeRecord);
}
export function getMetrics(id: string): Promise<Metrics> {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,80}$/.test(id) || id.includes('..')) throw new Error('실행 ID가 올바르지 않습니다');
  return request<Metrics>(`/api/runs/${id}/metrics`).then(normalizeRecord);
}
