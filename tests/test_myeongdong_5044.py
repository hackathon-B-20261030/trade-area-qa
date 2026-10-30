import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
spec=importlib.util.spec_from_file_location('real_replay',ROOT/'scripts/prepare_myeongdong_5044.py')
replay=importlib.util.module_from_spec(spec);spec.loader.exec_module(replay)


class DirectInputTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(prefix='myeongdong-direct-test-')
        cls.root=Path(cls.tmp.name);cls.fixture=cls.root/'fixture';cls.input=cls.root/'input'
        cls.bin=ROOT/'sim/target/release/mdsim'
        subprocess.run([str(cls.bin),'fixture','--out',str(cls.fixture)],check=True,capture_output=True,text=True)
        result=subprocess.run([sys.executable,str(ROOT/'scripts/prepare_myeongdong_5044.py'),'--out',str(cls.input),'--count','12','--fixture-root',str(cls.fixture)],capture_output=True,text=True)
        if result.returncode:raise AssertionError(result.stderr)
        cls.result=json.loads(result.stdout)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_select_unique_deterministic(self):
        table=pa.Table.from_pylist([{'synthetic_id':str(i)} for i in range(20)])
        ids=lambda rows:[x['synthetic_id'] for x in rows]
        self.assertEqual(ids(replay.select_profiles(table,7)),ids(replay.select_profiles(table,7)))
        self.assertEqual(len(set(ids(replay.select_profiles(table,7)))),7)
        with self.assertRaises(ValueError):replay.select_profiles(table,21)
        with self.assertRaises(ValueError):replay.select_profiles(pa.Table.from_pylist([{'synthetic_id':'x'},{'synthetic_id':'x'}]),1)

    def test_profile_age_and_budget(self):
        base={'synthetic_id':'sample','age':30,'sex':'여자','occupation':'무직','district':'서울-중구','family_type':'혼자 거주'}
        profile,attrs=replay.profile_from_row(base,replay.income_spec())
        self.assertEqual((profile['cap_money_comfort'],profile['cap_money_limit']),(64000,218000))
        self.assertEqual(profile['cap_climb_comfort'],8.)
        self.assertEqual(profile['cap_way_comfort'],.8)
        self.assertNotIn('age',profile);self.assertNotIn('sex',profile)
        self.assertEqual(attrs['income_label'],'assumption')
        old={**base,'age':75}; profile,_=replay.profile_from_row(old,replay.income_spec())
        self.assertAlmostEqual(profile['cap_climb_comfort'],5.3)
        self.assertAlmostEqual(profile['base_speed_mps'],1.34*.85)

    def test_network_refs_and_real_buildings(self):
        read=lambda name:pq.read_table(self.input/'input'/f'{name}.parquet').to_pylist()
        nodes={n['node_id'] for n in read('nodes')};edges={e['edge_id'] for e in read('edges')}
        self.assertGreater(len(nodes),500);self.assertGreater(len(edges),500)
        self.assertTrue(all(e['a'] in nodes and e['b'] in nodes for e in read('edges')))
        self.assertTrue(all(p['edge_id'] in edges for p in read('places')))
        self.assertTrue(all(p['node_id'] in nodes for p in read('portals')))
        self.assertEqual(len(read('buildings')),709)
        self.assertEqual(self.result['space']['common_business_buildings'],663)
        self.assertGreater(len(read('places')),100)
        self.assertLessEqual(self.result['space']['portal_max_snap_m'],30)

    def test_selected_input_unique_and_labeled(self):
        t=pq.read_table(self.input/'input/profiles.parquet'); rows=t.to_pylist()
        ids=[r['persona_id'] for r in rows]
        self.assertEqual(len(ids),12);self.assertEqual(ids,sorted(set(ids)))
        source=json.loads((self.input/'source.json').read_text())
        self.assertEqual(source['profile_count'],12)
        manifest=json.loads((self.input/'bundle/manifest.json').read_text())
        self.assertIn('forced_arrivals',manifest['assumption_flags'])
        self.assertIn('development_fixture_definitions',manifest['assumption_flags'])

    def test_bundle_passes_rust_contract(self):
        command=[str(self.bin),'validate','--root',str(self.input),'--defs',str(self.input/'defs'),'--bundle',str(self.input/'bundle'),'--scenario',str(self.input/'bundle/scenario.yaml')]
        result=subprocess.run(command,capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)
        info=json.loads(result.stdout)
        self.assertEqual(info['profiles'],12)
        self.assertEqual(info['places_excluded'],0)
        self.assertGreater(info['portals'],0)


if __name__=='__main__':unittest.main()
