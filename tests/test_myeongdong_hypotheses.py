from pathlib import Path
import json
import subprocess
import sys
import tempfile
import unittest
import pyarrow.parquet as pq
import yaml

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
import myeongdong_hypotheses as h
from myeongdong_scenarios import prepare_input, sha256

class HypothesisInputTests(unittest.TestCase):
    def test_each_candidate_changes_exactly_one_setting(self):
        baseline=h.options_for('baseline')
        self.assertEqual(h.changed_keys(baseline,baseline),[])
        for case in h.CASES:
            if case!='baseline': self.assertEqual(len(h.changed_keys(baseline,h.options_for(case))),1,case)
        with self.assertRaises(ValueError): h.options_for('missing')

    def test_difference_handles_zero_and_direction(self):
        self.assertEqual(h.difference(0,4),{'before':0,'after':4,'delta':4,'pct':None})
        self.assertEqual(h.difference(100,80)['pct'],-20)

    def test_seed_and_verified_existing_income_bundle(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-hyp-test-') as tmp:
            dest=Path(tmp)/'input'; prepare_input(h.BASE,dest,h.options_for('baseline'))
            h.restore_income_110(h.BASE,dest); h.update_seed(dest,h.SEEDS[1])
            manifest=json.loads((dest/'bundle/manifest.json').read_text())
            scenario=yaml.safe_load((dest/'bundle/scenario.yaml').read_text())
            source=json.loads((dest/'source.json').read_text())
            self.assertEqual((scenario['seed'],manifest['scenarios'][0]['seed']),(h.SEEDS[1],h.SEEDS[1]))
            self.assertEqual(manifest['scenarios'][0]['sha256'],sha256(dest/'bundle/scenario.yaml'))
            self.assertEqual(source['run_options']['income_multiplier'],1.1)
            self.assertEqual(source['income_profile_source']['run_id'],h.REFERENCE_ID)
            for role in ('profiles','profile_attrs'):
                entry=manifest['files'][role][0]
                self.assertEqual(entry['rows'],5044)
                self.assertEqual(entry['sha256'],sha256(dest/entry['path']))
                self.assertEqual(entry['rows'],pq.read_metadata(dest/entry['path']).num_rows)
            checked=subprocess.run([str(h.CLI),'validate','--root',str(dest),'--defs',str(dest/'defs'),'--bundle',str(dest/'bundle'),'--scenario',str(dest/'bundle/scenario.yaml')],text=True,capture_output=True)
            self.assertEqual(checked.returncode,0,checked.stderr)
            with self.assertRaises(ValueError): h.update_seed(dest,2**64)

if __name__=='__main__':unittest.main()

class HypothesisClassificationTests(unittest.TestCase):
    def test_followup_requires_same_engine_input_and_distinct_keys(self):
        from report_myeongdong_hypotheses import combine_batches
        primary={'engine':'e','base_source_sha256':'x','reference_job':'r','seeds':[1,2],'cases':{},'runs':{'0:baseline':1}}
        second={**primary,'runs':{'1:baseline':2}}
        self.assertEqual(len(combine_batches(primary,second)['runs']),2)
        with self.assertRaises(ValueError): combine_batches(primary,{**second,'engine':'other'})
        with self.assertRaises(ValueError): combine_batches(primary,{**second,'runs':{'0:baseline':2}})

    def test_repeat_direction_and_low_counts(self):
        from report_myeongdong_hypotheses import classification
        good=[{'before':20,'after':30,'delta':10},{'before':30,'after':36,'delta':6}]
        self.assertEqual(classification('discount',good)[0],'candidate')
        self.assertEqual(classification('discount',good[:1])[0],'not_replicated')
        self.assertEqual(classification('discount',[good[0],{'before':30,'after':20,'delta':-10}])[0],'mixed')
        self.assertEqual(classification('bench',[{'before':0,'after':6,'delta':6}]*2)[0],'sparse')
        self.assertEqual(classification('discount',[{'before':20,'after':10,'delta':-10}]*2)[0],'opposite')
        self.assertEqual(classification('building',[{'before':20,'after':10,'delta':-10}]*2)[0],'candidate')
