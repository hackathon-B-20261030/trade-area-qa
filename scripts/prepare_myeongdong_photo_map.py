#!/usr/bin/env python3
"""Prepare traceable satellite and facade photographs for the existing map."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import shutil
import subprocess
from pathlib import Path

import numpy as np
import rasterio
from rasterio.enums import Resampling
from rasterio.transform import from_bounds
from rasterio.warp import reproject, transform_bounds
from rasterio.windows import Window, from_bounds as window_from_bounds


ROOT = Path(__file__).resolve().parents[1]
SOURCE_MAP = ROOT / "data/myeongdong-3d-map"
OUTPUT = ROOT / "data/myeongdong-3d-map-photo"
SCENE_ID = "S2B_52SCG_20261028_0_L2A"
SCENE_TIME = "2026-10-28T02:27:13.169Z"
SATELLITE_URL = (
    "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/"
    "52/S/CG/2026/10/S2B_52SCG_20261028_0_L2A/TCI.tif"
)
ATTRIBUTION = "Contains modified Copernicus Sentinel data (2026)"

PHOTO_SPECS = [
    {"id": "way/210335697", "filename": "cathedral.jpg", "cache_name": "cbu-facade-cathedral.jpg",
     "title": "Myeongdong Cathedral front.jpg", "author": "Tristan Surtel", "date": "2019-07-31",
     "license": "CC BY-SA 4.0", "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
     "source_page": "https://commons.wikimedia.org/wiki/File:Myeongdong_Cathedral_front.jpg",
     "download_url": "https://thumb.wikimedia.org/wikipedia/commons/thumb/f/fc/Myeongdong_Cathedral_front.jpg/1280px-Myeongdong_Cathedral_front.jpg?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=thumbnail",
     "edge_indices": [0, 2, 3], "camera_direction": [-0.7012115, -0.7129533],
     "uv_crop": [0.20, 0.60, 0.93, 0.90],
     "orientation_basis": "Photo GPS 37.563684, 126.986817; camera-facing edges.",
     "sha256": "837f4b8349083548a4d063589ddf8bc6407e42ec1b0570996770db44d8bdad3c"},
    {"id": "way/348489095", "filename": "museum.jpg", "cache_name": "cbu-facade-museum.jpg",
     "title": "Bank of Korea Museum 20240219.jpg", "author": "Sean Young (@assanges)", "date": "2024-02-19",
     "license": "CC BY 4.0", "license_url": "https://creativecommons.org/licenses/by/4.0/",
     "source_page": "https://commons.wikimedia.org/wiki/File:Bank_of_Korea_Museum_20240219.jpg",
     "download_url": "https://thumb.wikimedia.org/wikipedia/commons/thumb/1/1b/Bank_of_Korea_Museum_20240219.jpg/1280px-Bank_of_Korea_Museum_20240219.jpg?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=thumbnail",
     "edge_indices": [58, 59, 60, 65, 68], "camera_direction": [-0.5444694, 0.8387807],
     "uv_crop": [0.10, 0.42, 0.92, 0.76],
     "orientation_basis": "Photo GPS 37.560917, 126.979606; camera-facing edges.",
     "sha256": "ae7e445bd2d60d502cbc1ab256feeae107333f84afdf20abea497868ef98df51"},
    {"id": "way/775236382", "filename": "post.jpg", "cache_name": "cbu-facade-post.jpg",
     "title": "서울중앙우체국.jpg", "author": "KSH1015", "date": "2023-09-29",
     "license": "CC BY-SA 3.0", "license_url": "https://creativecommons.org/licenses/by-sa/3.0/",
     "source_page": "https://commons.wikimedia.org/wiki/File:%EC%84%9C%EC%9A%B8%EC%A4%91%EC%95%99%EC%9A%B0%EC%B2%B4%EA%B5%AD.jpg",
     "download_url": "https://thumb.wikimedia.org/wikipedia/commons/thumb/c/c3/%EC%84%9C%EC%9A%B8%EC%A4%91%EC%95%99%EC%9A%B0%EC%B2%B4%EA%B5%AD.jpg/1280px-%EC%84%9C%EC%9A%B8%EC%A4%91%EC%95%99%EC%9A%B0%EC%B2%B4%EA%B5%AD.jpg?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=thumbnail",
     "edge_indices": [14, 15], "camera_direction": [-1.0, 0.0], "uv_crop": [0.04, 0.09, 0.97, 0.71],
     "orientation_basis": "Approximate west-facing street side from footprint and Sogong-ro; no photo GPS.",
     "sha256": "7343d48b4a72f3701dd17e7e977ef3811f164ed0f9bdcf786ecbf676f3a60d31"},
    {"id": "way/296668636", "filename": "youngnak.jpg", "cache_name": "cbu-facade-church.jpg",
     "title": "Young Nak Church 2007.jpg", "author": "David Milanaik (Davidabram)", "date": "2007-09-09",
     "license": "Public domain", "license_url": None,
     "source_page": "https://commons.wikimedia.org/wiki/File:Young_Nak_Church_2007.jpg",
     "download_url": "https://thumb.wikimedia.org/wikipedia/commons/thumb/e/e8/Young_Nak_Church_2007.jpg/1280px-Young_Nak_Church_2007.jpg?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=thumbnail",
     "edge_indices": [6, 8, 25, 28], "camera_direction": [0.0, 1.0], "uv_crop": [0.19, 0.32, 0.99, 0.59],
     "orientation_basis": "Approximate south-facing courtyard side from church footprint; no photo GPS.",
     "sha256": "0ee8f2b12a5389af795b98fa3e2499d502a64139fd695b449e0d0fff9be90805"},
]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def map_bounds(data: dict) -> tuple[float, float, float, float]:
    grid = data["terrain"]
    a, b, c, d, e, f = grid["transform"]
    width, height = grid["width"], grid["height"]
    if not (all(math.isfinite(x) for x in (a, b, c, d, e, f))
            and a > 0 and e < 0 and b == 0 and d == 0 and width > 0 and height > 0):
        raise ValueError("지도 지형의 좌표계 또는 범위가 올바르지 않습니다.")
    return c, f + e * height, c + a * width, f


def validate_satellite_dataset(src, bounds: tuple[float, float, float, float]) -> Window:
    if src.crs is None or src.crs.to_epsg() != 32652:
        raise ValueError("위성영상의 좌표계가 예상과 다릅니다.")
    if src.count != 3 or tuple(src.dtypes) != ("uint8",) * 3:
        raise ValueError("위성영상 RGB 밴드가 예상과 다릅니다.")
    if any(abs(float(x) - 10) > 1e-7 for x in src.res):
        raise ValueError("위성영상의 10m 원본 해상도를 확인하지 못했습니다.")
    transformed = transform_bounds("EPSG:4326", src.crs, *bounds, densify_pts=21)
    left, bottom, right, top = transformed
    if not (src.bounds.left <= left < right <= src.bounds.right
            and src.bounds.bottom <= bottom < top <= src.bounds.top):
        raise ValueError("위성영상이 지도 전체를 덮지 않습니다.")
    window = window_from_bounds(*transformed, transform=src.transform)
    col0, row0 = math.floor(window.col_off) - 2, math.floor(window.row_off) - 2
    col1 = math.ceil(window.col_off + window.width) + 2
    row1 = math.ceil(window.row_off + window.height) + 2
    if col0 < 0 or row0 < 0 or col1 > src.width or row1 > src.height:
        raise ValueError("위성영상이 보간 여유를 포함한 범위를 덮지 않습니다.")
    return Window(col0, row0, col1 - col0, row1 - row0)


def reproject_satellite(rgb, valid, source_transform, source_crs, bounds, size=256):
    if rgb.ndim != 3 or rgb.shape[0] != 3 or rgb.dtype != np.uint8:
        raise ValueError("RGB 영상 자료 형식이 올바르지 않습니다.")
    if valid.shape != rgb.shape[1:] or not valid.any() or size < 2:
        raise ValueError("영상의 유효 픽셀 또는 크기가 올바르지 않습니다.")
    transform = from_bounds(*bounds, size, size)
    out = np.zeros((3, size, size), dtype=np.uint8)
    for band in range(3):
        reproject(source=rgb[band], destination=out[band],
                  src_transform=source_transform, src_crs=source_crs,
                  dst_transform=transform, dst_crs="EPSG:4326",
                  src_nodata=None, resampling=Resampling.bilinear)
    covered = np.zeros((size, size), dtype=np.uint8)
    reproject(source=valid.astype(np.uint8), destination=covered,
              src_transform=source_transform, src_crs=source_crs,
              dst_transform=transform, dst_crs="EPSG:4326",
              src_nodata=0, dst_nodata=0, resampling=Resampling.nearest)
    if np.any(covered == 0):
        raise ValueError("지도 범위 안에 위성영상 결측 픽셀이 있습니다.")
    return out, transform


def fetch_satellite(data: dict, destination: Path) -> dict:
    bounds = map_bounds(data)
    with rasterio.Env(GDAL_DISABLE_READDIR_ON_OPEN="EMPTY_DIR", GDAL_HTTP_TIMEOUT="60",
                      GDAL_HTTP_MAX_RETRY="1", GDAL_TIFF_INTERNAL_MASK=True):
        with rasterio.open("/vsicurl/" + SATELLITE_URL) as src:
            window = validate_satellite_dataset(src, bounds)
            rgb = src.read(window=window)
            valid = src.dataset_mask(window=window)
            transform = src.window_transform(window)
            crs = src.crs
            result, result_transform = reproject_satellite(rgb, valid, transform, crs, bounds)
            profile = {
                "driver": "GTiff", "width": rgb.shape[2], "height": rgb.shape[1],
                "count": 3, "dtype": "uint8", "crs": crs, "transform": transform,
                "compress": "deflate", "nodata": None,
            }
            with rasterio.open(destination / "satellite_source.tif", "w", **profile) as out:
                out.write(rgb)
                out.write_mask(valid)
    with rasterio.open(destination / "satellite.png", "w", driver="PNG", width=256,
                       height=256, count=3, dtype="uint8") as png:
        png.write(result)
    with rasterio.open(destination / "satellite.png") as check:
        if not np.array_equal(check.read(), result):
            raise ValueError("위성영상 PNG 재독 검증에 실패했습니다.")
    metadata = {
        "scene_id": SCENE_ID, "datetime": SCENE_TIME, "source_url": SATELLITE_URL,
        "source_catalog": "https://earth-search.aws.element84.com/v1/collections/sentinel-2-l2a/items/" + SCENE_ID,
        "source_crs": str(crs), "source_resolution_m": [10, 10],
        "bounds_wgs84": list(bounds), "image_width": 256, "image_height": 256,
        "image_transform": list(result_transform)[:6],
        "source_window": [int(window.col_off), int(window.row_off), int(window.width), int(window.height)],
        "source_sha256": sha256(destination / "satellite_source.tif"),
        "image_sha256": sha256(destination / "satellite.png"),
        "attribution": ATTRIBUTION,
        "processing": "Native 10 m RGB window; bilinear reprojection to EPSG:4326 display image.",
        "license_url": "https://cds.climate.copernicus.eu/licences/ec-sentinel",
    }
    (destination / "satellite_metadata.json").write_text(
        json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return metadata


def validate_photo_specs(data: dict, specs: list[dict]) -> None:
    buildings = {building["id"]: building for building in data["buildings"]}
    identifiers = set()
    filenames = set()
    for photo in specs:
        required = ("id", "filename", "title", "author", "date", "license", "source_page",
                    "edge_indices", "camera_direction", "uv_crop", "orientation_basis", "sha256")
        if any(not photo.get(k) for k in required):
            raise ValueError("외벽 사진의 필수 출처나 적용 정보가 없습니다.")
        if photo["id"] not in buildings or photo["id"] in identifiers or photo["filename"] in filenames:
            raise ValueError("외벽 사진의 건물 또는 파일이 없거나 중복되었습니다.")
        identifiers.add(photo["id"])
        filenames.add(photo["filename"])
        if Path(photo["filename"]).name != photo["filename"]:
            raise ValueError("사진 파일명에 디렉터리를 넣을 수 없습니다.")
        edges = photo["edge_indices"]
        if len(set(edges)) != len(edges) or any(not isinstance(i, int) or i < 0 or
                                              i >= len(buildings[photo["id"]]["footprint"]) for i in edges):
            raise ValueError("외벽 사진의 벽면 인덱스가 유효하지 않습니다.")
        direction = photo["camera_direction"]
        if len(direction) != 2 or not all(math.isfinite(x) for x in direction) or math.hypot(*direction) < 0.5:
            raise ValueError("외벽 사진의 방향이 유효하지 않습니다.")
        crop = photo["uv_crop"]
        if len(crop) != 4 or not all(isinstance(x, (int, float)) and math.isfinite(x) for x in crop):
            raise ValueError("외벽 사진의 투영 범위가 유효하지 않습니다.")
        left, top, right, bottom = crop
        if not (0 <= left < right <= 1 and 0 <= top < bottom <= 1):
            raise ValueError("외벽 사진의 투영 범위가 유효하지 않습니다.")
        if len(photo["sha256"]) != 64:
            raise ValueError("외벽 사진의 해시가 유효하지 않습니다.")


def build_texture_bundle(destination: Path, photo_specs: list[dict]) -> None:
    meta = json.loads((destination / "satellite_metadata.json").read_text(encoding="utf-8"))
    def data_url(path: Path, mime: str) -> str:
        return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode("ascii")
    textures = {"bounds_wgs84": meta["bounds_wgs84"],
                "satellite": data_url(destination / "satellite.png", "image/png"), "photos": []}
    for spec in photo_specs:
        textures["photos"].append({k: spec[k] for k in ("id", "edge_indices", "camera_direction", "uv_crop")} |
                                  {"data": data_url(destination / "photos" / spec["filename"], "image/jpeg")})
    (destination / "texture_data.js").write_text(
        "window.MYEONGDONG_TEXTURES=" + json.dumps(textures, separators=(",", ":")) + ";\n", encoding="utf-8")


def prepare_photos(data: dict, destination: Path, cache: Path | None) -> list[dict]:
    validate_photo_specs(data, PHOTO_SPECS)
    photos = destination / "photos"
    if any((destination / name).exists() for name in ("photos_metadata.json", "texture_data.js")):
        raise FileExistsError("기존 사진 메타데이터나 텍스처를 덮어쓰지 않습니다.")
    photos.mkdir(exist_ok=False)
    try:
        for photo in PHOTO_SPECS:
            target = photos / photo["filename"]
            if cache:
                shutil.copyfile(cache / photo["cache_name"], target)
            else:
                subprocess.run(["curl", "-fsSL", "--max-time", "30", "--user-agent",
                                "CBU-Myeongdong-Map/1.0 (local map)", "-o", str(target),
                                photo["download_url"]], check=True)
            if target.read_bytes()[:2] != b"\xff\xd8" or sha256(target) != photo["sha256"]:
                raise ValueError("외벽 사진의 형식이나 검증된 해시가 다릅니다.")
        (destination / "photos_metadata.json").write_text(
            json.dumps(PHOTO_SPECS, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        build_texture_bundle(destination, PHOTO_SPECS)
    except Exception:
        shutil.rmtree(photos)
        (destination / "photos_metadata.json").unlink(missing_ok=True)
        (destination / "texture_data.js").unlink(missing_ok=True)
        raise
    return PHOTO_SPECS


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("stage", choices=["satellite", "photos"])
    parser.add_argument("--photo-cache", type=Path, default=None)
    args = parser.parse_args()
    if args.stage == "satellite":
        if OUTPUT.exists():
            raise FileExistsError(f"기존 결과를 덮어쓰지 않습니다: {OUTPUT}")
        data = json.loads((SOURCE_MAP / "map_data.json").read_text(encoding="utf-8"))
        OUTPUT.mkdir()
        try:
            shutil.copytree(SOURCE_MAP, OUTPUT, dirs_exist_ok=True)
            metadata = fetch_satellite(data, OUTPUT)
            print(json.dumps(metadata, ensure_ascii=False, indent=2))
        except Exception:
            if OUTPUT.exists():
                shutil.rmtree(OUTPUT)
            raise
    elif args.stage == "photos":
        if not (OUTPUT / "satellite_metadata.json").is_file():
            raise FileNotFoundError("먼저 satellite 단계를 완료해야 합니다.")
        data = json.loads((OUTPUT / "map_data.json").read_text(encoding="utf-8"))
        print(json.dumps(prepare_photos(data, OUTPUT, args.photo_cache), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
