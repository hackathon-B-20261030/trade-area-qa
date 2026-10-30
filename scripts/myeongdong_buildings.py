"""Resolve visible building IDs to explicit, assumed engine place inputs."""
from __future__ import annotations

from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path
from typing import Any

import pyarrow.parquet as pq
import yaml

import audit_myeongdong_ui_coordinates as audit
from prepare_myeongdong_5044 import OFFER, project

ROOT = Path(__file__).resolve().parent.parent
BUSINESSES = ROOT / 'apps/myeongdong-ui/src/data/building-business-data.json'
TYPES = ('original', 'cafe', 'restaurant', 'fashion', 'convenience', 'medical')


@lru_cache(maxsize=4)
def building_catalog(base_name: str) -> dict[str, dict[str, Any]]:
    base = Path(base_name)
    source = json.loads((base / 'source.json').read_text())
    glb, glb_hash = audit.load_glb(audit.DEFAULT_GLB)
    align = source['alignment']
    if glb_hash != align['glb_sha256'] or align['vertices_max_m'] > .2:
        raise ValueError('건물 카탈로그와 지도 좌표 검증이 일치하지 않습니다')
    centers = audit.model_centers(glb)
    buildings = pq.read_table(base / 'input/buildings.parquet').to_pylist()
    by_id = {r['building_id'].removeprefix('B:'): r for r in buildings}
    original_stores = set(pq.read_table(base / 'input/places.parquet', columns=['store_ref']).column(0).to_pylist())
    edges = pq.read_table(base / 'input/edges.parquet').to_pylist()
    segments = []
    for edge in edges:
        points = edge['polyline']
        if len(points) != 2 or edge['length_m'] <= 0:
            raise ValueError('건물 연결에는 두 점의 보행 간선이 필요합니다')
        segments.append({'row': edge, 'coords': ((points[0]['x'], points[0]['z']), (points[1]['x'], points[1]['z']))})
    if not segments:
        raise ValueError('건물을 연결할 보행 간선이 없습니다')
    businesses = json.loads(BUSINESSES.read_text())
    sx, ox = align['center_fit_model_x']['slope'], align['center_fit_model_x']['offset_m']
    sz, oz = align['center_fit_model_z']['slope'], align['center_fit_model_z']['offset_m']
    if not all(math.isfinite(x) for x in (sx, ox, sz, oz)) or abs(sx) < 1e-9 or abs(sz) < 1e-9:
        raise ValueError('건물 좌표 변환이 올바르지 않습니다')
    result: dict[str, dict[str, Any]] = {}
    for ident, (gx, gz) in centers.items():
        building = by_id.get(ident)
        if building:
            fp = building['footprint']
            if not fp: raise ValueError(f'건물 외곽이 없습니다: {ident}')
            x, z = sum(p['x'] for p in fp) / len(fp), sum(p['z'] for p in fp) / len(fp)
            basis = 'reference_map'
        else:
            x, z = (gx - ox) / sx, (gz - oz) / sz
            basis = 'glb_center_assumed'
        edge = min(segments, key=lambda e: project((x, z), e)[0])
        dist, t, ax, az, cross = project((x, z), edge)
        er = edge['row']
        if er['walk_mode'] == 'sidewalks':
            (x0, z0), (x1, z1) = edge['coords']
            offset = er['width_m'] / 2 - er['sidewalk_width_m'] / 2
            sign = 1 if cross >= 0 else -1
            ax += sign * (-(z1 - z0) / er['length_m']) * offset
            az += sign * ((x1 - x0) / er['length_m']) * offset
        name = businesses.get(ident, {}).get('name') or (building or {}).get('name') or '이름 없는 건물'
        result[ident] = {'id': ident, 'name': name, 'basis': basis, 'eligible': dist <= 80.0,
                         'original_store': ident in original_stores,
                         'distance_m': dist, 'x': x, 'z': z, 'edge_id': er['edge_id'],
                         'offset_m': t * er['length_m'], 'anchor_x': ax, 'anchor_z': az,
                         'side': 'left' if cross >= 0 else 'right'}
    return result


def apply_types(base: Path, places: list[dict[str, Any]], overrides: dict[str, Any]) -> tuple[list[dict[str, Any]], bool, list[str]]:
    if not overrides:
        return places, False, []
    catalog = building_catalog(str(base.resolve()))
    by_id = {p['store_ref']: p for p in places if p.get('store_ref')}
    template = dict(places[0])
    medical = False
    assumed: list[str] = []
    for ident, override in overrides.items():
        entry = catalog.get(ident)
        if not entry or not entry['eligible']:
            raise ValueError(f'건물 {ident}은 80m 이내의 보행망에 연결할 수 없습니다')
        kind = override['type']
        row = by_id.get(ident)
        if row is None and kind == 'original':
            raise ValueError(f'기존 상점이 없는 건물 {ident}의 업종을 먼저 지정하세요')
        if row is None:
            row = dict(template)
            row.update({'place_id': 'S:' + ident, 'store_ref': ident, 'name': entry['name'],
                        'x': entry['x'], 'z': entry['z'], 'edge_id': entry['edge_id'],
                        'offset_m': entry['offset_m'], 'anchor_x': entry['anchor_x'],
                        'anchor_z': entry['anchor_z'], 'side': entry['side'],
                        'layer': 0, 'floor': 1, 'source': 'scenario', 'label': 'assumption',
                        'assumed': ['user_building_type', 'category_proxy', 'location_proxy'],
                        'rent_krw_month': None, 'staff_count': None, 'wage_krw_month': None,
                        'revenue_krw_month': None, 'seats': None, 'footprint_m2': None, 'sense': None,
                        'day_types': ['weekday','weekend','event_day'], 'foreign_label': False,
                        'visibility': 1.0, 'fame': .2})
            places.append(row)
            by_id[ident] = row
        if entry['basis'] == 'glb_center_assumed': assumed.append(ident)
        if kind != 'original':
            for name in ('food','beauty','fashion','souvenir','culture','faith','nature'):
                row[f'offer_{name}'] = 0.0
            if kind == 'medical':
                medical = True
                row.update({'class':'medical', 'category':'medical', 'price_level_krw':0.0,
                            'service_rate_per_min':None, 'capacity':None, 'open_min':480, 'close_min':1200})
            else:
                cls, category, interest, strength, price, rate, capacity, open_min, close_min = OFFER[kind]
                row.update({'class':cls, 'category':category, 'price_level_krw':price,
                            'service_rate_per_min':rate, 'capacity':capacity,
                            'open_min':open_min, 'close_min':close_min})
                row[f'offer_{interest}'] = strength
            row['assumed'] = list(dict.fromkeys((row.get('assumed') or []) + ['user_building_type','category_proxy']))
    return places, medical, assumed


def add_medical_definition(defs: Path) -> str:
    path = defs / 'classes.yaml'
    doc = yaml.safe_load(path.read_text())
    classes = doc['classes']
    place = next(c for c in classes if c['id'] == 'place')
    categories = place['attributes']['category']['values']
    if 'medical' not in categories: categories.append('medical')
    if not any(c['id'] == 'medical' for c in classes):
        classes.append({'id':'medical','name':'의료 시설 (일반 방문 대리)','parent':'place','affords':['GO_TO']})
    path.write_text(yaml.safe_dump(doc, allow_unicode=True, sort_keys=False))
    digest = hashlib.sha256()
    for item in sorted(defs.glob('*.yaml')): digest.update(item.read_bytes())
    return digest.hexdigest()
