import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { validateStatus, ReplayReader, RequestGate, arrivalsAt } from '../src/engine/replay.ts';

const runPath = process.env.MDSIM_RUN_DIR;
const runTest = runPath ? test : test.skip;

runTest('reads actual Rust Myeongdong frame chunks and rejects tampering', async () => {
  const status = validateStatus(JSON.parse(await readFile(resolve(runPath, 'run_status.json'), 'utf8')));
  assert.equal(status.planned_bodies, 5044);
  assert.equal(status.spawned_bodies, 5044);
  assert.equal(status.status, 'complete');
  assert.equal(status.frames.length, 18);
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    const path = String(url).replace('/runs/test/', '');
    return new Response(await readFile(resolve(runPath, path)));
  };
  try {
    const reader = new ReplayReader('/runs/test');
    const first = await reader.frameAt(status, 0);
    const beforeBoundary = await reader.frameAt(status, 599);
    const afterBoundary = await reader.frameAt(status, 600);
    assert.equal(first.tick, 0);
    assert.equal(beforeBoundary.tick, 1198);
    assert.equal(afterBoundary.tick, 1200);
    assert.ok(beforeBoundary.records.length > 0);
    await assert.rejects(reader.frameAt(status, 10800), /생성되지 않은/);
    const altered = structuredClone(status);
    altered.frames[0].sha256 = '0'.repeat(64);
    await assert.rejects(new ReplayReader('/runs/test').frameAt(altered, 10), /해시/);
  } finally { globalThis.fetch = originalFetch; }
});

test('a newer request invalidates an older asynchronous seek', async () => {
  const gate = new RequestGate();
  let finishFirst;
  const slow = new Promise((resolve) => { finishFirst = resolve; });
  const shown = [];
  const old = gate.issue();
  const first = slow.then(() => { if (gate.current(old)) shown.push('old'); });
  const latest = gate.issue();
  if (gate.current(latest)) shown.push('new');
  finishFirst();
  await first;
  assert.deepEqual(shown, ['new']);
});

test('counts arrivals before the displayed frame tick, not the completed run', () => {
  assert.equal(arrivalsAt(0, 21600, 5044), 0);
  assert.equal(arrivalsAt(7200, 21600, 5044), 1682);
  assert.equal(arrivalsAt(21600, 21600, 5044), 5044);
  assert.throws(() => arrivalsAt(-1, 21600, 5044));
});

runTest('rejects gaps, bad coordinates and wrong completed tick', async () => {
  const original = JSON.parse(await readFile(resolve(runPath, 'run_status.json'), 'utf8'));
  for (const mutate of [
    (x) => { x.frames[1].tick_start += 1; },
    (x) => { x.alignment.vertices_max_m = 1; },
    (x) => { x.completed_tick -= 2; },
    (x) => { x.frames[0].path = '../other'; },
  ]) {
    const status = structuredClone(original);
    mutate(status);
    assert.throws(() => validateStatus(status));
  }
});
