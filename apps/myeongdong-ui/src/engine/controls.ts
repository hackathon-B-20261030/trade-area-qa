import type { RunOptions } from './client.ts';
import { defaultScenarioChoices, scenarioVariables } from '../preview/scenarios.ts';

type ScalarKey = Exclude<keyof RunOptions, 'building_overrides'>;
type Binding = { key: ScalarKey; values: readonly (number | string)[]; hint?: string } | { reason: string };
export const CONTROL_BINDINGS: Record<number, Binding> = {
  1: { key: 'pedestrian_hours', values: [0, 4, 8], hint: '4시간 12–16시 · 8시간 12–20시' },
  2: { key: 'width_delta_m', values: [0, .5, 1] },
  3: { key: 'stall_scale', values: [.8, 1, 1.2], hint: '100% = 가정 노점 40개' },
  4: { key: 'bench_count', values: [0, 10, 20] },
  5: { key: 'discount_pct', values: [0, 10, 20] },
  6: { key: 'hours_extension', values: [0, 1, 2], hint: '14–17시 실험 밖의 연장은 결과가 같을 수 있음' },
  7: { reason: '월 단위 임대료 모형 미연결' },
  8: { reason: '월 단위 행사 모형 미연결' },
  9: { reason: '5,044명 직접 입장에서는 배차를 사용하지 않음' },
  10: { reason: '입장 인원 5,044명 고정' },
  11: { reason: '입장 인원 5,044명 고정' },
  12: { key: 'income_multiplier', values: [.9, 1, 1.1] },
  13: { reason: '입장 인원 5,044명 고정 · 경쟁 유입 미사용' },
  14: { reason: '월 단위 인건비 모형 미연결' },
  15: { key: 'price_multiplier', values: [.9, 1, 1.1] },
  16: { key: 'visibility_level', values: ['low', 'base', 'high'] },
};
export const supportedVariables = scenarioVariables.filter((variable) => 'key' in CONTROL_BINDINGS[variable.id]);
export { defaultScenarioChoices, scenarioVariables };
