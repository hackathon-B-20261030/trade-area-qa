"""Acquire a pinned GEDTM30 extract for Seoul's Myeong-dong administrative area."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import ssl
import sys
import tempfile
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import certifi
import numpy as np
import rasterio
from affine import Affine
from rasterio.features import geometry_mask
from rasterio.mask import mask
from shapely.geometry import shape


BOUNDARY_URL = (
    "https://raw.githubusercontent.com/vuski/admdongkor/"
    "d5d5a821c8de6cc337d47eeca604d7af76271c9b/"
    "ver20261001/HangJeongDong_ver20261001.geojson"
)
BOUNDARY_SHA256 = "843ece3a0b3429742cd4a1b699cc13987a88b5e5401f382e7fdaaa194ea5e605"
TERRAIN_URL = (
    "https://s3.opengeohub.org/global/dtm/v1.2/"
    "gedtm_rf_m_30m_s_20060101_20151231_go_epsg.4326.3855_v1.2.tif"
)
TERRAIN_ETAG = '"8bf9b35f45e9befaa9b91d877bf230d7-3107"'
ADMIN_CODE = "1114055000"
ADMIN_NAME = "서울특별시 중구 명동"
NODATA = 3.4028234663852886e38
MAX_BOUNDARY_BYTES = 60 * 1024 * 1024
USER_AGENT = "CBU-Hack-Myeongdong-Terrain/1.0"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as src:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def select_boundary(payload: bytes, expected_sha256: str) -> dict:
    if hashlib.sha256(payload).hexdigest() != expected_sha256:
        raise ValueError("Boundary source SHA-256 does not match the pinned version")
    try:
        collection = json.loads(payload)
        if collection.get("type") != "FeatureCollection":
            raise ValueError("Boundary source is not a FeatureCollection")
        crs = collection.get("crs", {}).get("properties", {}).get("name")
        if crs != "urn:ogc:def:crs:OGC:1.3:CRS84":
            raise ValueError("Boundary source is not in CRS84")
        features = collection.get("features")
        if not isinstance(features, list):
            raise ValueError("Boundary source has no feature list")
        matches = [
            feature for feature in features
            if isinstance(feature, dict)
            and isinstance(feature.get("properties"), dict)
            and feature["properties"].get("adm_cd2") == ADMIN_CODE
        ]
        if len(matches) != 1:
            raise ValueError("Expected exactly one Myeong-dong administrative boundary")
        feature = matches[0]
        if feature.get("type") != "Feature" or feature["properties"].get("adm_nm") != ADMIN_NAME:
            raise ValueError("Boundary feature has an unexpected name or type")
        geometry = feature.get("geometry")
        if not isinstance(geometry, dict) or geometry.get("type") not in ("Polygon", "MultiPolygon"):
            raise ValueError("Boundary geometry is not a polygon")
        polygon = shape(geometry)
        if polygon.is_empty or not polygon.is_valid or polygon.area <= 0:
            raise ValueError("Boundary polygon is empty or invalid")
        return feature
    except (AttributeError, KeyError, TypeError, json.JSONDecodeError) as exc:
        raise ValueError("Invalid boundary source") from exc


def _open(request: urllib.request.Request):
    context = ssl.create_default_context(cafile=certifi.where())
    return urllib.request.urlopen(request, timeout=60, context=context)


def fetch_boundary(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with _open(request) as response:
        payload = response.read(MAX_BOUNDARY_BYTES + 1)
    if len(payload) > MAX_BOUNDARY_BYTES:
        raise ValueError("Boundary source exceeds the 60 MiB limit")
    return payload


def check_source(url: str, expected_etag: str) -> None:
    headers = {"User-Agent": USER_AGENT}
    with _open(urllib.request.Request(url, method="HEAD", headers=headers)) as response:
        if response.status != 200 or response.headers.get("ETag") != expected_etag:
            raise ValueError("Terrain source ETag has changed")
    with _open(urllib.request.Request(url, headers={**headers, "Range": "bytes=0-15"})) as response:
        content_range = response.headers.get("Content-Range", "")
        if (response.status != 206 or response.headers.get("ETag") != expected_etag
                or not content_range.startswith("bytes 0-15/") or len(response.read(17)) != 16):
            raise ValueError("Terrain source does not support the expected byte range")


def validate_source(src: rasterio.io.DatasetReader) -> None:
    if src.crs != rasterio.crs.CRS.from_epsg(4326):
        raise ValueError("Terrain source CRS is not EPSG:4326")
    if src.count != 1 or src.dtypes != ("float32",):
        raise ValueError("Terrain source is not a single Float32 band")
    if any(not math.isclose(res, 1 / 3600, rel_tol=0, abs_tol=1e-10) for res in src.res):
        raise ValueError("Terrain source resolution is not one arc-second")
    if src.nodata != NODATA:
        raise ValueError("Terrain source NoData value is unexpected")


def crop_terrain(src: rasterio.io.DatasetReader, geometry: dict) -> tuple[np.ma.MaskedArray, Affine]:
    polygon = shape(geometry)
    if polygon.is_empty or not polygon.is_valid or polygon.geom_type not in ("Polygon", "MultiPolygon"):
        raise ValueError("Boundary must be a valid nonempty polygon")
    left, bottom, right, top = polygon.bounds
    bounds = src.bounds
    if left < bounds.left or bottom < bounds.bottom or right > bounds.right or top > bounds.top:
        raise ValueError("The full boundary does not fit inside the terrain source")
    data, transform = mask(src, [geometry], crop=True, all_touched=True, filled=False)
    selected = geometry_mask([geometry], out_shape=data.shape[1:], transform=transform,
                             all_touched=True, invert=True)
    if not np.any(selected):
        raise ValueError("No terrain cells intersect the boundary")
    missing = np.ma.getmaskarray(data)[0] | ~np.isfinite(data.data[0])
    if np.any(selected & missing):
        raise ValueError("Terrain contains missing or non-finite values inside the boundary")
    return data, transform


def write_raster(path: Path, data: np.ma.MaskedArray, transform: Affine,
                 crs: rasterio.crs.CRS, nodata: float) -> None:
    with rasterio.open(path, "w", driver="GTiff", height=data.shape[1], width=data.shape[2],
                       count=1, dtype="float32", crs=crs, transform=transform, nodata=nodata,
                       compress="deflate", predictor=3) as dst:
        dst.write(data.filled(nodata).astype("float32"))
        dst.update_tags(UNIT="metre", VERTICAL_DATUM="EGM2008 (EPSG:3855)",
                        SOURCE="GEDTM30 v1.2", SELECTION="all_touched=true; no resampling")


def verify_raster(path: Path, data: np.ma.MaskedArray, transform: Affine,
                  crs: rasterio.crs.CRS, nodata: float) -> dict:
    with rasterio.open(path) as saved:
        if (saved.crs != crs or saved.transform != transform or saved.count != 1
                or saved.dtypes != ("float32",) or saved.nodata != nodata
                or (saved.height, saved.width) != data.shape[1:]):
            raise ValueError("Saved raster metadata does not match the selected source")
        actual = saved.read(masked=True)
        if (not np.array_equal(np.ma.getmaskarray(actual), np.ma.getmaskarray(data))
                or not np.array_equal(actual.compressed(), data.compressed())):
            raise ValueError("Saved raster values or mask do not match the selected source")
        valid = actual.compressed()
        if valid.size == 0 or not np.all(np.isfinite(valid)):
            raise ValueError("Saved raster has no finite elevation values")
        return {
            "file": path.name,
            "sha256": sha256_file(path),
            "crs": crs.to_string(),
            "width": saved.width,
            "height": saved.height,
            "transform": [float(v) for v in tuple(saved.transform)[:6]],
            "nodata": float(nodata),
            "selection": "all_touched=true; no resampling",
            "valid_cells": int(valid.size),
            "min_m": float(valid.min()),
            "max_m": float(valid.max()),
        }


def build_package(boundary_payload: bytes, output_dir: Path) -> None:
    output_dir = Path(output_dir)
    if output_dir.exists():
        raise FileExistsError(f"Output already exists: {output_dir}")
    boundary = select_boundary(boundary_payload, BOUNDARY_SHA256)
    check_source(TERRAIN_URL, TERRAIN_ETAG)
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".myeongdong-terrain-", dir=output_dir.parent) as temp:
        stage = Path(temp)
        boundary_path = stage / "boundary.geojson"
        boundary_collection = {"type": "FeatureCollection", "features": [boundary]}
        boundary_path.write_text(json.dumps(boundary_collection, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        if json.loads(boundary_path.read_text(encoding="utf-8")) != boundary_collection:
            raise ValueError("Saved boundary does not match the selected feature")
        raster_path = stage / "terrain_gedtm30_v1.2.tif"
        with rasterio.Env(GDAL_DISABLE_READDIR_ON_OPEN="EMPTY_DIR", GDAL_HTTP_TIMEOUT="60",
                          GDAL_HTTP_MAX_RETRY="1"):
            with rasterio.open("/vsicurl/" + TERRAIN_URL) as src:
                validate_source(src)
                data, transform = crop_terrain(src, boundary["geometry"])
                write_raster(raster_path, data, transform, src.crs, src.nodata)
                raster_info = verify_raster(raster_path, data, transform, src.crs, src.nodata)
        check_source(TERRAIN_URL, TERRAIN_ETAG)
        metadata = {
            "schema_version": 1,
            "created_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "area": {
                "name": ADMIN_NAME,
                "admin_code": ADMIN_CODE,
                "statistics_code": boundary["properties"]["adm_cd"],
                "boundary_file": boundary_path.name,
                "boundary_sha256": sha256_file(boundary_path),
                "boundary_source_url": BOUNDARY_URL,
                "boundary_source_sha256": BOUNDARY_SHA256,
                "boundary_version": "2026-10-01",
                "boundary_license": "CC BY 4.0; SGIS KOGL Type 1 attribution",
            },
            "elevation_source": {
                "name": "Global Ensemble Digital Terrain Model (GEDTM30)",
                "version": "1.2.0",
                "doi": "https://doi.org/10.5281/zenodo.18887460",
                "url": TERRAIN_URL,
                "etag": TERRAIN_ETAG,
                "license": "CC BY 4.0",
                "source_period": "2006-01-01/2015-12-31",
                "vertical_datum": "EGM2008 geoid (EPSG:3855)",
                "unit": "metre",
            },
            "raster": raster_info,
        }
        (stage / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        (stage / "README.md").write_text(
            f"""# 명동 행정동 지면 고도 데이터

