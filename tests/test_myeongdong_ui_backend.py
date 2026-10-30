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
from myeongdong_ui_server import prepare_input, sha256, validate_options


class InputBundleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.base = Path('/private/tmp/mdsim-5044-input-path').read_text().strip()
        cls.base = Path(cls.base)
        cls.cli = ROOT / 'sim/target/release/mdsim'

    def test_option_validation(self):
        self.assertEqual(validate_options({'pedestrian_only': True, 'discount_pct': 10})['discount_pct'], 10)
        for options in ({}, {'pedestrian_only': 1, 'discount_pct': 10},
                        {'pedestrian_only': False, 'discount_pct': 11},
                        {'pedestrian_only': False, 'discount_pct': True},
                        {'pedestrian_only': False, 'discount_pct': 0, 'cmd': 'x'}):
            with self.assertRaises(ValueError):
                validate_options(options)

    def test_isolated_bundle_options_and_rust_validate(self):
        original_places = sha256(self.base / 'input/places.parquet')
        original_scenario = sha256(self.base / 'bundle/scenario.yaml')
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-bundle-') as tmp:
            dest = Path(tmp) / 'input'
            prepare_input(self.base, dest, {'pedestrian_only': True, 'discount_pct': 10})
            scenario = yaml.safe_load((dest / 'bundle/scenario.yaml').read_text())
            self.assertEqual(scenario['pedestrian_only'], [{'start': '12:00', 'end': '20:00'}])
            self.assertEqual(set(pq.read_table(dest / 'input/places.parquet', columns=['discount_pct']).column(0).to_pylist()), {10.0})
            manifest = json.loads((dest / 'bundle/manifest.json').read_text())
            self.assertEqual(manifest['files']['places'][0]['sha256'], sha256(dest / 'input/places.parquet'))
            self.assertEqual(manifest['scenarios'][0]['sha256'], sha256(dest / 'bundle/scenario.yaml'))
            self.assertEqual(manifest['files']['profiles'][0]['rows'], 5044)
            checked = subprocess.run([str(self.cli), 'validate', '--root', str(dest), '--defs', str(dest / 'defs'), '--bundle', str(dest / 'bundle'), '--scenario', str(dest / 'bundle/scenario.yaml')], capture_output=True, text=True)
            self.assertEqual(checked.returncode, 0, checked.stderr)
            self.assertEqual(sha256(self.base / 'input/places.parquet'), original_places)
            self.assertEqual(sha256(self.base / 'bundle/scenario.yaml'), original_scenario)
            with self.assertRaises(ValueError):
                prepare_input(self.base, dest, {'pedestrian_only': False, 'discount_pct': 0})

    def test_default_condition_is_not_pedestrian_only(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-default-') as tmp:
            dest = Path(tmp) / 'input'
            prepare_input(self.base, dest, {'pedestrian_only': False, 'discount_pct': 0})
            self.assertEqual(yaml.safe_load((dest / 'bundle/scenario.yaml').read_text())['pedestrian_only'], [])
            self.assertEqual(set(pq.read_table(dest / 'input/places.parquet', columns=['discount_pct']).column(0).to_pylist()), {0.0})

class RunManagerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from myeongdong_ui_server import RunManager
        cls.RunManager = RunManager
        cls.base = Path(Path('/private/tmp/mdsim-real-small-path').read_text().strip())
        cls.runner = ROOT / 'sim/target/release/examples/replay_myeongdong_5044'

    def make_manager(self, tmp, runner=None):
        return self.RunManager(self.base, Path(tmp) / 'jobs', Path(tmp) / 'out',
                               runner or self.runner, None, expected=12)

    def wait_job(self, manager, run_id, timeout=50):
        import time
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            result = manager.get(run_id)
            if result['state'] in ('complete', 'failed'):
                return result
            time.sleep(.1)
        self.fail('engine did not finish in time')

    def test_small_actual_engine(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-real-') as tmp:
            manager = self.make_manager(tmp)
            job = manager.start({'pedestrian_only': True, 'discount_pct': 10})
            result = self.wait_job(manager, job['run_id'])
            self.assertEqual(result['state'], 'complete', result.get('error'))
            self.assertEqual(result['spawned_bodies'], 12)
            self.assertEqual(result['options']['pedestrian_hours'], 8)
            self.assertEqual(result['options']['discount_pct'], 10)
            out = Path(tmp) / 'out' / job['run_id']
            status = json.loads((out / 'run_status.json').read_text())
            self.assertEqual(status['provenance']['run_options'], result['options'])
            self.assertEqual(len(status['frames']), 18)
            checked = subprocess.run([str(ROOT / 'sim/target/release/mdsim'), 'verify', '--run', str(out)], capture_output=True, text=True)
            self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_current_source_build_and_binary_provenance(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-build-') as tmp:
            manager = self.RunManager(self.base, Path(tmp) / 'jobs', Path(tmp) / 'out',
                                      self.runner, None, expected=12, sim_dir=ROOT / 'sim')
            job = manager.start({'pedestrian_only': True, 'discount_pct': 10})
            result = self.wait_job(manager, job['run_id'], timeout=50)
            self.assertEqual(result['state'], 'complete', result.get('error'))
            provenance = result['engine_build']
            self.assertEqual(len(provenance['source_sha256']), 64)
            self.assertEqual(len(provenance['binary_sha256']), 64)
            self.assertEqual(sha256(Path(tmp) / 'jobs' / job['run_id'] / 'engine.bin'), provenance['binary_sha256'])
            status = json.loads((Path(tmp) / 'out' / job['run_id'] / 'run_status.json').read_text())
            self.assertEqual(status['provenance']['engine_build'], provenance)

    def test_build_rejects_changing_source(self):
        from unittest.mock import patch
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-build-race-') as tmp:
            manager = self.RunManager(self.base, Path(tmp) / 'jobs', Path(tmp) / 'out',
                                      self.runner, None, expected=12, sim_dir=ROOT / 'sim')
            with patch('myeongdong_ui_server.engine_source_sha256', side_effect=['before', 'after']), \
                 patch('myeongdong_ui_server.subprocess.run', return_value=SimpleNamespace(returncode=0)):
                job = manager.start({'pedestrian_only': False, 'discount_pct': 0})
                result = self.wait_job(manager, job['run_id'])
            self.assertEqual(result['state'], 'failed')
            self.assertIn('빌드 중 변경', result['error'])
            self.assertFalse((Path(tmp) / 'out' / job['run_id'] / 'run_status.json').exists())

    def test_failure_status(self):
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-fail-') as tmp:
            manager = self.make_manager(tmp, Path('/usr/bin/false'))
            job = manager.start({'pedestrian_only': False, 'discount_pct': 0})
            result = self.wait_job(manager, job['run_id'])
            self.assertEqual(result['state'], 'failed')
            self.assertIn('실패', result['error'])

    def test_one_job_at_a_time(self):
        import threading
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-lock-') as tmp:
            manager = self.make_manager(tmp)
            release = threading.Event()
            def paused(run_id, options):
                release.wait(5)
                with manager.lock:
                    manager.active = None
            manager._execute = paused
            manager.start({'pedestrian_only': False, 'discount_pct': 0})
            with self.assertRaises(BlockingIOError):
                manager.start({'pedestrian_only': False, 'discount_pct': 0})
            release.set()

class SummaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from myeongdong_ui_server import summarize_run
        cls.summarize_run = staticmethod(summarize_run)
        cls.base = Path(Path('/private/tmp/mdsim-5044-input-path').read_text().strip())
        cls.run_dir = Path(Path('/private/tmp/mdsim-5044-run-path').read_text().strip())

    def test_actual_events_match_metrics_and_buildings(self):
        result = self.summarize_run(self.run_dir, self.base)
        self.assertEqual(result['totals']['arrivals'], 5044)
        self.assertEqual(result['totals']['enters'], 477)
        self.assertEqual(result['totals']['purchases'], 64)
        self.assertEqual(result['totals']['revenue_krw'], 1597000)
        self.assertEqual(len(result['buildings']), 161)
        self.assertEqual(result['buildings']['way/60839164'], {'enters': 3, 'purchases': 1, 'revenue_krw': 50000.0})
        self.assertNotIn('unmodeled-building', result['buildings'])

    def test_hash_tamper_and_incomplete_rejected(self):
        import shutil
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-summary-') as tmp:
            dest = Path(tmp)
            for name in ('manifest.json', 'metrics_10min.parquet', 'events.parquet'):
                shutil.copy2(self.run_dir / name, dest / name)
            with (dest / 'events.parquet').open('ab') as stream:
                stream.write(b'tamper')
            with self.assertRaisesRegex(ValueError, '해시'):
                self.summarize_run(dest, self.base)
        with tempfile.TemporaryDirectory(prefix='mdsim-ui-incomplete-') as tmp:
            with self.assertRaises(BlockingIOError):
                self.summarize_run(Path(tmp), self.base)


if __name__ == '__main__':
    unittest.main()
