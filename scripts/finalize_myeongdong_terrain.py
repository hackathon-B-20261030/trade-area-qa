"""Create the agreed, mask-aware smoothed Myeong-dong terrain dataset."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import rasterio
from numpy.lib.stride_tricks import sliding_window_view


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def gaussian_smooth(data: np.ma.MaskedArray, sigma: float = 1.0, radius: int = 4) -> np.ma.MaskedArray:
    data = np.ma.asarray(data)
    if data.ndim != 2 or not math.isfinite(sigma) or sigma <= 0:
        raise ValueError("Expected a two-dimensional grid and positive finite sigma")
    if isinstance(radius, bool) or not isinstance(radius, int) or radius < 1:
        raise ValueError("Kernel radius must be a positive integer")
    mask = np.ma.getmaskarray(data)
    valid = ~mask
    if not np.any(valid) or not np.isfinite(data.data[valid]).all():
        raise ValueError("The grid must contain finite, unmasked elevations")
    distance = np.arange(-radius, radius + 1, dtype=np.float64)
    weights = np.exp(-0.5 * (distance / sigma) ** 2)
    kernel = np.outer(weights, weights)
    values = np.where(valid, data.data, 0).astype(np.float64)
    padded_values = np.pad(values, radius, mode="constant", constant_values=0)
    padded_valid = np.pad(valid.astype(np.float64), radius, mode="constant", constant_values=0)
    window_shape = (2 * radius + 1, 2 * radius + 1)
    weighted_values = np.einsum("ijkl,kl->ij", sliding_window_view(padded_values, window_shape), kernel)
    weighted_valid = np.einsum("ijkl,kl->ij", sliding_window_view(padded_valid, window_shape), kernel)
    if np.any(weighted_valid[valid] <= 0):
        raise ValueError("A valid cell has no available smoothing weights")
    output = np.zeros(data.shape, dtype=np.float32)
    output[valid] = (weighted_values[valid] / weighted_valid[valid]).astype(np.float32)
    return np.ma.array(output, mask=mask, copy=False)


def finalize(source_dir: Path, output_dir: Path) -> None:
    source_dir, output_dir = Path(source_dir), Path(output_dir)
    if output_dir.exists():
        raise FileExistsError(f"Final output already exists: {output_dir}")
    source_metadata_path = source_dir / "metadata.json"
    metadata = json.loads(source_metadata_path.read_text(encoding="utf-8"))
    raster_meta = metadata["raster"]
    raster_path = source_dir / raster_meta["file"]
    original_hash = file_sha256(raster_path)
    if original_hash != raster_meta["sha256"]:
        raise ValueError("Original terrain hash does not match its metadata")
    with rasterio.open(raster_path) as source:
        if (source.crs is None or source.crs.to_string() != raster_meta["crs"] or source.count != 1
                or source.dtypes != ("float32",) or source.nodata != raster_meta["nodata"]
                or source.width != raster_meta["width"] or source.height != raster_meta["height"]
                or list(tuple(source.transform)[:6]) != raster_meta["transform"]):
            raise ValueError("Original terrain grid does not match its metadata")
        original = source.read(1, masked=True)
        if (original.count() != raster_meta["valid_cells"] or original.count() != 1462
                or float(original.min()) != raster_meta["min_m"] or float(original.max()) != raster_meta["max_m"]):
            raise ValueError("Original terrain values do not match its metadata")
        smoothed = gaussian_smooth(original)
        crs, transform, nodata = source.crs, source.transform, source.nodata

    output_dir.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".final-terrain-", dir=output_dir.parent) as temporary:
        stage = Path(temporary)
        result_path = stage / "terrain_smoothed.tif"
        with rasterio.open(result_path, "w", driver="GTiff", height=smoothed.shape[0], width=smoothed.shape[1],
                           count=1, dtype="float32", crs=crs, transform=transform, nodata=nodata,
                           compress="deflate", predictor=3) as target:
            target.write(smoothed.filled(nodata), 1)
            target.update_tags(UNIT="metre", VERTICAL_DATUM="EGM2008 (EPSG:3855)",
                               SOURCE="GEDTM30 v1.2", PROCESSING="mask-normalized Gaussian sigma=1 cell radius=4 cells")
        with rasterio.open(result_path) as result:
            saved = result.read(1, masked=True)
            if (result.crs != crs or result.transform != transform or result.nodata != nodata
                    or result.count != 1 or result.dtypes != ("float32",)
                    or not np.array_equal(np.ma.getmaskarray(saved), np.ma.getmaskarray(original))
                    or not np.array_equal(saved.compressed(), smoothed.compressed())):
                raise ValueError("Re-read final terrain differs from the verified result")
        if file_sha256(raster_path) != original_hash:
            raise ValueError("Original terrain changed during processing")
        changes = np.abs(saved.compressed().astype(np.float64) - original.compressed().astype(np.float64))
        result_meta = {
            "status": "finalized",
            "created_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "source": {
                "raster_path": os.path.relpath(raster_path, output_dir),
                "raster_sha256": original_hash,
                "metadata_path": os.path.relpath(source_metadata_path, output_dir),
                "dataset": metadata["elevation_source"]["name"],
                "version": metadata["elevation_source"]["version"],
                "license": metadata["elevation_source"]["license"],
            },
            "processing": {
                "method": "mask-normalized Gaussian convolution",
                "sigma_cells": 1.0,
                "radius_cells": 4,
                "missing_policy": "exclude missing cells and renormalize; retain original mask",
                "display_interpolation": "cell centers exact; shared corner averages; linear triangle surface",
            },
            "raster": {
                "file": result_path.name,
                "sha256": file_sha256(result_path),
                "crs": crs.to_string(),
                "width": saved.shape[1],
                "height": saved.shape[0],
                "transform": [float(value) for value in tuple(transform)[:6]],
                "nodata": float(nodata),
                "valid_cells": int(saved.count()),
                "min_m": float(saved.min()),
                "max_m": float(saved.max()),
                "mean_absolute_change_m": float(changes.mean()),
                "max_absolute_change_m": float(changes.max()),
            },
            "vertical_datum": metadata["elevation_source"]["vertical_datum"],
            "unit": "metre",
        }
        (stage / "metadata.json").write_text(json.dumps(result_meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        (stage / "README.md").write_text(f"""# 명동 행정동 최종 지형 — 표준 스무딩