서울특별시 중구 명동 행정동(행정동코드 `{ADMIN_CODE}`)의 지면 고도입니다.

| 파일 | 내용 |
|---|---|
| `terrain_gedtm30_v1.2.tif` | 지면 고도 GeoTIFF, {raster_info['width']}×{raster_info['height']} 픽셀, 유효 셀 {raster_info['valid_cells']:,}개 |
| `boundary.geojson` | 사용한 명동 행정동 경계 |
| `metadata.json` | 출처, 해시, 좌표계, 격자, 고도 통계 |

- 고도 원본: Ho, Yufeng & Hengl, Tom, **Global Ensemble Digital Terrain Model 30m (GEDTM30), v1.2.0**, [Zenodo DOI: 10.5281/zenodo.18887460](https://doi.org/10.5281/zenodo.18887460), [논문 DOI: 10.7717/peerj.19673](https://doi.org/10.7717/peerj.19673). [OpenTopography 자료 설명과 CC BY 4.0](https://portal.opentopography.org/datasetMetadata?otCollectionID=OT.082025.4326.1).
- 경계 원본: 통계청 통계지리정보서비스(SGIS, https://sgis.kostat.go.kr)에서 공공누리 제1유형으로 개방한 행정동 경계를 가공한 자료([vuski/admdongkor](https://github.com/vuski/admdongkor)), CC BY 4.0. 경계 버전은 2026-10-01이며 이 패키지에서는 명동만 추출했습니다. [자료 이용 조건](https://github.com/vuski/admdongkor/blob/d5d5a821c8de6cc337d47eeca604d7af76271c9b/LICENSE-DATA).
- 좌표: 수평 EPSG:4326(WGS84), 수직 EGM2008 지오이드(EPSG:3855). 고도 단위는 미터입니다. GeoTIFF의 수평 좌표와 별도로 수직 기준을 여기에 기록합니다.
- 픽셀 간격은 1초각(약 30m)이며, 보간 없이 원본 픽셀을 사용합니다. 행정동 경계에 닿는 픽셀을 포함하므로 픽셀 일부는 경계 밖으로 걸칩니다. 경계에 닿지 않는 픽셀은 NoData입니다.
- 2006~2015년 원천 자료를 사용해 건물·식생의 영향을 줄이도록 모델링한 추정 지면으로, 최신 측량값이나 개별 건물·도로의 정밀 높이가 아닙니다.
- 결과 고도 범위: {raster_info['min_m']:.1f}~{raster_info['max_m']:.1f}m. 실제 고도 정확도를 별도로 검증한 값은 아닙니다.

재생성 시 프로젝트 루트에서 Python 환경에 `pip install -r scripts/requirements-myeongdong-terrain.txt`를 실행한 뒤 `python scripts/download_myeongdong_terrain.py`를 사용합니다. 기존 출력이 있으면 덮어쓰지 않습니다.
""", encoding="utf-8")
        if json.loads((stage / "metadata.json").read_text(encoding="utf-8")) != metadata:
            raise ValueError("Saved metadata differs from the verified result")
        if output_dir.exists():
            raise FileExistsError(f"Output already exists: {output_dir}")
        os.rename(stage, output_dir)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--boundary-file", type=Path, help="Use a previously downloaded, pinned boundary source")
    parser.add_argument("--output-dir", type=Path,
                        default=Path(__file__).resolve().parents[1] / "data" / "myeongdong-terrain")
    args = parser.parse_args()
    if args.output_dir.exists():
        raise FileExistsError(f"Output already exists: {args.output_dir}")
    if args.boundary_file is not None:
        if args.boundary_file.stat().st_size > MAX_BOUNDARY_BYTES:
            raise ValueError("Boundary source exceeds the 60 MiB limit")
        payload = args.boundary_file.read_bytes()
    else:
        payload = fetch_boundary(BOUNDARY_URL)
    build_package(payload, args.output_dir)
    print(f"Saved verified Myeong-dong terrain to {args.output_dir}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, rasterio.errors.RasterioError) as exc:
        print(f"Terrain download failed: {exc}", file=sys.stderr)
        sys.exit(1)
