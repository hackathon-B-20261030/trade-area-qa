import type { RunOptions } from './client.ts';

export function sameOptions(a: RunOptions | null | undefined, b: RunOptions): boolean {
  if (!a) return false;
  const keys = ['pedestrian_hours', 'width_delta_m', 'stall_scale', 'bench_count', 'discount_pct',
    'hours_extension', 'income_multiplier', 'price_multiplier', 'visibility_level'] as const;
  if (keys.some((key) => a[key] !== b[key])) return false;
  const ids = Object.keys(a.building_overrides).sort();
  const otherIds = Object.keys(b.building_overrides).sort();
  if (ids.length !== otherIds.length || ids.some((id, index) => id !== otherIds[index])) return false;
  return ids.every((id) => {
    const x = a.building_overrides[id], y = b.building_overrides[id];
    return x.type === y.type && x.discount_pct === y.discount_pct &&
      x.price_multiplier === y.price_multiplier && x.hours_extension === y.hours_extension;
  });
}

/** Guard against multiple clicks while the request or another run is active. */
export function createRunGuard() {
  let requesting = false;
  return {
    begin(busy: boolean): boolean {
      if (busy || requesting) return false;
      requesting = true;
      return true;
    },
    end(): void { requesting = false; },
  };
}
