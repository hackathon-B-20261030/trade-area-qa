from __future__ import annotations

import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

import pyarrow.parquet as pq
import yaml

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from myeongdong_ui_server import sha256, summarize_run


@unittest.skipUnless(os.environ.get('MDSIM_UI_JOB'), 'Set MDSIM_UI_JOB to a completed UI run')
class FullEngineRunTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.run_id = os.environ['MDSIM_UI_JOB']
        cls.job = ROOT / 'sim/runs/ui_jobs' / cls.run_id
        cls.input_dir = cls.job / 'input'
        cls.output_dir = ROOT / 'apps/myeongdong-ui/public/runs' / cls.run_id
        cls.baseline = Path(Path('/private/tmp/mdsim-5044-run-path').read_text().strip())

    def test_full_run_provenance_counts_and_hashes(self):
        job = json.loads((self.job / 'job.json').read_text())
        status = json.loads((self.output_dir / 'run_status.json').read_text())
        self.assertEqual(job['state'], 'complete')
        self.assertEqual(job['options'], {'pedestrian_only': True, 'discount_pct': 10})
        self.assertEqual(status['spawned_bodies'], 5044)
        self.assertEqual(len(status['frames']), 18)
        self.assertEqual(status['provenance']['engine_build'], job['engine_build'])
        self.assertEqual(sha256(self.job / 'engine.bin'), job['engine_build']['binary_sha256'])
        self.assertEqual(len(job['engine_build']['source_sha256']), 64)
        agents = pq.read_table(self.output_dir / 'agents.parquet', columns=['persona_id']).column(0).to_pylist()
        self.assertEqual(len(agents), 5044)
        self.assertEqual(len(set(agents)), 5044)
        checked = subprocess.run([str(ROOT / 'sim/target/release/mdsim'), 'verify', '--run', str(self.output_dir)], capture_output=True, text=True)
        self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_selected_conditions_reach_engine_records(self):
        scenario = yaml.safe_load((self.input_dir / 'bundle/scenario.yaml').read_text())
        self.assertEqual(scenario['pedestrian_only'], [{'start':'14:00','end':'17:00'}])
        places = pq.read_table(self.input_dir / 'input/places.parquet', columns=['place_id','price_level_krw','discount_pct']).to_pylist()
        self.assertEqual({p['discount_pct'] for p in places}, {10.0})
        price = {p['place_id']: p['price_level_krw'] * .9 for p in places}
        events = pq.read_table(self.output_dir / 'events.parquet', columns=['kind','place_id','value']).to_pylist()
        purchases = [e for e in events if e['kind'] == 'purchase']
        self.assertGreater(len(purchases), 0)
        self.assertTrue(all(any(abs(e['value'] - price[e['place_id']] * mult) < .01 for mult in (.6, 1., 1.8)) for e in purchases))
        edges = pq.read_table(self.input_dir / 'input/edges.parquet', columns=['edge_id','ped_zone']).to_pylist()
        ped_ids = {e['edge_id'] for e in edges if e['ped_zone']}
        def first_area(directory):
            rows = pq.read_table(directory / 'metrics_10min.parquet', columns=['frame_start_min','metric','key','value']).to_pylist()
            return {r['key']:r['value'] for r in rows if r['frame_start_min'] == 840 and r['metric'] == 'edge_effective_area_m2'}
        baseline, altered = first_area(self.baseline), first_area(self.output_dir)
        self.assertEqual(len(ped_ids), 135)
        self.assertTrue(all(altered[k] > baseline[k] for k in ped_ids))
        result = summarize_run(self.output_dir, self.input_dir)
        self.assertEqual(result['totals']['purchases'], len(purchases))
        self.assertAlmostEqual(result['totals']['revenue_krw'], sum(e['value'] for e in purchases))


if __name__ == '__main__':
    unittest.main()
