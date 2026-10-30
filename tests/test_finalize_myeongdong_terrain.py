import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
import rasterio
from rasterio.transform import from_origin

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from finalize_myeongdong_terrain import file_sha256, finalize, gaussian_smooth


class SmoothingTests(unittest.TestCase):
    def test_constant_and_mask(self):
        data = np.ma.array(np.full((4, 5), -3.0, dtype=np.float32),
                           mask=[[0, 1, 0, 0, 0], [0, 0, 0, 1, 0],
                                 [1, 0, 0, 0, 0], [0, 0, 0, 0, 1]])
        result = gaussian_smooth(data)
        np.testing.assert_array_equal(np.ma.getmaskarray(result), np.ma.getmaskarray(data))
        np.testing.assert_array_equal(result.compressed(), data.compressed())
        one = np.ma.masked_all((3, 3), dtype="float32")
        one[1, 1] = 0
        self.assertEqual(float(gaussian_smooth(one)[1, 1]), 0)

    def test_peak_diffuses_without_invalid_leak(self):
        data = np.ma.array(np.zeros((7, 7), dtype=np.float32), mask=np.zeros((7, 7), dtype=bool))
        data[3, 3] = 100
        data.mask[0, 0] = True
        data.data[0, 0] = 999999
        result = gaussian_smooth(data)
        self.assertTrue(result.mask[0, 0])
        self.assertGreater(float(result[3, 3]), 0)
        self.assertLess(float(result[3, 3]), 100)
        self.assertGreater(float(result[3, 4]), 0)
        self.assertAlmostEqual(float(result[3, 4]), float(result[4, 3]), places=4)
        self.assertGreaterEqual(float(result.min()), 0)
        self.assertLessEqual(float(result.max()), 100)

    def test_invalid_data_rejected(self):
        for data in (np.ma.masked_all((2, 2)), np.ma.array([[float('nan')]]),
                     np.ma.array([[float('inf')]]), np.ma.array([1, 2, 3])):
            with self.subTest(data=str(data)), self.assertRaises(ValueError):
                gaussian_smooth(data)
        good = np.ma.array([[1]])
        for kwargs in ({"sigma": 0}, {"sigma": -1}, {"sigma": float('nan')},
                       {"radius": 0}, {"radius": 1.5}, {"radius": True}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                gaussian_smooth(good, **kwargs)

    def test_source_errors_and_existing_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source"
            source.mkdir()
            output = Path(temporary) / "output"
            raster = source / "source.tif"
            with rasterio.open(raster, "w", driver="GTiff", width=1, height=1, count=1,
                               dtype="float32", transform=from_origin(0, 1, 1, 1), nodata=-9999) as dst:
                dst.write(np.array([[1]], dtype="float32"), 1)
            meta = {"raster": {"file": raster.name, "sha256": file_sha256(raster), "crs": "EPSG:4326"}}
            (source / "metadata.json").write_text(json.dumps(meta), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "grid does not match"):
                finalize(source, output)
            self.assertFalse(output.exists())
            output.mkdir()
            marker = output / "keep"
            marker.write_text("existing", encoding="utf-8")
            with self.assertRaises(FileExistsError):
                finalize(source, output)
            self.assertEqual(marker.read_text(encoding="utf-8"), "existing")


if __name__ == "__main__":
    unittest.main()
