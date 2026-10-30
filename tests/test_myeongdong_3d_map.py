import copy
import json
import math
import sys
import unittest
from pathlib import Path

import numpy as np
from rasterio.transform import from_origin
from shapely.geometry import Polygon, box

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import build_myeongdong_3d_map as builder


class ExtendTerrainTests(unittest.TestCase):
    def test_blend_preserves_confirmed_cells(self):
        raw = np.ma.array(np.arange(12 * 13, dtype="float32").reshape(12, 13), mask=False)
        final = np.ma.array(np.full((4, 5), 45, dtype="float32"),
                            mask=[[0, 0, 1, 1, 1], [0, 1, 1, 0, 1],
                                  [0, 0, 1, 0, 0], [1, 1, 1, 0, 0]])
        raw_before, final_before = raw.copy(), final.copy()
        result = builder.blend_extended(raw, final)
        valid = ~np.ma.getmaskarray(final)
        np.testing.assert_array_equal(result.data[valid], final.data[valid])
        self.assertFalse(np.ma.getmaskarray(result).any())
        self.assertTrue(np.isfinite(result.data).all())
        np.testing.assert_array_equal(raw.data, raw_before.data)
        np.testing.assert_array_equal(final.data, final_before.data)

    def test_blend_rejects_bad_input(self):
        final = np.ma.array([[1.0]], mask=[[False]])
        good = np.ma.array(np.ones((9, 9)), mask=np.zeros((9, 9), dtype=bool))
        for raw in (np.ones((4, 4)), np.ma.array(np.ones((9, 9)), mask=np.ones((9, 9))),
                    np.ma.array(np.full((9, 9), np.nan)), np.ma.array(np.full((9, 9), np.inf))):
            with self.subTest(raw=str(raw)), self.assertRaises(ValueError):
                builder.blend_extended(raw, final)
        for fixed in (np.ma.masked_all((1, 1)), np.ma.array([[np.nan]]), np.ma.array([1])):
            with self.subTest(fixed=str(fixed)), self.assertRaises(ValueError):
                builder.blend_extended(good, fixed)
        with self.assertRaises(ValueError):
            builder.blend_extended(good, final, radius=3)


class AlignmentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        directory = Path('/Users/aiden/Downloads/myeongdong-3d')
        cls.data = json.loads((directory / 'myeongdong_data.json').read_text(encoding='utf-8-sig'))
        cls.osm = json.loads((directory / 'osm_raw.json').read_text(encoding='utf-8-sig'))

    def test_coordinate_contract(self):
        center = self.data['center']
        self.assertEqual(builder.to_local(center['lon'], center['lat'], center), (0, 0))
        way = next(e for e in self.osm['elements'] if e['type'] == 'way' and e['id'] == 404947436)
        nodes = {e['id']: e for e in self.osm['elements'] if e['type'] == 'node'}
        raw = [builder.to_local(nodes[n]['lon'], nodes[n]['lat'], center) for n in way['nodes']]
        building = next(b for b in self.data['buildings'] if b['id'] == 404947436)
        for point in building['footprint']:
            self.assertLess(min(math.dist(point, candidate) for candidate in raw), 0.015)

    def test_relation_and_duplicate_repair(self):
        output = builder.reconstruct_buildings(self.data['buildings'], self.osm, self.data['center'])
        self.assertEqual(len(output), 709)
        tower = next(b for b in output if b['id'] == 'relation/8814915')
        self.assertEqual(tower['height'], 120)
        self.assertEqual(tower['source_ids'], ['way/634096161', 'way/634096168'])
        self.assertTrue(Polygon(tower['footprint']).is_valid)
        ways = {e['id']: e for e in self.osm['elements'] if e['type'] == 'way'}
        nodes = {e['id']: e for e in self.osm['elements'] if e['type'] == 'node'}
        ring = ways[634096161]['nodes'] + ways[634096168]['nodes'][1:-1]
        expected = [builder.to_local(nodes[n]['lon'], nodes[n]['lat'], self.data['center']) for n in ring]
        np.testing.assert_allclose(tower['footprint'], expected, atol=1e-10, rtol=0)
        duplicate = [b for b in output if b['id'] == 'way/772877690']
        self.assertEqual(len(duplicate), 1)
        self.assertEqual(len(duplicate[0]['source_ids']), 4)
        self.assertEqual(duplicate[0]['height'], 6.1)

    def test_repair_rejects_inconsistent_source(self):
        missing_relation = {'elements': [e for e in self.osm['elements']
                           if not (e['type'] == 'relation' and e['id'] == 8814915)]}
        with self.assertRaisesRegex(ValueError, 'relation is missing'):
            builder.reconstruct_buildings(self.data['buildings'], missing_relation, self.data['center'])
        changed = copy.deepcopy(self.data['buildings'])
        duplicates = [b for b in changed if b['id'] == 772877690]
        duplicates[1]['height'] += 1
        with self.assertRaisesRegex(ValueError, 'differ in geometry or height'):
            builder.reconstruct_buildings(changed, self.osm, self.data['center'])

    def test_ground_and_roads(self):
        center = {'lat': 37.563, 'lon': 126.98525}
        transform = from_origin(center['lon'], center['lat'], 0.0001, 0.0001)
        surface = builder.build_surface(np.full((2, 2), 10.0, dtype='float32'), transform, center)
        xmin, zmin, xmax, zmax = surface.bounds.bounds
        footprint = box(xmin + 1, zmin + 1, xmin + 4, zmin + 4)
        low, high = builder.terrain_range_for_polygon(footprint, surface)
        self.assertAlmostEqual(low, 10)
        self.assertAlmostEqual(high, 10)
        with self.assertRaises(ValueError):
            builder.terrain_range_for_polygon(box(xmin - 3, zmin, xmin + 3, zmin + 4), surface)
        road = {'id': 1, 'name': 'test', 'path': [[xmin - 5, (zmin + zmax) / 2], [xmax + 5, (zmin + zmax) / 2]]}
        result = builder.project_roads([road], surface)
        self.assertTrue(result[0]['segments'])
        for segment in result[0]['segments']:
            for x, y, z in segment:
                self.assertAlmostEqual(y, 10.25, places=3)
                self.assertGreaterEqual(x, xmin - 0.001)
                self.assertLessEqual(x, xmax + 0.001)
                self.assertGreaterEqual(z, zmin - 0.001)
                self.assertLessEqual(z, zmax + 0.001)


if __name__ == "__main__":
    unittest.main()
