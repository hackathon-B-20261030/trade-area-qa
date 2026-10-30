import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRunGuard, sameOptions } from '../src/engine/manualRun.ts';
import { DEFAULT_RUN_OPTIONS, DEFAULT_BUILDING_OVERRIDE } from '../src/engine/client.ts';

test('unapplied changes compare all global and per-building settings', () => {
  const applied = {...DEFAULT_RUN_OPTIONS, pedestrian_hours:8, discount_pct:10};
  assert.equal(sameOptions(applied, {...applied}), true);
  assert.equal(sameOptions(applied, {...applied, bench_count:10}), false);
  assert.equal(sameOptions(null, applied), false);
  const first = {...applied, building_overrides:{'way/1':{...DEFAULT_BUILDING_OVERRIDE,type:'cafe'}, 'way/2':{...DEFAULT_BUILDING_OVERRIDE,type:'medical'}}};
  const reordered = {...applied, building_overrides:{'way/2':{...DEFAULT_BUILDING_OVERRIDE,type:'medical'}, 'way/1':{...DEFAULT_BUILDING_OVERRIDE,type:'cafe'}}};
  assert.equal(sameOptions(first,reordered), true);
  assert.equal(sameOptions(first, {...reordered, building_overrides:{...reordered.building_overrides,'way/1':{...reordered.building_overrides['way/1'],discount_pct:20}}}),false);
});

test('a request in progress and an active run block duplicate submissions', async () => {
  const guard = createRunGuard();
  let resolveRequest;
  let calls = 0;
  const deferred = new Promise(resolve => { resolveRequest = resolve; });
  const submit = async (busy) => {
    if (!guard.begin(busy)) return;
    try { calls += 1; await deferred; } finally { guard.end(); }
  };
  const first = submit(false);
  await submit(false);
  assert.equal(calls, 1);
  resolveRequest();
  await first;
  await submit(true);
  assert.equal(calls, 1);
  await submit(false);
  assert.equal(calls, 2);
});
