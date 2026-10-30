from __future__ import annotations
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
import pyarrow.parquet as pq

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from myeongdong_buildings import building_catalog
from myeongdong_scenarios import prepare_input


class BuildingOverridesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.base=Path(Path('/private/tmp/mdsim-5044-input-path').read_text().strip()).resolve()
        cls.catalog=building_catalog(str(cls.base))
        cls.known='way/60839164'
        cls.estimated=next(k for k,v in cls.catalog.items() if v['eligible'] and v['basis']=='glb_center_assumed')
        cls.outside=next(k for k,v in cls.catalog.items() if not v['eligible'])

    def test_catalog_alignment_and_limits(self):
        self.assertEqual(len(self.catalog),1252)
        self.assertTrue(self.catalog[self.known]['eligible'])
        self.assertEqual(self.catalog[self.known]['basis'],'reference_map')
        self.assertTrue(self.catalog[self.estimated]['eligible'])
        self.assertGreater(self.catalog[self.outside]['distance_m'],80)

    def test_two_individual_types_and_medical_definition(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-buildings-') as tmp:
            dest=Path(tmp)/'input'
            options={'pedestrian_hours':0,'discount_pct':10,'price_multiplier':1.1,'hours_extension':1,
                     'building_overrides':{
                         self.known:{'type':'cafe','discount_pct':20,'price_multiplier':.9,'hours_extension':2},
                         self.estimated:{'type':'medical','discount_pct':None,'price_multiplier':None,'hours_extension':None},
                     }}
            prepare_input(self.base,dest,options)
            places=pq.read_table(dest/'input/places.parquet').to_pylist()
            store=next(p for p in places if p['store_ref']==self.known)
            medical=next(p for p in places if p['store_ref']==self.estimated)
            self.assertEqual((store['class'],store['category']),('cafe','cafe'))
            self.assertEqual(store['discount_pct'],20)
            self.assertAlmostEqual(store['price_level_krw'],6300)
            self.assertEqual(store['close_min'],1440)
            self.assertEqual((medical['class'],medical['category']),('medical','medical'))
            self.assertEqual(medical['price_level_krw'],0)
            other=next(p for p in places if p['store_ref'] not in (None,self.known,self.estimated))
            self.assertEqual(other['discount_pct'],10)
            source=json.loads((dest/'source.json').read_text())
            self.assertIn(self.estimated,source['glb_center_assumed_buildings'])
            manifest=json.loads((dest/'bundle/manifest.json').read_text())
            self.assertIn('medical_generic_visit',manifest['assumption_flags'])
            cli=ROOT/'sim/target/release/mdsim'
            result=subprocess.run([str(cli),'validate','--root',str(dest),'--defs',str(dest/'defs'),'--bundle',str(dest/'bundle'),'--scenario',str(dest/'bundle/scenario.yaml')],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr)
            validation=json.loads(result.stdout)
            self.assertEqual(validation['places_excluded'],0)

    def test_building_settings_run_in_actual_engine(self):
        from myeongdong_ui_server import RunManager, summarize_run
        import time
        small=Path(Path('/private/tmp/mdsim-real-small-path').read_text().strip())
        with tempfile.TemporaryDirectory(prefix='mdsim-buildings-run-') as tmp:
            manager=RunManager(small,Path(tmp)/'jobs',Path(tmp)/'out',
                ROOT/'sim/target/release/examples/replay_myeongdong_5044',None,expected=12)
            options={'discount_pct':10,'building_overrides':{
                self.known:{'type':'cafe','discount_pct':20},
                self.estimated:{'type':'medical'},
            }}
            job=manager.start(options)
            deadline=time.monotonic()+50
            while time.monotonic()<deadline:
                result=manager.get(job['run_id'])
                if result['state'] in ('complete','failed'): break
                time.sleep(.1)
            self.assertEqual(result['state'],'complete',result.get('error'))
            self.assertEqual(result['spawned_bodies'],12)
            out=Path(tmp)/'out'/job['run_id']
            bundle=Path(tmp)/'jobs'/job['run_id']/'input'
            summary=summarize_run(out,bundle)
            self.assertEqual(summary['totals']['arrivals'],12)
            self.assertIn(self.known,summary['buildings'])
            self.assertIn(self.estimated,summary['buildings'])

    def test_invalid_or_outside_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-building-outside-') as tmp:
            with self.assertRaisesRegex(ValueError,'보행망'):
                prepare_input(self.base,Path(tmp)/'input',{'pedestrian_hours':0,'building_overrides':{self.outside:{'type':'cafe'}}})
        with tempfile.TemporaryDirectory(prefix='mdsim-building-unsafe-') as tmp:
            with self.assertRaises(ValueError):
                prepare_input(self.base,Path(tmp)/'input',{'pedestrian_hours':0,'building_overrides':{'../../etc':{'type':'cafe'}}})

if __name__=='__main__':unittest.main()
