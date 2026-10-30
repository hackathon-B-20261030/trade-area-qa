"""Translate supported original UI variables into immutable Rust input bundles."""
from __future__ import annotations

import hashlib
import json
import math
import re
from pathlib import Path
import shutil
from typing import Any

import pyarrow as pa
import pyarrow.parquet as pq
import yaml

from prepare_myeongdong_5044 import PERSONAS, SPEC, estimate_income, income_spec

DEFAULTS: dict[str, Any] = {
    'pedestrian_hours': 0, 'width_delta_m': 0.0, 'stall_scale': 1.0,
    'bench_count': 0, 'discount_pct': 0, 'hours_extension': 0,
    'income_multiplier': 1.0, 'price_multiplier': 1.0,
    'visibility_level': 'base', 'building_overrides': {},
}
ENUMS = {
    'pedestrian_hours': (0, 4, 8), 'width_delta_m': (0.0, 0.5, 1.0),
    'stall_scale': (0.8, 1.0, 1.2), 'bench_count': (0, 10, 20),
    'discount_pct': (0, 10, 20), 'hours_extension': (0, 1, 2),
    'income_multiplier': (0.9, 1.0, 1.1), 'price_multiplier': (0.9, 1.0, 1.1),
    'visibility_level': ('low', 'base', 'high'),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def validate_options(options: Any) -> dict[str, Any]:
    if not isinstance(options, dict) or not options or set(options) - (set(DEFAULTS) | {'pedestrian_only'}):
        raise ValueError('지원하지 않는 시나리오 항목입니다')
    out = {**DEFAULTS, **options}
    if 'pedestrian_only' in options:
        if type(options['pedestrian_only']) is not bool or 'pedestrian_hours' in options:
            raise ValueError('이전 보행 전용 설정이 올바르지 않습니다')
        out['pedestrian_hours'] = 8 if options['pedestrian_only'] else 0
        del out['pedestrian_only']
    for key, allowed in ENUMS.items():
        val = out[key]
        if isinstance(val, bool) or val not in allowed or (key != 'visibility_level' and not isinstance(val, (int, float))):
            raise ValueError(f'{key} 값이 지원 범위를 벗어났습니다')
    overrides = out['building_overrides']
    if not isinstance(overrides, dict) or len(overrides) > 1252:
        raise ValueError('건물별 설정 형식이 올바르지 않습니다')
    normalized = {}
    for ident, value in sorted(overrides.items()):
        if not isinstance(ident, str) or not re.fullmatch(r'(way|relation|node)/[0-9]+', ident):
            raise ValueError('건물 ID 형식이 올바르지 않습니다')
        if not isinstance(value, dict) or set(value) - {'type','discount_pct','price_multiplier','hours_extension'}:
            raise ValueError(f'건물 {ident} 설정이 올바르지 않습니다')
        v = {'type':'original', 'discount_pct':None, 'price_multiplier':None, 'hours_extension':None, **value}
        if v['type'] not in ('original','cafe','restaurant','fashion','convenience','medical'):
            raise ValueError(f'건물 {ident} 업종을 지원하지 않습니다')
        for key in ('discount_pct','price_multiplier','hours_extension'):
            if v[key] is not None and (isinstance(v[key], bool) or v[key] not in ENUMS[key]):
                raise ValueError(f'건물 {ident}의 {key} 값이 올바르지 않습니다')
        normalized[ident] = v
    out['building_overrides'] = normalized
    return out


def _write_table(path: Path, original: pa.Table, rows: list[dict[str, Any]]) -> None:
    temp = path.with_suffix('.parquet.tmp')
    pq.write_table(pa.Table.from_pylist(rows, schema=original.schema), temp)
    temp.replace(path)


def _point_on_edges(edges: list[dict[str, Any]], distance: float) -> tuple[dict[str, Any], float, float, float]:
    if distance < 0:
        raise ValueError('배치 거리가 올바르지 않습니다')
    for edge in edges:
        length = edge['length_m']
        if distance <= length:
            pts = edge['polyline']
            if len(pts) != 2 or length <= 0:
                raise ValueError('두 점으로 된 유효 간선이 필요합니다')
            t = distance / length
            x = pts[0]['x'] + (pts[1]['x'] - pts[0]['x']) * t
            z = pts[0]['z'] + (pts[1]['z'] - pts[0]['z']) * t
            return edge, distance, x, z
        distance -= length
    raise ValueError('노점이나 벤치를 놓을 보행 공간이 부족합니다')


def _facility(template: dict[str, Any], edge: dict[str, Any], offset: float, x: float, z: float,
              index: int, kind: str) -> dict[str, Any]:
    row = dict(template)
    row.update({
        'place_id': f'{"L" if kind == "stall" else "R"}:{index:04d}',
        'class': 'stall' if kind == 'stall' else 'rest_facility',
        'category': 'stall' if kind == 'stall' else 'bench',
        'name': f'{"노점" if kind == "stall" else "벤치"} {index}',
        'x': x, 'z': z, 'edge_id': edge['edge_id'], 'offset_m': offset,
        'anchor_x': x, 'anchor_z': z, 'side': 'left' if index % 2 else 'right',
        'layer': 0, 'floor': 1, 'price_level_krw': 5000.0 if kind == 'stall' else None,
        'discount_pct': 0.0, 'open_min': 960 if kind == 'stall' else 0,
        'close_min': 1380 if kind == 'stall' else 1440,
        'day_types': ['weekday', 'weekend', 'event_day'], 'visibility': .6, 'fame': .2,
        'foreign_label': False, 'service_rate_per_min': 1.0 if kind == 'stall' else None,
        'capacity': 15 if kind == 'stall' else None, 'seats': 3 if kind == 'bench' else None,
        'footprint_m2': 3.0 if kind == 'stall' else None, 'sense': None,
        'rent_krw_month': None, 'staff_count': None, 'wage_krw_month': None,
        'revenue_krw_month': None, 'source': 'scenario', 'label': 'assumption',
        'assumed': ['scenario_location', 'service_rate_per_min', 'capacity'] if kind == 'stall' else ['scenario_location', 'seats'],
        'store_ref': None,
    })
    for name in ('food', 'beauty', 'fashion', 'souvenir', 'culture', 'faith', 'nature'):
        row[f'offer_{name}'] = .6 if kind == 'stall' and ((index % 4 == 0 and name == 'souvenir') or (index % 4 != 0 and name == 'food')) else 0.0
    return row


def _change_income(base: Path, dest: Path, factor: float, source: dict[str, Any]) -> None:
    for path in (PERSONAS, SPEC):
        relative = str(path.relative_to(Path(__file__).resolve().parent.parent))
        if sha256(path) != source['source_sha256'][relative]:
            raise ValueError(f'소득 원본 또는 사양 해시가 변경됐습니다: {relative}')
    profile_path, attrs_path = dest / 'input/profiles.parquet', dest / 'input/profile_attrs.parquet'
    prof_table, attrs_table = pq.read_table(profile_path), pq.read_table(attrs_path)
    profiles, attrs = prof_table.to_pylist(), attrs_table.to_pylist()
    origin = {r['synthetic_id']: r for r in pq.read_table(PERSONAS, columns=['synthetic_id', 'age', 'sex', 'occupation']).to_pylist()}
    attr_by_id = {a['persona_id']: a for a in attrs}
    spec = income_spec()
    ref, budget, sens = spec['price_sensitivity']['ref_income_krw'], spec['budget'], spec['price_sensitivity']
    if len(attr_by_id) != len(profiles):
        raise ValueError('소득 속성과 프로필이 일치하지 않습니다')
    for prof in profiles:
        attr = attr_by_id[prof['persona_id']]
        row = origin.get(attr['source_ref'])
        if row is None:
            raise ValueError('소득 계산에 필요한 인물이 없습니다')
        income, _, group = estimate_income(row, spec)
        if income != attr['income_krw']:
            raise ValueError('기준 소득과 원본 계산이 다릅니다')
        changed = income * factor
        if group:
            prof['cap_money_comfort'] = float(budget['non_earner'][group]['comfort'])
            prof['cap_money_limit'] = float(budget['non_earner'][group]['limit'])
        else:
            prof['cap_money_comfort'] = max(budget['floor_krw']['comfort'], budget['comfort_ref_krw'] * (changed / ref) ** budget['income_exponent']['comfort'])
            prof['cap_money_limit'] = max(budget['floor_krw']['limit'], budget['limit_ref_krw'] * (changed / ref) ** budget['income_exponent']['limit'])
        prof['sens_money'] = max(sens['min'], min(sens['max'], (ref / changed) ** sens['exponent']))
        attr['income_krw'] = round(changed)
    _write_table(profile_path, prof_table, profiles)
    _write_table(attrs_path, attrs_table, attrs)


def prepare_input(base: Path, dest: Path, options: dict[str, Any]) -> Path:
    options = validate_options(options)
    if not base.is_dir() or dest.exists():
        raise ValueError('기준 입력이 없거나 출력 폴더가 이미 존재합니다')
    if dest.resolve().is_relative_to(base.resolve()):
        raise ValueError('출력 폴더는 기준 입력 바깥에 있어야 합니다')
    shutil.copytree(base, dest)
    source_path = dest / 'source.json'
    source = json.loads(source_path.read_text())
    scenario_path = dest / 'bundle/scenario.yaml'
    scenario = yaml.safe_load(scenario_path.read_text())
    if scenario['window'] != {'start': '14:00', 'end': '17:00'}:
        raise ValueError('이 실행은 14:00~17:00 입력만 지원합니다')
    hours = options['pedestrian_hours']
    scenario['pedestrian_only'] = [] if hours == 0 else [{'start': '12:00', 'end': '16:00' if hours == 4 else '20:00'}]
    scenario['static_applied'] = {
        'v02_width_delta_m': options['width_delta_m'], 'v03_stall_scale': options['stall_scale'],
        'v04_added_benches': options['bench_count'], 'v05_discount_pct': options['discount_pct'],
        'v06_hours_extension_h': options['hours_extension'], 'v12_income_multiplier': options['income_multiplier'],
        'v15_price_scale': options['price_multiplier'], 'v16_visibility_level': options['visibility_level'],
    }
    scenario_path.write_text(yaml.safe_dump(scenario, allow_unicode=True, sort_keys=False))

    edges_path, places_path = dest / 'input/edges.parquet', dest / 'input/places.parquet'
    edges_table, places_table = pq.read_table(edges_path), pq.read_table(places_path)
    edges, places = edges_table.to_pylist(), places_table.to_pylist()
    if not edges or not places:
        raise ValueError('보행 간선 또는 대리 상점이 없습니다')
    delta = {'low': -.2, 'base': 0., 'high': .2}[options['visibility_level']]
    for edge in edges:
        if edge['inside_boundary'] and (edge['ped_zone'] or edge['road_type'] == 'pedestrian'):
            col = 'sidewalk_width_m' if edge['walk_mode'] == 'sidewalks' else 'width_m'
            edge[col] += options['width_delta_m']
        if edge['inside_boundary']:
            edge['ad_intensity'] = max(0.0, min(1.0, edge['ad_intensity'] + delta))
    _write_table(edges_path, edges_table, edges)
    targets = sorted((e for e in edges if e['inside_boundary'] and e['ped_zone']), key=lambda e:e['edge_id'])
    total = sum(e['length_m'] for e in targets)
    if not targets or total <= 0:
        raise ValueError('노점·벤치를 배치할 대상 보행 간선이 없습니다')
    count = round(40 * options['stall_scale'])
    template = places[0]
    for index in range(1, count + 1):
        edge, offset, x, z = _point_on_edges(targets, index * total / (count + 1))
        places.append(_facility(template, edge, offset, x, z, index, 'stall'))
    for index in range(1, options['bench_count'] + 1):
        edge, offset, x, z = _point_on_edges(targets, 15.0 + 30.0 * (index - 1))
        places.append(_facility(template, edge, offset, x, z, index, 'bench'))
    from myeongdong_buildings import apply_types, add_medical_definition
    places, medical, glb_assumed = apply_types(base, places, options['building_overrides'])
    for place in places:
        if place['class'] in ('store', 'cafe', 'stall', 'medical'):
            override = options['building_overrides'].get(place.get('store_ref')) or {}
            discount = override.get('discount_pct') if override.get('discount_pct') is not None else options['discount_pct']
            price_scale = override.get('price_multiplier') if override.get('price_multiplier') is not None else options['price_multiplier']
            hours_extension = override.get('hours_extension') if override.get('hours_extension') is not None else options['hours_extension']
            place['discount_pct'] = float(discount) if place['class'] != 'medical' else 0.0
            place['price_level_krw'] = float(place['price_level_krw']) * price_scale
            place['close_min'] = min(2880, place['close_min'] + 60 * hours_extension)
            place['visibility'] = max(0.0, min(1.0, place['visibility'] + delta))
    _write_table(places_path, places_table, places)
    if options['income_multiplier'] != 1:
        _change_income(base, dest, options['income_multiplier'], source)

    manifest_path = dest / 'bundle/manifest.json'
    manifest = json.loads(manifest_path.read_text())
    if medical:
        manifest['ontology_sha256'] = add_medical_definition(dest / 'defs')
        manifest['assumption_flags'].append('medical_generic_visit')
    if glb_assumed:
        manifest['assumption_flags'].append('glb_center_unverified_buildings')
    for role in ('edges', 'places', 'profiles', 'profile_attrs'):
        path = dest / manifest['files'][role][0]['path']
        manifest['files'][role][0]['sha256'] = sha256(path)
        manifest['files'][role][0]['rows'] = pq.read_metadata(path).num_rows
    manifest['scenarios'][0]['sha256'] = sha256(scenario_path)
    identity = manifest['bundle_id'] + ':' + manifest['ontology_sha256'] + ':' + ':'.join(manifest['files'][k][0]['sha256'] for k in ('edges','places','profiles','profile_attrs')) + ':' + manifest['scenarios'][0]['sha256']
    manifest['bundle_id'] = hashlib.sha256(identity.encode()).hexdigest()[:16]
    for flag in ('assumed_scenario_stalls', 'assumed_scenario_benches'):
        if flag not in manifest['assumption_flags']: manifest['assumption_flags'].append(flag)
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
    source['run_options'] = options
    source['glb_center_assumed_buildings'] = glb_assumed
    if medical: source['limitations'] = list(source['limitations']) + ['의료 시설은 일반 방문 대리값이며 진료 모형 없음']
    if glb_assumed: source['limitations'] = list(source['limitations']) + ['GLB 전용 건물의 외곽·장애물 정합 미검증']
    source['scenario_facilities'] = {'stalls': count, 'benches': options['bench_count'], 'location': 'assumed'}
    source['limitations'] = list(source['limitations']) + ['노점·벤치 배치는 가정']
    source_path.write_text(json.dumps(source, ensure_ascii=False, indent=2) + '\n')
    return dest