서울 중구 명동 행정동의 지면 고도 데이터를 표준 강도로 스무딩한 최종 파생 지형입니다.

| 파일 | 내용 |
|---|---|
| `terrain_smoothed.tif` | 최종 지면 고도 GeoTIFF (EPSG:4326, EGM2008, 미터) |
| `metadata.json` | 처리 방법, 원본 및 결과 SHA-256, 격자와 변화 통계 |

원본 [GEDTM30 v1.2.0](https://doi.org/10.5281/zenodo.18887460) 고도는 `../terrain_gedtm30_v1.2.tif`이며 덮어쓰지 않았습니다. 자료는 CC BY 4.0이고, 원본 출처와 명동 경계 출처(SGIS / vuski·admdongkor)는 [상위 README](../README.md)를 따릅니다.

- 가우시안 표준편차 **1.0격자**, 반경 **4격자**를 적용했습니다. 유효 셀만 가중 평균하고 NoData는 제외한 뒤 가중치를 정규화했습니다. 경계 및 NoData 마스크는 원본과 같습니다.
- 픽셀 간격은 원본과 같은 1초각(약 30m)입니다. 스무딩은 작은 봉우리와 골짜기를 줄이므로 측량 정확도나 실제 해상도를 높이지 않습니다.
- 최종 유효 셀 {saved.count():,}개, 높이 범위 {float(saved.min()):.1f}~{float(saved.max()):.1f}m; 원본 대비 평균 절대 변화 {float(changes.mean()):.2f}m, 최대 절대 변화 {float(changes.max()):.2f}m입니다.
- 연속 3D 표면에서 격자 중심은 최종값을 사용하고, 모서리와 면 내부는 인접 유효 격자에 기반한 표시용 보간값입니다. 행정동에 걸친 격자의 일부는 경계 밖으로 확장될 수 있습니다.

프로젝트 루트에서 `python scripts/finalize_myeongdong_terrain.py`로 재생성할 수 있습니다. 기존 최종 디렉터리는 자동으로 덮어쓰지 않습니다.
""", encoding="utf-8")
        if json.loads((stage / "metadata.json").read_text(encoding="utf-8")) != result_meta:
            raise ValueError("Saved final metadata differs from the verified result")
        if output_dir.exists():
            raise FileExistsError(f"Final output already exists: {output_dir}")
        os.rename(stage, output_dir)


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=root / "data" / "myeongdong-terrain")
    parser.add_argument("--output-dir", type=Path, default=None)
    args = parser.parse_args()
    output = args.output_dir if args.output_dir else args.source_dir / "final"
    finalize(args.source_dir, output)
    print(f"Saved finalized terrain to {output}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, rasterio.errors.RasterioError) as exc:
        print(f"Terrain finalization failed: {exc}", file=sys.stderr)
        sys.exit(1)
