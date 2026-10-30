"""Build a reproducible 3D map from the supplied buildings and finalized terrain."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import rasterio
from rasterio.windows import Window
import shapely
from shapely.geometry import LineString, Polygon, box
from shapely.strtree import STRtree

from download_myeongdong_terrain import TERRAIN_ETAG, TERRAIN_URL, check_source, validate_source
from finalize_myeongdong_terrain import gaussian_smooth

ROOT = Path(__file__).resolve().parents[1]
FINAL_DIR = ROOT / "data" / "myeongdong-terrain" / "final"
MAP_DIR = ROOT / "data" / "myeongdong-3d-map"
INPUT_DIR = Path("/Users/aiden/Downloads/myeongdong-3d")
BUILDING_HASH = "e895d86244c3e0cb137e064eae9abd28885b7d3de522addfab63ee75872e0660"
OSM_HASH = "bd7291f741d6ef0feafd39f0632a7f6d6890131197d08a539f56b34c46b25b83"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def blend_extended(raw_with_halo: np.ma.MaskedArray, final: np.ma.MaskedArray,
                   radius: int = 4) -> np.ma.MaskedArray:
    raw = np.ma.asarray(raw_with_halo)
    fixed = np.ma.asarray(final)
    if (raw.ndim != 2 or fixed.ndim != 2 or radius != 4
            or raw.shape != (fixed.shape[0] + 2 * radius, fixed.shape[1] + 2 * radius)):
        raise ValueError("The source window must have a four-cell halo")
    if np.ma.getmaskarray(raw).any() or not np.isfinite(raw.data).all():
        raise ValueError("The additional source window contains missing or non-finite elevations")
    fixed_mask = np.ma.getmaskarray(fixed)
    valid = ~fixed_mask
    if not valid.any() or not np.isfinite(fixed.data[valid]).all():
        raise ValueError("The finalized terrain contains no finite valid elevations")
    combined = np.ma.array(raw.data.astype(np.float64, copy=True), mask=np.zeros(raw.shape, dtype=bool))
    center = combined.data[radius:-radius, radius:-radius]
    center[valid] = fixed.data[valid]
    result = gaussian_smooth(combined, sigma=1.0, radius=radius)[radius:-radius, radius:-radius].copy()
    result.data[valid] = fixed.data[valid]
    result.mask = np.zeros(fixed.shape, dtype=bool)
    return result


def extend_terrain(final_dir: Path, output_dir: Path) -> None:
    final_dir, output_dir = Path(final_dir), Path(output_dir)
    if output_dir.exists():
        raise FileExistsError(f"Map output already exists: {output_dir}")
    final_meta = json.loads((final_dir / "metadata.json").read_text(encoding="utf-8"))
    raster_meta = final_meta["raster"]
    final_path = final_dir / raster_meta["file"]
    final_hash = sha256_file(final_path)
    if final_hash != raster_meta["sha256"] or final_meta["status"] != "finalized":
        raise ValueError("Finalized terrain hash or status does not match metadata")
    with rasterio.open(final_path) as saved:
        fixed = saved.read(1, masked=True)
        if (saved.crs is None or saved.crs.to_epsg() != 4326 or (saved.width, saved.height) != (57, 57)
                or fixed.count() != 1462 or list(tuple(saved.transform)[:6]) != raster_meta["transform"]
                or saved.nodata != raster_meta["nodata"]):
            raise ValueError("Finalized terrain grid does not match its metadata")
        transform, crs, nodata = saved.transform, saved.crs, saved.nodata
    check_source(TERRAIN_URL, TERRAIN_ETAG)
    with rasterio.Env(GDAL_DISABLE_READDIR_ON_OPEN="EMPTY_DIR", GDAL_HTTP_TIMEOUT="60", GDAL_HTTP_MAX_RETRY="1"):
        with rasterio.open("/vsicurl/" + TERRAIN_URL) as source:
            validate_source(source)
            col = (transform.c - source.transform.c) / source.transform.a
            row = (transform.f - source.transform.f) / source.transform.e
            if abs(col - round(col)) > 1e-6 or abs(row - round(row)) > 1e-6:
                raise ValueError("Finalized terrain does not align with the source pixel grid")
            row, col = round(row), round(col)
            if (row < 4 or col < 4 or row + 57 + 4 > source.height or col + 57 + 4 > source.width):
                raise ValueError("Source cannot cover the complete terrain and halo")
            raw = source.read(1, window=Window(col - 4, row - 4, 65, 65), masked=True)
    result = blend_extended(raw, fixed)
    check_source(TERRAIN_URL, TERRAIN_ETAG)
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".myeongdong-map-", dir=output_dir.parent) as temporary:
        stage = Path(temporary)
        path = stage / "terrain_extended.tif"
        with rasterio.open(path, "w", driver="GTiff", width=57, height=57, count=1,
                           dtype="float32", crs=crs, transform=transform, nodata=nodata,
                           compress="deflate", predictor=3) as target:
            target.write(result.filled(nodata), 1)
            target.update_tags(UNIT="metre", VERTICAL_DATUM="EGM2008 (EPSG:3855)",
                               SOURCE="GEDTM30 v1.2", PROCESSING="fixed interior; sigma=1 cell, radius=4")
        with rasterio.open(path) as verified:
            reread = verified.read(1, masked=True)
            if (verified.crs != crs or verified.transform != transform or verified.nodata != nodata
                    or reread.count() != 3249 or not np.array_equal(reread.data, result.data)
                    or not np.array_equal(reread.data[~np.ma.getmaskarray(fixed)], fixed.data[~np.ma.getmaskarray(fixed)])):
                raise ValueError("Extended terrain failed the re-read or preservation check")
        if sha256_file(final_path) != final_hash:
            raise ValueError("Finalized terrain changed during the extension")
        metadata = {
            "status": "verified", "source_url": TERRAIN_URL, "source_etag": TERRAIN_ETAG,
            "source_license": "CC BY 4.0", "source_period": "2006-01-01/2015-12-31",
            "final_source_sha256": final_hash, "file": path.name, "sha256": sha256_file(path),
            "crs": crs.to_string(), "transform": [float(v) for v in tuple(transform)[:6]],
            "width": 57, "height": 57, "valid_cells": int(reread.count()),
            "preserved_cells": int(fixed.count()), "added_cells": int(reread.count() - fixed.count()),
            "min_m": float(reread.min()), "max_m": float(reread.max()),
            "sigma_cells": 1.0, "radius_cells": 4,
            "outside_method": "Gaussian-smoothed original and fixed interior; original finalized interior restored exactly",
            "vertical_datum": final_meta["vertical_datum"], "unit": "metre",
        }
        (stage / "terrain_metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        if json.loads((stage / "terrain_metadata.json").read_text(encoding="utf-8")) != metadata:
            raise ValueError("Extended terrain metadata could not be verified")
        if output_dir.exists():
            raise FileExistsError(f"Map output already exists: {output_dir}")
        os.rename(stage, output_dir)


def to_local(lon: float, lat: float, center: dict) -> tuple[float, float]:
    longitude_scale = 111320.0 * math.cos(math.radians(center["lat"]))
    return (lon - center["lon"]) * longitude_scale, -(lat - center["lat"]) * 111132.0


@dataclass
class Surface:
    vertices: list[tuple[float, float, float]]
    triangles: list[tuple[int, int, int]]
    polygons: list[Polygon]
    planes: list[tuple[float, float, float]]
    index: STRtree
    bounds: Polygon


def build_surface(raster: np.ndarray, transform, center: dict) -> Surface:
    raster = np.ma.asarray(raster)
    if (raster.ndim != 2 or not raster.size or np.ma.getmaskarray(raster).any()
            or not np.isfinite(raster.data).all()):
        raise ValueError("Surface raster must be a finite, complete 2D grid")
    values = raster.data.astype(np.float64)
    height, width = values.shape
    sums = np.zeros((height + 1, width + 1), dtype=np.float64)
    counts = np.zeros_like(sums)
    for dr, dc in ((0, 0), (0, 1), (1, 0), (1, 1)):
        sums[dr:dr + height, dc:dc + width] += values
        counts[dr:dr + height, dc:dc + width] += 1
    corner_values = sums / counts
    vertices: list[tuple[float, float, float]] = []

    def vertex(column: float, row: float, altitude: float) -> tuple[float, float, float]:
        lon, lat = transform * (column, row)
        x, z = to_local(lon, lat, center)
        return (x, float(altitude), z)

    for row in range(height + 1):
        for col in range(width + 1):
            vertices.append(vertex(col, row, corner_values[row, col]))
    corners = len(vertices)
    for row in range(height):
        for col in range(width):
            vertices.append(vertex(col + 0.5, row + 0.5, values[row, col]))
    triangles: list[tuple[int, int, int]] = []
    for row in range(height):
        for col in range(width):
            center_index = corners + row * width + col
            corner_indices = (row * (width + 1) + col, row * (width + 1) + col + 1,
                              (row + 1) * (width + 1) + col + 1, (row + 1) * (width + 1) + col)
            for edge in range(4):
                triangles.append((center_index, corner_indices[(edge + 1) % 4], corner_indices[edge]))
    polygons: list[Polygon] = []
    planes: list[tuple[float, float, float]] = []
    for a, b, c in triangles:
        x0, y0, z0 = vertices[a]
        x1, y1, z1 = vertices[b]
        x2, y2, z2 = vertices[c]
        det = (x1 - x0) * (z2 - z0) - (x2 - x0) * (z1 - z0)
        if abs(det) < 1e-12:
            raise ValueError("Terrain contains a degenerate triangle")
        px = ((y1 - y0) * (z2 - z0) - (y2 - y0) * (z1 - z0)) / det
        pz = ((x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)) / det
        planes.append((px, pz, y0 - px * x0 - pz * z0))
        polygons.append(Polygon([(x0, z0), (x1, z1), (x2, z2)]))
    outer_x = [vertices[i][0] for i in (0, width, height * (width + 1), (height + 1) * (width + 1) - 1)]
    outer_z = [vertices[i][2] for i in (0, width, height * (width + 1), (height + 1) * (width + 1) - 1)]
    return Surface(vertices, triangles, polygons, planes, STRtree(polygons),
                   box(min(outer_x), min(outer_z), max(outer_x), max(outer_z)))


def reconstruct_buildings(buildings: list[dict], osm: dict, center: dict) -> list[dict]:
    nodes = {item["id"]: item for item in osm["elements"] if item["type"] == "node"}
    ways = {item["id"]: item for item in osm["elements"] if item["type"] == "way"}
    relations = {item["id"]: item for item in osm["elements"] if item["type"] == "relation"}
    relation = relations.get(8814915)
    if relation is None:
        raise ValueError("The IBK building relation is missing")
    outer = [m["ref"] for m in relation["members"] if m.get("type") == "way" and m.get("role") == "outer"]
    if outer != [634096161, 634096168]:
        raise ValueError("The IBK building relation has unexpected outer ways")
    parts = [item for item in buildings if item["id"] in outer]
    if len(parts) != 2 or any(parts[0][key] != parts[1][key] for key in ("name", "height", "minHeight")):
        raise ValueError("IBK building parts do not have matching attributes")
    nodes_a, nodes_b = ways[outer[0]]["nodes"], ways[outer[1]]["nodes"]
    if nodes_a[-1] != nodes_b[0] or nodes_b[-1] != nodes_a[0]:
        raise ValueError("The IBK building's outer ways do not form one closed ring")
    node_ids = nodes_a + nodes_b[1:]
    try:
        coords = [list(to_local(nodes[n]["lon"], nodes[n]["lat"], center)) for n in node_ids]
    except KeyError as exc:
        raise ValueError("An IBK building node is missing") from exc
    poly = Polygon(coords)
    if not poly.is_valid or poly.is_empty or poly.area <= 0:
        raise ValueError("The reconstructed IBK building polygon is invalid")
    repaired = dict(parts[0])
    repaired.update(id="relation/8814915", source_ids=["way/634096161", "way/634096168"],
                    footprint=coords[:-1])
    duplicate = [item for item in buildings if item["id"] == 772877690]
    if len(duplicate) != 4 or any(
        any(item[key] != duplicate[0][key] for key in ("footprint", "height", "minHeight"))
        for item in duplicate[1:]
    ):
        raise ValueError("Repeated building outlines differ in geometry or height")
    seen_duplicate = False
    result = []
    for item in buildings:
        if item["id"] in outer:
            if item["id"] == outer[0]:
                result.append(repaired)
            continue
        if item["id"] == 772877690:
            if seen_duplicate:
                continue
            seen_duplicate = True
            keep = dict(item)
            keep.update(id="way/772877690", source_ids=["way/772877690"] * 4)
            result.append(keep)
            continue
        keep = dict(item)
        keep.update(id=f"way/{item['id']}", source_ids=[f"way/{item['id']}"])
        result.append(keep)
    if len(result) != len(buildings) - 4:
        raise ValueError("Unexpected number of buildings after source-backed repairs")
    return result


def _plane_height(surface: Surface, index: int, x: float, z: float) -> float:
    a, b, c = surface.planes[index]
    return a * x + b * z + c


def terrain_range_for_polygon(geometry, surface: Surface) -> tuple[float, float]:
    if geometry.is_empty or not geometry.is_valid or geometry.area <= 0 or not surface.bounds.covers(geometry):
        raise ValueError("A building footprint is invalid or outside the terrain")
    minimum, maximum, covered = float("inf"), float("-inf"), 0.0
    for index_value in surface.index.query(geometry, predicate="intersects"):
        index = int(index_value)
        intersection = geometry.intersection(surface.polygons[index])
        if intersection.is_empty or intersection.area <= 0:
            continue
        covered += intersection.area
        for x, z in shapely.get_coordinates(intersection):
            elevation = _plane_height(surface, index, float(x), float(z))
            minimum, maximum = min(minimum, elevation), max(maximum, elevation)
    if (not math.isfinite(minimum) or abs(covered - geometry.area) > max(1e-6, geometry.area * 1e-6)):
        raise ValueError("A building footprint is not fully covered by terrain triangles")
    return minimum, maximum


def _line_parts(geometry):
    if geometry.is_empty:
        return
    if geometry.geom_type in ("LineString", "LinearRing"):
        yield geometry
    elif hasattr(geometry, "geoms"):
        for part in geometry.geoms:
            yield from _line_parts(part)


def project_roads(roads: list[dict], surface: Surface) -> list[dict]:
    output = []
    for road in roads:
        if len(road.get("path", [])) < 2:
            raise ValueError("Road has fewer than two coordinates")
        line = LineString(road["path"]).intersection(surface.bounds)
        segments = []
        for piece in _line_parts(line):
            for index_value in surface.index.query(piece, predicate="intersects"):
                index = int(index_value)
                for part in _line_parts(piece.intersection(surface.polygons[index])):
                    coords = list(part.coords)
                    for first, second in zip(coords, coords[1:]):
                        if math.dist(first, second) < 1e-7:
                            continue
                        segment = []
                        for x, z in (first, second):
                            height = _plane_height(surface, index, x, z) + 0.25
                            segment.append([round(float(x), 3), round(float(height), 3), round(float(z), 3)])
                        segments.append(segment)
        if not segments:
            raise ValueError(f"Road has no portion on the terrain: {road['id']}")
        output.append({"id": str(road["id"]), "name": str(road.get("name") or ""), "segments": segments})
    return output


def prepare_map(input_dir: Path, output_dir: Path) -> None:
    input_dir, output_dir = Path(input_dir), Path(output_dir)
    planned = [output_dir / "map_data.json", output_dir / "map_data.js", output_dir / "source"]
    if any(path.exists() for path in planned):
        raise FileExistsError("Map data or copied sources already exist")
    input_path, osm_path = input_dir / "myeongdong_data.json", input_dir / "osm_raw.json"
    if sha256_file(input_path) != BUILDING_HASH or sha256_file(osm_path) != OSM_HASH:
        raise ValueError("A supplied building source differs from the inspected version")
    data = json.loads(input_path.read_text(encoding="utf-8-sig"))
    osm = json.loads(osm_path.read_text(encoding="utf-8-sig"))
    if (data["center"] != {"lat": 37.563, "lon": 126.98525}
            or len(data["buildings"]) != 713 or len(data["roads"]) != 347):
        raise ValueError("The provided map data has unexpected dimensions or origin")
    terrain_meta = json.loads((output_dir / "terrain_metadata.json").read_text(encoding="utf-8"))
    terrain_path = output_dir / terrain_meta["file"]
    if sha256_file(terrain_path) != terrain_meta["sha256"]:
        raise ValueError("Extended terrain hash does not match its metadata")
    with rasterio.open(terrain_path) as source:
        raster = source.read(1, masked=True)
        if raster.shape != (57, 57) or raster.count() != 3249 or source.crs.to_epsg() != 4326:
            raise ValueError("Extended terrain has an unexpected grid")
        transform = source.transform
        surface = build_surface(raster, transform, data["center"])
    buildings = reconstruct_buildings(data["buildings"], osm, data["center"])
    output_buildings = []
    for building in buildings:
        polygon = Polygon(building["footprint"])
        height, minimum = float(building["height"]), float(building["minHeight"])
        if not math.isfinite(height) or not math.isfinite(minimum) or height <= minimum or minimum < 0:
            raise ValueError(f"Invalid building height: {building['id']}")
        ground_min, ground_max = terrain_range_for_polygon(polygon, surface)
        output_buildings.append({
            "id": str(building["id"]), "source_ids": building["source_ids"],
            "name": str(building.get("name") or ""), "height": height, "minHeight": minimum,
            "base": ground_min, "ground_max": ground_max,
            "footprint": [[float(x), float(z)] for x, z in building["footprint"]],
        })
    roads = project_roads(data["roads"], surface)
    result = {
        "metadata": {
            "building_input_records": 713, "building_objects": len(output_buildings),
            "road_input_records": 347, "road_objects": len(roads), "center": data["center"],
            "building_source_sha256": BUILDING_HASH, "osm_source_sha256": OSM_HASH,
            "terrain_sha256": terrain_meta["sha256"], "terrain_source": "GEDTM30 v1.2 / CC BY 4.0",
            "map_source": "© OpenStreetMap contributors / ODbL",
            "repairs": {"joined_relation": 8814915, "joined_ways": [634096161, 634096168],
                        "repeated_outer_way": 772877690, "duplicate_records_removed": 3},
            "road_model": "source centerlines clipped to terrain and projected onto ground; tunnels are not underground",
        },
        "terrain": {"width": 57, "height": 57, "transform": [float(v) for v in tuple(transform)[:6]],
                    "values": raster.data.astype(float).tolist()},
        "buildings": output_buildings,
        "roads": roads,
    }
    if len(output_buildings) != 709 or len(roads) != 347:
        raise ValueError("Map output feature counts are unexpected")
    with tempfile.TemporaryDirectory(prefix=".prepared-", dir=output_dir) as temporary:
        stage = Path(temporary)
        encoded = json.dumps(result, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        (stage / "map_data.json").write_text(encoded + "\n", encoding="utf-8")
        safe_js = encoded.replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")
        (stage / "map_data.js").write_text("window.MYEONGDONG_MAP_DATA = " + safe_js + ";\n", encoding="utf-8")
        sources = stage / "source"
        sources.mkdir()
        shutil.copyfile(input_path, sources / input_path.name)
        shutil.copyfile(osm_path, sources / osm_path.name)
        if (json.loads((stage / "map_data.json").read_text(encoding="utf-8")) != result
                or sha256_file(sources / input_path.name) != BUILDING_HASH
                or sha256_file(sources / osm_path.name) != OSM_HASH):
            raise ValueError("Prepared map data or source copies failed verification")
        if any(path.exists() for path in planned):
            raise FileExistsError("Map data or copied sources already exist")
        for name in ("map_data.json", "map_data.js", "source"):
            os.rename(stage / name, output_dir / name)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["extend", "prepare"])
    parser.add_argument("--final-dir", type=Path, default=FINAL_DIR)
    parser.add_argument("--output-dir", type=Path, default=MAP_DIR)
    parser.add_argument("--input-dir", type=Path, default=INPUT_DIR)
    args = parser.parse_args()
    if args.command == "extend":
        extend_terrain(args.final_dir, args.output_dir)
        print(f"Saved verified extended terrain to {args.output_dir}")
    else:
        prepare_map(args.input_dir, args.output_dir)
        print(f"Saved aligned map data to {args.output_dir}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, rasterio.errors.RasterioError) as exc:
        print(f"3D map build failed: {exc}", file=sys.stderr)
        sys.exit(1)
