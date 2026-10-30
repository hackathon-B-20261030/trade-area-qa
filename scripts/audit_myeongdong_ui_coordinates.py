"""Compare imported GLB building centers with the engine's source map.

The fitted transform is only accepted after independent footprint-vertex checks.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import statistics
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_GLB = ROOT / 'apps/myeongdong-ui/public/myeongdong.glb'
DEFAULT_MAP = ROOT / 'data/myeongdong-3d-map/map_data.json'


def load_glb(path: Path) -> tuple[dict, str]:
    data = path.read_bytes()
    if len(data) < 20:
        raise ValueError('GLB header is truncated')
    magic, version, length = struct.unpack_from('<4sII', data)
    size, kind = struct.unpack_from('<I4s', data, 12)
    if magic != b'glTF' or version != 2 or length != len(data) or kind != b'JSON' or size > len(data) - 20:
        raise ValueError('Invalid GLB header or JSON chunk')
    payload = json.loads(data[20:20 + size].decode('utf-8'))
    return payload, hashlib.sha256(data).hexdigest()


def model_centers(glb: dict) -> dict[str, tuple[float, float]]:
    nodes = glb['nodes']
    roots = set(glb['scenes'][glb.get('scene', 0)]['nodes'])
    result: dict[str, tuple[float, float]] = {}
    for index, node in enumerate(nodes):
        ident = node.get('extras', {}).get('id')
        if not ident:
            continue
        if ident in result:
            raise ValueError(f'Duplicate GLB building ID: {ident}')
        if index not in roots or any(key in node for key in ('matrix', 'translation', 'rotation', 'scale')):
            raise ValueError(f'Building node has an unsupported parent or transform: {ident}')
        primitives = glb['meshes'][node['mesh']]['primitives']
        accessors = [glb['accessors'][p['attributes']['POSITION']] for p in primitives]
        x_min = min(a['min'][0] for a in accessors)
        x_max = max(a['max'][0] for a in accessors)
        z_min = min(a['min'][2] for a in accessors)
        z_max = max(a['max'][2] for a in accessors)
        point = ((x_min + x_max) / 2, (z_min + z_max) / 2)
        if not all(math.isfinite(v) for v in point):
            raise ValueError(f'Non-finite GLB center: {ident}')
        result[ident] = point
    return result


def map_centers(data: dict) -> dict[str, tuple[float, float]]:
    result: dict[str, tuple[float, float]] = {}
    for building in data['buildings']:
        ident = building['id']
        if ident in result:
            raise ValueError(f'Duplicate map building ID: {ident}')
        points = building['footprint']
        x_values = [point[0] for point in points]
        z_values = [point[1] for point in points]
        center = ((min(x_values) + max(x_values)) / 2, (min(z_values) + max(z_values)) / 2)
        if not all(math.isfinite(v) for v in center):
            raise ValueError(f'Non-finite map center: {ident}')
        result[ident] = center
    return result


def fit(source: list[float], target: list[float]) -> tuple[float, float]:
    x_mean, y_mean = statistics.mean(source), statistics.mean(target)
    denominator = sum((x - x_mean) ** 2 for x in source)
    if denominator == 0:
        raise ValueError('Cannot fit a coordinate axis with zero variance')
    slope = sum((x - x_mean) * (y - y_mean) for x, y in zip(source, target)) / denominator
    return slope, y_mean - slope * x_mean


def _boundary_distance(point: tuple[float, float], ring: list) -> float:
    x, z = point
    best = math.inf
    for (x0, z0), (x1, z1) in zip(ring, ring[1:] + ring[:1]):
        dx, dz = x1 - x0, z1 - z0
        squared = dx * dx + dz * dz
        t = max(0.0, min(1.0, ((x - x0) * dx + (z - z0) * dz) / squared)) if squared else 0.0
        best = min(best, math.hypot(x - x0 - t * dx, z - z0 - t * dz))
    return best


def vertex_residuals(path: Path, glb: dict, buildings: dict, x_fit: tuple[float, float], z_fit: tuple[float, float]) -> list[float]:
    data = path.read_bytes()
    json_size = struct.unpack_from('<I', data, 12)[0]
    binary_header = 20 + json_size
    if binary_header + 8 > len(data):
        raise ValueError('Missing GLB binary chunk')
    binary_size, kind = struct.unpack_from('<I4s', data, binary_header)
    start = binary_header + 8
    if kind != b'BIN\0' or start + binary_size > len(data):
        raise ValueError('Invalid GLB binary chunk')
    sx, ox = x_fit
    sz, oz = z_fit
    if not all(math.isfinite(x) for x in (sx, ox, sz, oz)) or abs(sx) < 1e-9 or abs(sz) < 1e-9:
        raise ValueError('Invalid coordinate transform')
    residuals = []
    for node in glb['nodes']:
        ident = node.get('extras', {}).get('id')
        if ident not in buildings:
            continue
        ring = buildings[ident]['footprint']
        if len(ring) < 3:
            raise ValueError(f'Invalid footprint: {ident}')
        points = set()
        for primitive in glb['meshes'][node['mesh']]['primitives']:
            acc = glb['accessors'][primitive['attributes']['POSITION']]
            if acc.get('componentType') != 5126 or acc.get('type') != 'VEC3' or 'sparse' in acc:
                raise ValueError(f'Unsupported GLB position accessor: {ident}')
            view = glb['bufferViews'][acc['bufferView']]
            if view.get('buffer', 0) != 0:
                raise ValueError(f'Unsupported GLB buffer: {ident}')
            stride = view.get('byteStride', 12)
            offset = view.get('byteOffset', 0) + acc.get('byteOffset', 0)
            count = acc['count']
            if stride < 12 or count < 0 or offset < 0 or (count and offset + (count - 1) * stride + 12 > binary_size):
                raise ValueError(f'Truncated GLB position accessor: {ident}')
            for index in range(count):
                x, _, z = struct.unpack_from('<fff', data, start + offset + index * stride)
                if not math.isfinite(x) or not math.isfinite(z):
                    raise ValueError(f'Non-finite GLB vertex: {ident}')
                points.add(((x - ox) / sx, (z - oz) / sz))
        residuals.extend(_boundary_distance(point, ring) for point in points)
    if not residuals:
        raise ValueError('No common footprint vertices')
    return sorted(residuals)


def audit(glb_path: Path, map_path: Path) -> dict:
    glb, glb_hash = load_glb(glb_path)
    map_bytes = map_path.read_bytes()
    source_data = json.loads(map_bytes)
    model, source = model_centers(glb), map_centers(source_data)
    ids = sorted(model.keys() & source.keys())
    if len(ids) < 3:
        raise ValueError('At least three common building IDs are required')
    x_slope, x_offset = fit([source[i][0] for i in ids], [model[i][0] for i in ids])
    z_slope, z_offset = fit([source[i][1] for i in ids], [model[i][1] for i in ids])
    identity = [math.dist(source[i], model[i]) for i in ids]
    residuals = sorted(math.hypot(
        x_slope * source[i][0] + x_offset - model[i][0],
        z_slope * source[i][1] + z_offset - model[i][1],
    ) for i in ids)
    p95_index = math.ceil(0.95 * len(residuals)) - 1
    buildings = {v['id']: v for v in source_data['buildings'] if v['id'] in ids}
    vertex_errors = vertex_residuals(glb_path, glb, buildings, (x_slope, x_offset), (z_slope, z_offset))
    if vertex_errors[-1] > 0.2:
        raise ValueError(f'GLB footprint alignment exceeds 0.2m: {vertex_errors[-1]:.3f}m')
    return {
        'glb_sha256': glb_hash,
        'map_sha256': hashlib.sha256(map_bytes).hexdigest(),
        'glb_building_ids': len(model),
        'map_building_ids': len(source),
        'shared_building_ids': len(ids),
        'glb_only_ids': len(model.keys() - source.keys()),
        'map_only_ids': len(source.keys() - model.keys()),
        'identity_median_distance_m': statistics.median(identity),
        'center_fit_model_x': {'slope': x_slope, 'offset_m': x_offset},
        'center_fit_model_z': {'slope': z_slope, 'offset_m': z_offset},
        'center_fit_residual_m': {'median': statistics.median(residuals), 'p95': residuals[p95_index], 'max': residuals[-1]},
        'vertices_count': len(vertex_errors),
        'vertices_p95_m': vertex_errors[math.ceil(0.95 * len(vertex_errors)) - 1],
        'vertices_max_m': vertex_errors[-1],
        'limitation': 'Building alignment only; GLB ground is flat and sidewalk geometry and outside the common area are not validated.',
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--glb', type=Path, default=DEFAULT_GLB)
    parser.add_argument('--map', type=Path, default=DEFAULT_MAP)
    args = parser.parse_args()
    try:
        print(json.dumps(audit(args.glb, args.map), ensure_ascii=False, indent=2))
    except (OSError, ValueError, KeyError, IndexError, TypeError, struct.error) as exc:
        print(f'Coordinate audit failed: {exc}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
