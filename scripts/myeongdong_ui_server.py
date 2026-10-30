#!/usr/bin/env python3
"""Local-only bridge from the Myeongdong UI to the Rust simulation."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import shutil
from typing import Any

import pyarrow as pa
import pyarrow.parquet as pq
import yaml

ROOT = Path(__file__).resolve().parent.parent


from myeongdong_scenarios import prepare_input, sha256, validate_options


def summarize_run(output: Path, input_dir: Path) -> dict[str, Any]:
    """Verify engine records and aggregate real simulation events by mapped building."""
    manifest_path = output / 'manifest.json'
    if not manifest_path.is_file():
        raise BlockingIOError('엔진 결과가 아직 완성되지 않았습니다')
    manifest = json.loads(manifest_path.read_text())
    if manifest.get('status') != 'complete':
        raise BlockingIOError('완료된 엔진 기록만 조회할 수 있습니다')
    input_manifest = json.loads((input_dir / 'bundle/manifest.json').read_text())
    if (manifest['bundle_id'] != input_manifest['bundle_id'] or
            manifest['scenario_sha256'] != input_manifest['scenarios'][0]['sha256']):
        raise ValueError('엔진 결과와 입력 번들이 일치하지 않습니다')
    files = manifest['files']
    for key, expected_path in [('events', 'events.parquet'), ('metrics_10min', 'metrics_10min.parquet')]:
        entry = files[key]
        if entry['path'] != expected_path or sha256(output / expected_path) != entry['sha256']:
            raise ValueError(f'{key} 기록의 해시가 일치하지 않습니다')
    place_path = input_dir / 'input/places.parquet'
    if sha256(place_path) != input_manifest['files']['places'][0]['sha256']:
        raise ValueError('장소 입력의 해시가 일치하지 않습니다')
    places = pq.read_table(place_path, columns=['place_id', 'store_ref']).to_pylist()
    mapping: dict[str, str | None] = {}
    buildings: dict[str, dict[str, Any]] = {}
    for place in places:
        place_id, building_id = place['place_id'], place['store_ref']
        if not place_id or place_id in mapping or (building_id and building_id in buildings):
            raise ValueError('장소와 건물 ID가 중복되거나 누락되었습니다')
        mapping[place_id] = building_id
        if building_id:
            buildings[building_id] = {'enters': 0, 'purchases': 0, 'revenue_krw': 0.0}
    events = pq.read_table(output / 'events.parquet', columns=['kind', 'place_id', 'value']).to_pylist()
    totals: dict[str, Any] = {'arrivals': 0, 'enters': 0, 'purchases': 0, 'revenue_krw': 0.0}
    for event in events:
        kind = event['kind']
        if kind == 'spawn':
            totals['arrivals'] += 1
        elif kind in ('enter', 'purchase'):
            if event['place_id'] not in mapping:
                raise ValueError('사건의 장소가 입력 장소와 일치하지 않습니다')
            building_id = mapping[event['place_id']]
            if kind == 'enter':
                totals['enters'] += 1
                if building_id: buildings[building_id]['enters'] += 1
            else:
                value = event['value']
                if value is None or value < 0:
                    raise ValueError('구매액이 올바르지 않습니다')
                totals['purchases'] += 1
                totals['revenue_krw'] += value
                if building_id:
                    buildings[building_id]['purchases'] += 1
                    buildings[building_id]['revenue_krw'] += value
    rows = pq.read_table(output / 'metrics_10min.parquet', columns=['metric', 'key', 'value']).to_pylist()
    by_metric = {k: 0.0 for k in ('arrivals', 'enters', 'purchases', 'revenue_krw')}
    peak = 0.0
    for row in rows:
        if row['key']:
            continue
        if row['metric'] in by_metric:
            by_metric[row['metric']] += row['value']
        if row['metric'] == 'active_bodies_max':
            peak = max(peak, row['value'])
    for key in by_metric:
        if abs(totals[key] - by_metric[key]) > .01:
            raise ValueError(f'{key} 사건 합계와 10분 지표가 다릅니다')
    if totals['arrivals'] != manifest['counts']['bodies']:
        raise ValueError('입장 사건과 실행 인원 수가 다릅니다')
    totals['peak_active'] = int(peak)
    source = json.loads((input_dir / 'source.json').read_text())
    return {'schema_version': 1, 'run_id': manifest['run_id'], 'window': manifest['window'],
            'totals': totals, 'buildings': buildings,
            'options': source.get('run_options', {'pedestrian_only': False, 'discount_pct': 0}),
            'limitations': source['limitations']}


# The browser communicates only with this loopback service through Vite's proxy.
import argparse
import os
import re
import subprocess
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

RUN_ID = re.compile(r'^ui-[0-9a-f]{32}$')
MAX_BODY = 256 * 1024


def engine_source_sha256(sim_dir: Path) -> str:
    files = [sim_dir / 'Cargo.toml', sim_dir / 'Cargo.lock']
    files.extend(p for p in (sim_dir / 'crates').rglob('*')
                 if p.is_file() and (p.suffix == '.rs' or p.name == 'Cargo.toml'))
    digest = hashlib.sha256()
    for path in sorted(files, key=lambda p: str(p.relative_to(sim_dir))):
        digest.update(str(path.relative_to(sim_dir)).encode() + b'\0')
        with path.open('rb') as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(block)
        digest.update(b'\0')
    return digest.hexdigest()


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    tmp = path.with_suffix('.json.tmp')
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    tmp.replace(path)


class RunManager:
    def __init__(self, base: Path, jobs: Path, outputs: Path, runner: Path,
                 guard: Path | None, expected: int = 5044, sim_dir: Path | None = None):
        self.base, self.jobs, self.outputs, self.runner, self.guard = base, jobs, outputs, runner, guard
        self.expected = expected
        self.sim_dir = sim_dir
        self.lock = threading.Lock()
        self.active: str | None = None
        jobs.mkdir(parents=True, exist_ok=True)
        outputs.mkdir(parents=True, exist_ok=True)
        for path in jobs.glob('ui-*/job.json'):
            try:
                job = json.loads(path.read_text())
                if job.get('state') in ('preparing', 'building', 'queued', 'running'):
                    job.update(state='failed', error='서버가 종료되어 실행 상태를 복구하지 못했습니다')
                    atomic_json(path, job)
            except (OSError, ValueError):
                continue

    def health(self) -> dict[str, Any]:
        try:
            source = json.loads((self.base / 'source.json').read_text())
            if source['profile_count'] != self.expected:
                raise ValueError(f'필요한 인물 수 {self.expected}와 입력이 다릅니다')
            if self.sim_dir is None and not self.runner.is_file():
                raise ValueError('Rust 실행 파일이 없습니다')
            if self.guard is not None and not self.guard.is_file():
                raise ValueError('memguard가 없습니다')
            if self.sim_dir is not None and (not (self.sim_dir / 'Cargo.toml').is_file() or not shutil.which('cargo')):
                raise ValueError('엔진 소스 또는 Cargo가 없습니다')
            return {'ready': True, 'planned_bodies': self.expected}
        except (OSError, ValueError, KeyError) as exc:
            return {'ready': False, 'error': str(exc)}

    def _write(self, run_id: str, **changes: Any) -> dict[str, Any]:
        path = self.jobs / run_id / 'job.json'
        job = json.loads(path.read_text())
        job.update(changes)
        atomic_json(path, job)
        return job

    def start(self, options: Any) -> dict[str, Any]:
        options = validate_options(options)
        health = self.health()
        if not health['ready']:
            raise RuntimeError(health['error'])
        with self.lock:
            if self.active is not None:
                raise BlockingIOError('다른 엔진 실행이 진행 중입니다')
            run_id = f'ui-{uuid.uuid4().hex}'
            jobdir, out = self.jobs / run_id, self.outputs / run_id
            jobdir.mkdir()
            out.mkdir()
            job = {'run_id': run_id, 'state': 'preparing', 'replay_base': f'/runs/{run_id}',
                   'options': options, 'planned_bodies': self.expected, 'error': None}
            atomic_json(jobdir / 'job.json', job)
            self.active = run_id
            threading.Thread(target=self._execute, args=(run_id, options), daemon=True).start()
            return job

    def _execute(self, run_id: str, options: dict[str, Any]) -> None:
        jobdir, out = self.jobs / run_id, self.outputs / run_id
        try:
            prepared = prepare_input(self.base, jobdir / 'input', options)
            env = os.environ.copy()
            env['RAYON_NUM_THREADS'] = '4'
            log_path = jobdir / 'engine.log'
            source_hash = None
            if self.sim_dir is not None:
                self._write(run_id, state='building')
                before = engine_source_sha256(self.sim_dir)
                build = [shutil.which('cargo') or 'cargo', 'build', '--offline', '--locked',
                         '-p', 'mdsim-runtime', '--release', '--example', 'replay_myeongdong_5044', '-j', '2']
                if self.guard is not None:
                    build = [str(self.guard), 'run', '--label', f'myeongdong-build:{run_id}', '--mem-gb', '3', '--', *build]
                with log_path.open('wb') as log:
                    result = subprocess.run(build, cwd=self.sim_dir, stdout=log, stderr=subprocess.STDOUT, env=env)
                if result.returncode:
                    detail = log_path.read_text(errors='replace')[-2000:].strip()
                    raise RuntimeError(f'엔진 빌드 실패 (코드 {result.returncode}): {detail}')
                source_hash = engine_source_sha256(self.sim_dir)
                if before != source_hash:
                    raise RuntimeError('엔진 소스가 빌드 중 변경됐습니다. 다시 실행해 주세요')
            original_hash = sha256(self.runner)
            isolated_runner = jobdir / 'engine.bin'
            shutil.copyfile(self.runner, isolated_runner)
            isolated_runner.chmod(0o755)
            binary_hash = sha256(isolated_runner)
            if original_hash != binary_hash or sha256(self.runner) != binary_hash:
                raise RuntimeError('엔진 실행 파일이 복사 중 변경됐습니다. 다시 실행해 주세요')
            build_info = {'source_sha256': source_hash, 'binary_sha256': binary_hash}
            atomic_json(jobdir / 'engine_build.json', build_info)
            source_path = prepared / 'source.json'
            source = json.loads(source_path.read_text())
            source['engine_build'] = build_info
            atomic_json(source_path, source)
            self._write(run_id, state='queued', engine_build=build_info)
            command = [str(isolated_runner), str(prepared), str(prepared / 'defs'), str(prepared / 'bundle'),
                       str(prepared / 'bundle/scenario.yaml'), str(out), str(self.expected)]
            if self.guard is not None:
                command = [str(self.guard), 'run', '--label', f'myeongdong-ui:{run_id}', '--mem-gb', '3', '--', *command]
            with log_path.open('ab') as log:
                process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, env=env)
                self._write(run_id, state='running')
                exit_code = process.wait()
            if exit_code:
                detail = log_path.read_text(errors='replace')[-2000:].strip()
                raise RuntimeError(f'엔진 실행이 종료 코드 {exit_code}로 실패했습니다: {detail}')
            status = json.loads((out / 'run_status.json').read_text())
            manifest = json.loads((out / 'manifest.json').read_text())
            if status['status'] != 'complete' or status['spawned_bodies'] != self.expected or manifest['status'] != 'complete':
                raise RuntimeError('엔진 종료와 완료 기록이 일치하지 않습니다')
            self._write(run_id, state='complete')
        except Exception as exc:
            self._write(run_id, state='failed', error=str(exc))
        finally:
            with self.lock:
                if self.active == run_id:
                    self.active = None

    def get(self, run_id: str) -> dict[str, Any]:
        if not RUN_ID.fullmatch(run_id):
            raise FileNotFoundError('실행을 찾을 수 없습니다')
        path = self.jobs / run_id / 'job.json'
        if not path.is_file():
            raise FileNotFoundError('실행을 찾을 수 없습니다')
        job = json.loads(path.read_text())
        status_path = self.outputs / run_id / 'run_status.json'
        job['status_available'] = status_path.is_file()
        if status_path.is_file():
            status = json.loads(status_path.read_text())
            job['completed_tick'] = status['completed_tick']
            job['spawned_bodies'] = status['spawned_bodies']
            job['total_ticks'] = round((status['window_end_s'] - status['window_start_s']) / status['dt_s'])
        return job


class ApiHandler(BaseHTTPRequestHandler):
    server: 'ApiServer'

    def _json(self, status: int, payload: dict[str, Any]) -> None:
        data = json.dumps(payload, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json; charset=utf-8')
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('X-Content-Type-Options', 'nosniff')
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:
        if self.path == '/api/buildings':
            try:
                from myeongdong_buildings import building_catalog
                entries = building_catalog(str(self.server.manager.base.resolve()))
                return self._json(200, {'buildings': {k: {'eligible':v['eligible'], 'basis':v['basis'], 'original_store':v['original_store'], 'distance_m':round(v['distance_m'],1)} for k,v in entries.items()}})
            except (OSError, ValueError, KeyError) as exc:
                return self._json(500, {'error':str(exc)})
        match_metrics = re.fullmatch(r'/api/runs/([A-Za-z0-9][A-Za-z0-9._-]{0,80})/metrics', self.path)
        if match_metrics:
            run_id = match_metrics.group(1)
            if '..' in run_id:
                return self._json(404, {'error': '실행을 찾을 수 없습니다'})
            manager = self.server.manager
            out = manager.outputs / run_id
            input_dir = manager.jobs / run_id / 'input' if RUN_ID.fullmatch(run_id) else manager.base
            if not out.is_dir() or out.resolve().parent != manager.outputs.resolve():
                return self._json(404, {'error': '실행을 찾을 수 없습니다'})
            try:
                return self._json(200, summarize_run(out, input_dir))
            except BlockingIOError as exc:
                return self._json(409, {'error': str(exc)})
            except (OSError, ValueError, KeyError) as exc:
                return self._json(500, {'error': str(exc)})
        if self.path == '/api/health':
            return self._json(200, self.server.manager.health())
        match = re.fullmatch(r'/api/runs/(ui-[0-9a-f]{32})', self.path)
        if match:
            try:
                return self._json(200, self.server.manager.get(match.group(1)))
            except FileNotFoundError as exc:
                return self._json(404, {'error': str(exc)})
        self._json(404, {'error': '요청 경로가 없습니다'})

    def do_POST(self) -> None:
        if self.path != '/api/runs':
            return self._json(404, {'error': '요청 경로가 없습니다'})
        if self.headers.get('Origin') != 'http://127.0.0.1:5173':
            return self._json(403, {'error': '로컬 UI에서만 실행할 수 있습니다'})
        if self.headers.get_content_type() != 'application/json':
            return self._json(400, {'error': 'JSON 요청이 필요합니다'})
        try:
            size = int(self.headers.get('Content-Length', '0'))
            if size <= 0 or size > MAX_BODY:
                raise ValueError('요청 크기가 올바르지 않습니다')
            options = json.loads(self.rfile.read(size))
            return self._json(202, self.server.manager.start(options))
        except BlockingIOError as exc:
            return self._json(409, {'error': str(exc)})
        except (ValueError, json.JSONDecodeError) as exc:
            return self._json(400, {'error': str(exc)})
        except (RuntimeError, OSError) as exc:
            return self._json(500, {'error': str(exc)})


class ApiServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], manager: RunManager):
        self.manager = manager
        super().__init__(address, ApiHandler)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True, help='prepared 5044-person input directory')
    parser.add_argument('--port', type=int, default=5174)
    parser.add_argument('--memguard', type=Path, default=Path.home() / 'memguard/bin/memguard',
                        help='path to the memguard executable')
    args = parser.parse_args()
    manager = RunManager(args.input.resolve(), ROOT / 'sim/runs/ui_jobs', ROOT / 'apps/myeongdong-ui/public/runs',
                         ROOT / 'sim/target/release/examples/replay_myeongdong_5044', args.memguard, sim_dir=ROOT / 'sim')
    server = ApiServer(('127.0.0.1', args.port), manager)
    print(f'Myeongdong local engine API: http://127.0.0.1:{args.port}', flush=True)
    server.serve_forever()


if __name__ == '__main__':
    main()
