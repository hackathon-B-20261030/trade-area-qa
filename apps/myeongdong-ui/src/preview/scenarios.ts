export type BuildingType = 'cafe' | 'restaurant' | 'medical' | 'fashion' | 'convenience' | 'other' | 'none';

export type BuildingMeta = {
  id: string;
  type: BuildingType;
  originalType: BuildingType;
  name: string;
  addr: string;
  height: number;
  storeCount: number;
};

export const scenarioVariables = [
  { id: 1, name: '보행자 전용 시간', kind: 'u', options: ['0시간', '4시간', '8시간'] },
  { id: 2, name: '보행 폭', unit: 'm', kind: 'u', options: ['기준', '+0.5', '+1.0'] },
  { id: 3, name: '노점 수', kind: 'u', options: ['기준의 80%', '100%', '120%'] },
  { id: 4, name: '벤치 수', kind: 'u', options: ['0개', '10개', '20개'] },
  { id: 5, name: '할인율', kind: 'u', options: ['0%', '10%', '20%'] },
  { id: 6, name: '영업시간', kind: 'u', options: ['기준', '+1시간', '+2시간'] },
  { id: 7, name: '임대료 지원율', kind: 'u', options: ['0%', '10%', '20%'] },
  { id: 8, name: '행사 횟수', kind: 'u', options: ['월 0회', '월 2회', '월 4회'] },
  { id: 9, name: '배차 간격', kind: 'u', options: ['기준의 80%', '100%', '120%'] },
  { id: 10, name: '방문 수요', kind: 'w', options: ['기준의 80%', '100%', '120%'] },
  { id: 11, name: '외국인 유입량', kind: 'w', note: true, options: ['기준의 50%', '100%', '150%'] },
  { id: 12, name: '방문객 평균 소득', kind: 'w', note: true, options: ['기준의 90%', '100%', '110%'] },
  { id: 13, name: '경쟁 상권과의 거리', kind: 'w', note: true, options: ['접근 −20%', '기준', '+20%'] },
  { id: 14, name: '인건비', kind: 'u', note: true, options: ['기준의 90%', '100%', '110%'] },
  { id: 15, name: '상품·서비스 가격', kind: 'u', note: true, options: ['기준의 90%', '100%', '110%'] },
  { id: 16, name: '시계성·옥외 광고', kind: 'u', note: true, options: ['낮음', '기준', '높음'] },
] as const;

export const defaultScenarioChoices: Record<number, number> = {
  3: 1,
  9: 1,
  10: 1,
  11: 1,
  12: 1,
  13: 1,
  14: 1,
  15: 1,
};

export function mockMonthlySales(building: BuildingMeta) {
  const typeBase: Record<BuildingType, number> = {
    cafe: 1350000,
    restaurant: 1980000,
    medical: 2240000,
    fashion: 1730000,
    convenience: 1480000,
    other: 920000,
    none: 480000,
  };
  const hash = [...building.id].reduce((sum, character) => ((sum * 31) + character.charCodeAt(0)) % 620000, 0);
  const dailyEstimate = typeBase[building.type] + building.storeCount * 185000 + hash;
  return Math.round((dailyEstimate * 30) / 1000) * 1000;
}

function scenarioMultiplier(choices: Record<number, number>) {
  const impact = scenarioVariables.reduce((sum, variable) => {
    const defaultChoice = defaultScenarioChoices[variable.id] ?? 0;
    const selectedChoice = choices[variable.id] ?? defaultChoice;
    const weight = variable.kind === 'w' ? .075 : .035;
    return sum + (selectedChoice - defaultChoice) * weight;
  }, 0);
  return Math.max(.55, Math.min(1.75, 1 + impact));
}

export function expectedFootTraffic(building: BuildingMeta, choices: Record<number, number>) {
  const hash = [...building.id].reduce((sum, character) => ((sum * 29) + character.charCodeAt(0)) % 780, 0);
  const baseTraffic = 460 + hash + building.height * 7 + building.storeCount * 95;
  return Math.round(baseTraffic * scenarioMultiplier(choices));
}

export function expectedMonthlySales(building: BuildingMeta, choices: Record<number, number>) {
  return Math.round((mockMonthlySales(building) * scenarioMultiplier(choices)) / 1000) * 1000;
}

