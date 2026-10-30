from __future__ import annotations
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
import pyarrow.parquet as pq
import yaml

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from myeongdong_scenarios import DEFAULTS, prepare_input, sha256, validate_options
from prepare_myeongdong_5044 import PERSONAS, estimate_income, income_spec


class ScenarioInputsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.base = Path(Path('/private/tmp/mdsim-5044-input-path').read_text().strip())
        cls.options = {**DEFAULTS, 'pedestrian_hours':4, 'width_delta_m':.5, 'stall_scale':1.2,
                       'bench_count':20, 'discount_pct':20, 'hours_extension':1,
                       'income_multiplier':1.1, 'price_multiplier':.9, 'visibility_level':'low'}

    def test_nine_variables_and_rust_contract(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-variables-') as tmp:
            dest = Path(tmp) / 'bundle'
            hashes = {f:sha256(self.base/'input'/f) for f in ['edges.parquet','places.parquet','profiles.parquet','profile_attrs.parquet']}
            prepare_input(self.base, dest, self.options)
            scenario = yaml.safe_load((dest/'bundle/scenario.yaml').read_text())
            self.assertEqual(scenario['pedestrian_only'], [{'start':'12:00','end':'16:00'}])
            edge_old = {x['edge_id']:x for x in pq.read_table(self.base/'input/edges.parquet').to_pylist()}
            edges = pq.read_table(dest/'input/edges.parquet').to_pylist()
            target = next(e for e in edges if e['inside_boundary'] and e['ped_zone'] and e['walk_mode']=='full')
            self.assertAlmostEqual(target['width_m'], edge_old[target['edge_id']]['width_m']+.5)
            self.assertAlmostEqual(target['ad_intensity'], max(0, edge_old[target['edge_id']]['ad_intensity']-.2))
            places = pq.read_table(dest/'input/places.parquet').to_pylist()
            stalls = [p for p in places if p['class']=='stall']
            benches = [p for p in places if p['class']=='rest_facility']
            self.assertEqual(len(stalls),48)
            self.assertEqual(len(benches),20)
            self.assertEqual(sum(p['offer_souvenir'] > 0 for p in stalls),12)
            self.assertTrue(all(p['open_min']==960 and p['close_min']==1440 and p['discount_pct']==20 for p in stalls))
            self.assertTrue(all(p['seats']==3 for p in benches))
            old_store = pq.read_table(self.base/'input/places.parquet').to_pylist()[0]
            new_store = next(p for p in places if p['place_id']==old_store['place_id'])
            self.assertEqual(new_store['discount_pct'],20)
            self.assertAlmostEqual(new_store['price_level_krw'],old_store['price_level_krw']*.9)
            self.assertEqual(new_store['close_min'], min(2880,old_store['close_min']+60))
            self.assertAlmostEqual(new_store['visibility'], max(0,old_store['visibility']-.2))
            old_prof = {r['persona_id']:r for r in pq.read_table(self.base/'input/profiles.parquet').to_pylist()}
            profs = pq.read_table(dest/'input/profiles.parquet').to_pylist()
            attrs = pq.read_table(self.base/'input/profile_attrs.parquet').to_pylist()
            attr_by_id = {a['persona_id']:a for a in attrs}
            original = {r['synthetic_id']:r for r in pq.read_table(PERSONAS,columns=['synthetic_id','age','sex','occupation']).to_pylist()}
            spec=income_spec()
            earner = next(p for p in profs if estimate_income(original[attr_by_id[p['persona_id']]['source_ref']], spec)[2] is None)
            non_earner = next(p for p in profs if estimate_income(original[attr_by_id[p['persona_id']]['source_ref']], spec)[2] is not None)
            self.assertGreater(earner['cap_money_limit'],old_prof[earner['persona_id']]['cap_money_limit'])
            self.assertEqual(non_earner['cap_money_limit'],old_prof[non_earner['persona_id']]['cap_money_limit'])
            self.assertEqual(len(profs),5044)
            self.assertEqual(len({p['persona_id'] for p in profs}),5044)
            manifest=json.loads((dest/'bundle/manifest.json').read_text())
            for role in ['edges','places','profiles','profile_attrs']:
                entry=manifest['files'][role][0]
                self.assertEqual(entry['sha256'],sha256(dest/entry['path']))
                self.assertEqual(entry['rows'],pq.read_metadata(dest/entry['path']).num_rows)
            cli=ROOT/'sim/target/release/mdsim'
            result=subprocess.run([str(cli),'validate','--root',str(dest),'--defs',str(dest/'defs'),'--bundle',str(dest/'bundle'),'--scenario',str(dest/'bundle/scenario.yaml')],text=True,capture_output=True)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(hashes,{f:sha256(self.base/'input'/f) for f in hashes})

    def test_invalid_values_are_rejected(self):
        for key, value in [('pedestrian_hours',1),('width_delta_m',-1),('stall_scale',2),('bench_count',11),('discount_pct',True),('hours_extension',4),('income_multiplier',0),('price_multiplier',2),('visibility_level','unknown')]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_options({**DEFAULTS,key:value})

    def test_facility_positions_repeat(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-repeat-') as tmp:
            a,b=Path(tmp)/'a',Path(tmp)/'b'
            prepare_input(self.base,a,{'pedestrian_hours':0})
            prepare_input(self.base,b,{'pedestrian_hours':0})
            def facilities(path):
                return [p for p in pq.read_table(path/'input/places.parquet').to_pylist() if p['class'] in ('stall','rest_facility')]
            self.assertEqual(facilities(a),facilities(b))
            self.assertEqual(len(facilities(a)),40)

if __name__=='__main__': unittest.main()
