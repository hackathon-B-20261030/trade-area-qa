import importlib.util
import json
import struct
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('ui_coordinate_audit', ROOT / 'scripts/audit_myeongdong_ui_coordinates.py')
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class CoordinateAuditTest(unittest.TestCase):
    def test_real_buildings_show_a_coordinate_offset(self):
        result = audit.audit(audit.DEFAULT_GLB, audit.DEFAULT_MAP)
        self.assertEqual(result['shared_building_ids'], 663)
        self.assertGreater(result['identity_median_distance_m'], 100)
        self.assertLess(result['center_fit_residual_m']['p95'], 0.1)
        self.assertEqual(result['vertices_count'], 4014)
        self.assertLess(result['vertices_p95_m'], 0.1)
        self.assertLess(result['vertices_max_m'], 0.2)
        self.assertEqual(result['glb_building_ids'], result['shared_building_ids'] + result['glb_only_ids'])
        self.assertEqual(result['map_building_ids'], result['shared_building_ids'] + result['map_only_ids'])

    def test_rejects_invalid_glb(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'bad.glb'
            path.write_bytes(b'not a glb')
            with self.assertRaises(ValueError):
                audit.load_glb(path)

    def test_glb_ground_is_flat_while_source_terrain_varies(self):
        glb, _ = audit.load_glb(audit.DEFAULT_GLB)
        ground = next(node for node in glb['nodes'] if node.get('name') == 'ground')
        position = glb['meshes'][ground['mesh']]['primitives'][0]['attributes']['POSITION']
        accessor = glb['accessors'][position]
        self.assertAlmostEqual(accessor['min'][1], accessor['max'][1], places=6)
        self.assertEqual((ground['matrix'][1], ground['matrix'][9]), (0, 0))
        terrain = json.loads(audit.DEFAULT_MAP.read_text())['terrain']['values']
        heights = [height for row in terrain for height in row if height is not None]
        self.assertGreater(max(heights) - min(heights), 1)

    def test_rejects_insufficient_shared_ids(self):
        glb = {'scene': 0, 'scenes': [{'nodes': [0]}],
               'nodes': [{'extras': {'id': 'way/1'}, 'mesh': 0}],
               'meshes': [{'primitives': [{'attributes': {'POSITION': 0}}]}],
               'accessors': [{'min': [0, 0, 0], 'max': [2, 1, 2]}]}
        body = json.dumps(glb).encode()
        binary = struct.pack('<4sII', b'glTF', 2, 20 + len(body)) + struct.pack('<I4s', len(body), b'JSON') + body
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'map.glb').write_bytes(binary)
            (path / 'map.json').write_text(json.dumps({'buildings': [{'id': 'way/1', 'footprint': [[0, 0], [2, 2]]}]}))
            with self.assertRaisesRegex(ValueError, 'three common'):
                audit.audit(path / 'map.glb', path / 'map.json')

    def test_rejects_hidden_node_transforms_and_duplicate_ids(self):
        base = {'scene': 0, 'scenes': [{'nodes': [0, 1]}],
                'nodes': [{'extras': {'id': 'way/1'}, 'mesh': 0}, {'extras': {'id': 'way/1'}, 'mesh': 0}],
                'meshes': [{'primitives': [{'attributes': {'POSITION': 0}}]}],
                'accessors': [{'min': [0, 0, 0], 'max': [2, 1, 2]}]}
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            audit.model_centers(base)
        base['nodes'] = [{'extras': {'id': 'way/1'}, 'mesh': 0, 'translation': [1, 0, 0]}]
        with self.assertRaisesRegex(ValueError, 'transform'):
            audit.model_centers(base)

    def test_rejects_invalid_vertex_transform(self):
        glb, _ = audit.load_glb(audit.DEFAULT_GLB)
        buildings = {b['id']: b for b in json.loads(audit.DEFAULT_MAP.read_text())['buildings']}
        with self.assertRaisesRegex(ValueError, 'transform'):
            audit.vertex_residuals(audit.DEFAULT_GLB, glb, buildings, (0, 0), (1, 0))


if __name__ == '__main__':
    unittest.main()
