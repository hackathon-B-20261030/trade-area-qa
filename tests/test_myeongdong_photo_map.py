import importlib.util
import copy
import base64
import hashlib
from html.parser import HTMLParser
import json
import math
import unittest
from pathlib import Path

import numpy as np
import rasterio
from rasterio.io import MemoryFile
from rasterio.transform import from_bounds, from_origin
from rasterio.warp import transform

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("photo_map", ROOT / "scripts/prepare_myeongdong_photo_map.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SatelliteTests(unittest.TestCase):
    def test_bounds(self):
        bounds = module.map_bounds({"terrain": {"transform": [0.5, 0, 10, 0, -0.25, 40],
                                                "width": 4, "height": 8}})
        self.assertEqual(bounds, (10, 38, 12, 40))
        with self.assertRaises(ValueError):
            module.map_bounds({"terrain": {"transform": [0.5, 1, 10, 0, -0.25, 40],
                                           "width": 4, "height": 8}})

    def test_bad_imagery(self):
        rgb = np.zeros((3, 4, 4), dtype=np.uint8)
        rgb[0] = 70
        valid = np.full((4, 4), 255, dtype=np.uint8)
        aff = from_bounds(10, 20, 14, 24, 4, 4)
        out, _ = module.reproject_satellite(rgb, valid, aff, "EPSG:4326", (10, 20, 14, 24), 4)
        self.assertTrue(np.array_equal(out, rgb))  # Other channels may legitimately be zero.
        valid[:, :] = 0
        with self.assertRaises(ValueError):
            module.reproject_satellite(rgb, valid, aff, "EPSG:4326", (10, 20, 14, 24), 4)
        with MemoryFile() as mem:
            with mem.open(driver="GTiff", width=4, height=4, count=3, dtype="uint8",
                          crs="EPSG:4326", transform=from_origin(10, 24, 1, 1)) as src:
                with self.assertRaises(ValueError):
                    module.validate_satellite_dataset(src, (10, 20, 14, 24))

    def test_satellite_real(self):
        folder = module.OUTPUT
        if not (folder / "satellite_metadata.json").exists():
            self.skipTest("실제 위성 원본을 준비한 뒤 검증합니다")
        metadata = json.loads((folder / "satellite_metadata.json").read_text())
        data = json.loads((folder / "map_data.json").read_text())
        self.assertEqual(metadata["scene_id"], module.SCENE_ID)
        self.assertEqual(metadata["source_resolution_m"], [10, 10])
        self.assertEqual(metadata["bounds_wgs84"], list(module.map_bounds(data)))
        self.assertEqual(module.sha256(folder / "satellite_source.tif"), metadata["source_sha256"])
        self.assertEqual(module.sha256(folder / "satellite.png"), metadata["image_sha256"])
        self.assertIn("Copernicus Sentinel", metadata["attribution"])
        with rasterio.open(folder / "satellite_source.tif") as source, rasterio.open(folder / "satellite.png") as png:
            self.assertEqual(source.crs.to_epsg(), 32652)
            self.assertEqual(source.res, (10, 10))
            self.assertEqual(png.shape, (256, 256))
            self.assertTrue(np.all(source.dataset_mask() > 0))
            native = source.read()
            result = png.read()
            # Independent interpolation at scattered output-pixel centres.
            dst_transform = from_bounds(*metadata["bounds_wgs84"], 256, 256)
            positions = [(row, col) for row in (5, 47, 127, 208, 250) for col in (5, 54, 127, 201, 250)]
            for row, col in positions:
                lon, lat = dst_transform * (col + 0.5, row + 0.5)
                xs, ys = transform("EPSG:4326", source.crs, [lon], [lat])
                src_col, src_row = (~source.transform) * (xs[0], ys[0])
                y, x = src_row - 0.5, src_col - 0.5
                r0, c0 = math.floor(y), math.floor(x)
                fy, fx = y - r0, x - c0
                for band in range(3):
                    expected = (float(native[band, r0, c0]) * (1 - fy) * (1 - fx)
                                + float(native[band, r0 + 1, c0]) * fy * (1 - fx)
                                + float(native[band, r0, c0 + 1]) * (1 - fy) * fx
                                + float(native[band, r0 + 1, c0 + 1]) * fy * fx)
                    self.assertLessEqual(abs(float(result[band, row, col]) - expected), 2.5)


class FacadeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads((module.SOURCE_MAP / "map_data.json").read_text())
        cls.buildings = {building["id"]: building for building in cls.data["buildings"]}

    def test_photo_specs(self):
        from shapely.geometry import Polygon
        module.validate_photo_specs(self.data, module.PHOTO_SPECS)
        self.assertEqual({s["id"] for s in module.PHOTO_SPECS},
                         {"way/210335697", "way/348489095", "way/775236382", "way/296668636"})
        # Each projected edge really faces the recorded camera side.
        for spec in module.PHOTO_SPECS:
            points = self.buildings[spec["id"]]["footprint"]
            is_ccw = Polygon(points).exterior.is_ccw
            camera = spec["camera_direction"]
            for index in spec["edge_indices"]:
                p, q = points[index], points[(index + 1) % len(points)]
                dx, dz = q[0] - p[0], q[1] - p[1]
                length = math.hypot(dx, dz)
                self.assertGreater(length, 2)
                nx, nz = (dz / length, -dx / length) if is_ccw else (-dz / length, dx / length)
                self.assertGreater(nx * camera[0] + nz * camera[1], 0.84)
        # OSM identifies the chapel as the building within the Young Nak church grounds.
        raw = json.loads((module.SOURCE_MAP / "source/osm_raw.json").read_text())
        nodes = {x["id"]: (x["lon"], x["lat"]) for x in raw["elements"] if x["type"] == "node"}
        ways = {x["id"]: x for x in raw["elements"] if x["type"] == "way"}
        church = Polygon([nodes[n] for n in ways[387491589]["nodes"]])
        chapel = Polygon([nodes[n] for n in ways[296668636]["nodes"]])
        self.assertEqual(ways[387491589]["tags"]["name"], "영락교회")
        self.assertTrue(church.covers(chapel))
        for ident, name in ((210335697, "명동대성당"), (348489095, "한국은행 화폐박물관"),
                            (775236382, "서울중앙우체국")):
            self.assertIn(name, ways[ident]["tags"]["name"])

    def test_invalid_photo_spec(self):
        for mutation in (lambda p: p.update(id="way/does-not-exist"),
                         lambda p: p.update(edge_indices=[99999]),
                         lambda p: p.update(uv_crop=[0.7, 0, 0.2, 1]),
                         lambda p: p.update(author="")):
            broken = copy.deepcopy(module.PHOTO_SPECS)
            mutation(broken[0])
            with self.assertRaises(ValueError):
                module.validate_photo_specs(self.data, broken)
        broken = copy.deepcopy(module.PHOTO_SPECS)
        broken.append(copy.deepcopy(broken[0]))
        with self.assertRaises(ValueError):
            module.validate_photo_specs(self.data, broken)

    def test_photo_credits(self):
        path = module.OUTPUT / "photos_metadata.json"
        if not path.exists():
            self.skipTest("사진을 준비한 뒤 검증합니다")
        photos = json.loads(path.read_text())
        module.validate_photo_specs(self.data, photos)
        self.assertEqual(len(photos), 4)
        for photo in photos:
            self.assertTrue(photo["source_page"].startswith("https://commons.wikimedia.org/wiki/File:"))
            self.assertIn(photo["license"], {"CC BY-SA 4.0", "CC BY 4.0", "CC BY-SA 3.0", "Public domain"})
            self.assertEqual(module.sha256(module.OUTPUT / "photos" / photo["filename"]), photo["sha256"])
            self.assertTrue(photo["date"] and photo["author"] and photo["orientation_basis"])


class OutputTests(unittest.TestCase):
    def test_photo_map_provenance(self):
        original, output = module.SOURCE_MAP, module.OUTPUT
        for name in ("map_data.json", "map_data.js", "terrain_extended.tif", "terrain_metadata.json",
                     "viewer.css", "source/myeongdong_data.json", "source/osm_raw.json",
                     "vendor/three.min.js", "vendor/OrbitControls.js", "vendor/LICENSE.txt"):
            self.assertEqual(module.sha256(original / name), module.sha256(output / name), name)
        metadata = json.loads((output / "satellite_metadata.json").read_text())
        photos = json.loads((output / "photos_metadata.json").read_text())
        content = (output / "texture_data.js").read_text()
        prefix = "window.MYEONGDONG_TEXTURES="
        self.assertTrue(content.startswith(prefix) and content.endswith(";\n"))
        packed = json.loads(content[len(prefix):-2])
        self.assertEqual(packed["bounds_wgs84"], metadata["bounds_wgs84"])
        def digest_data_url(data):
            return hashlib.sha256(base64.b64decode(data.split(",", 1)[1], validate=True)).hexdigest()
        self.assertEqual(digest_data_url(packed["satellite"]), metadata["image_sha256"])
        self.assertEqual(len(packed["photos"]), 4)
        for asset, photo in zip(packed["photos"], photos, strict=True):
            self.assertEqual(asset["id"], photo["id"])
            self.assertEqual(asset["edge_indices"], photo["edge_indices"])
            self.assertEqual(digest_data_url(asset["data"]), photo["sha256"])
        class Resources(HTMLParser):
            def __init__(self):
                super().__init__()
                self.scripts = []
            def handle_starttag(self, tag, attrs):
                attrs = dict(attrs)
                if tag == "script" and "src" in attrs:
                    self.scripts.append(attrs["src"])
        html = (output / "index.html").read_text()
        parser = Resources()
        parser.feed(html)
        self.assertEqual(parser.scripts, ["vendor/three.min.js", "vendor/OrbitControls.js", "map_data.js",
                                          "texture_data.js", "viewer.js"])
        self.assertTrue(all((output / name).is_file() for name in parser.scripts))
        for token in ("Sentinel", "10m", "Tristan Surtel", "Sean Young", "KSH1015", "David Milanaik"):
            self.assertIn(token, html)
        readme = (output / "README.md").read_text()
        self.assertIn("네 건물", readme)
        self.assertIn("10m", readme)
        self.assertIn("file://", readme)


if __name__ == "__main__":
    unittest.main()
