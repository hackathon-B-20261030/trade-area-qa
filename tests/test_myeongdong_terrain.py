import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import rasterio
import numpy as np
from rasterio.io import MemoryFile
from rasterio.transform import from_origin

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import download_myeongdong_terrain as terrain


def collection():
    return {
        "type": "FeatureCollection",
        "crs": {"type": "name", "properties": {"name": "urn:ogc:def:crs:OGC:1.3:CRS84"}},
        "features": [{
            "type": "Feature",
            "properties": {"adm_cd2": terrain.ADMIN_CODE, "adm_nm": terrain.ADMIN_NAME},
            "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [2, 0], [2, 2], [0, 2], [0, 0]]]},
        }],
    }


def payload(data):
    raw = json.dumps(data).encode()
    return raw, hashlib.sha256(raw).hexdigest()


class SourceTests(unittest.TestCase):
    def test_select_boundary_valid(self):
        source = collection()
        raw, digest = payload(source)
        self.assertEqual(terrain.select_boundary(raw, digest), source["features"][0])

    def test_select_boundary_rejects_invalid(self):
        source = collection()
        raw, digest = payload(source)
        with self.assertRaises(ValueError):
            terrain.select_boundary(raw, "0" * 64)
        invalid_json = b"{"
        with self.assertRaises(ValueError):
            terrain.select_boundary(invalid_json, hashlib.sha256(invalid_json).hexdigest())
        variants = []
        for edit in ("crs", "missing", "duplicate", "name", "empty", "self_intersection", "point"):
            item = copy.deepcopy(source)
            if edit == "crs":
                item["crs"]["properties"]["name"] = "EPSG:5179"
            elif edit == "missing":
                item["features"] = []
            elif edit == "duplicate":
                item["features"].append(copy.deepcopy(item["features"][0]))
            elif edit == "name":
                item["features"][0]["properties"]["adm_nm"] = "다른 동"
            elif edit == "empty":
                item["features"][0]["geometry"]["coordinates"] = []
            elif edit == "self_intersection":
                item["features"][0]["geometry"]["coordinates"] = [[[0, 0], [2, 2], [0, 2], [2, 0], [0, 0]]]
            elif edit == "point":
                item["features"][0]["geometry"] = {"type": "Point", "coordinates": [0, 0]}
            variants.append((edit, item))
        for name, item in variants:
            with self.subTest(name=name), self.assertRaises(ValueError):
                terrain.select_boundary(*payload(item))

    def test_validate_source_rejects_mismatch(self):
        good = {"crs": rasterio.crs.CRS.from_epsg(4326), "count": 1, "dtypes": ("float32",),
                "res": (1 / 3600, 1 / 3600), "nodata": terrain.NODATA}
        terrain.validate_source(SimpleNamespace(**good))
        for field, value in {"crs": None, "count": 2, "dtypes": ("int16",),
                             "res": (1 / 3600, 1 / 7200), "nodata": None}.items():
            with self.subTest(field=field), self.assertRaises(ValueError):
                terrain.validate_source(SimpleNamespace(**(good | {field: value})))


class CropTests(unittest.TestCase):
    def make_raster(self, values):
        memory = MemoryFile()
        with memory.open(driver="GTiff", height=5, width=5, count=1, dtype="float32",
                         crs="EPSG:4326", transform=from_origin(0, 5, 1, 1), nodata=-9999) as dst:
            dst.write(values.astype("float32"), 1)
        self.addCleanup(memory.close)
        return memory

    def test_crop_preserves_values_and_boundary(self):
        values = np.arange(25, dtype="float32").reshape((5, 5)) - 12
        geom = {"type": "Polygon", "coordinates": [[[1.2, 1.2], [3.8, 1.2], [1.2, 3.8], [1.2, 1.2]]]}
        with self.make_raster(values).open() as src:
            result, transform = terrain.crop_terrain(src, geom)
            self.assertEqual(result.shape, (1, 3, 3))
            self.assertEqual(transform, from_origin(1, 4, 1, 1))
            self.assertTrue(result.mask[0, 0, 2])
            self.assertFalse(result.mask[0, 2, 0])
            expected = values[1:4, 1:4]
            np.testing.assert_array_equal(result.data[0][~result.mask[0]], expected[~result.mask[0]])
            with tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / "terrain.tif"
                terrain.write_raster(path, result, transform, src.crs, src.nodata)
                with rasterio.open(path) as saved:
                    self.assertEqual(saved.crs, src.crs)
                    self.assertEqual(saved.transform, transform)
                    np.testing.assert_array_equal(np.ma.getmaskarray(saved.read(masked=True)), result.mask)
                    np.testing.assert_array_equal(saved.read(masked=True).compressed(), result.compressed())

    def test_crop_rejects_invalid_coverage(self):
        box = {"type": "Polygon", "coordinates": [[[1.2, 1.2], [3.8, 1.2], [3.8, 3.8], [1.2, 3.8], [1.2, 1.2]]]}
        outside = {"type": "Polygon", "coordinates": [[[-1, 0], [1, 0], [1, 1], [-1, 1], [-1, 0]]]}
        empty = {"type": "Polygon", "coordinates": []}
        values = np.arange(25, dtype="float32").reshape((5, 5))
        with self.make_raster(values).open() as src:
            for geom in (outside, empty):
                with self.assertRaises(ValueError):
                    terrain.crop_terrain(src, geom)
        for value in (-9999, np.nan, np.inf, -np.inf):
            with self.subTest(value=value):
                bad = values.copy()
                bad[2, 2] = value
                with self.make_raster(bad).open() as src, self.assertRaises(ValueError):
                    terrain.crop_terrain(src, box)


class PackageTests(unittest.TestCase):
    def test_verify_raster_detects_corruption(self):
        transform = from_origin(0, 2, 1, 1)
        crs = rasterio.crs.CRS.from_epsg(4326)
        data = np.ma.array([[[0, -1], [2, 3]]], mask=[[[False, False], [False, True]]], dtype="float32")
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "terrain.tif"
            terrain.write_raster(path, data, transform, crs, -9999)
            result = terrain.verify_raster(path, data, transform, crs, -9999)
            self.assertEqual((result["valid_cells"], result["min_m"], result["max_m"]), (3, -1.0, 2.0))
            changed = data.copy()
            changed[0, 0, 0] = 9
            terrain.write_raster(path, changed, transform, crs, -9999)
            with self.assertRaises(ValueError):
                terrain.verify_raster(path, data, transform, crs, -9999)
            changed = data.copy()
            changed.mask[0, 0, 0] = True
            terrain.write_raster(path, changed, transform, crs, -9999)
            with self.assertRaises(ValueError):
                terrain.verify_raster(path, data, transform, crs, -9999)
            terrain.write_raster(path, data, transform, rasterio.crs.CRS.from_epsg(3857), -9999)
            with self.assertRaises(ValueError):
                terrain.verify_raster(path, data, transform, crs, -9999)

    def test_output_exists_fails(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "existing"
            path.mkdir()
            marker = path / "keep"
            marker.write_text("existing", encoding="utf-8")
            with self.assertRaises(FileExistsError):
                terrain.build_package(b"invalid", path)
            self.assertEqual(marker.read_text(encoding="utf-8"), "existing")


if __name__ == "__main__":
    unittest.main()
