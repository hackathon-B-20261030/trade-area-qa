import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startRun, getJob, getMetrics, normalizeOptions, DEFAULT_RUN_OPTIONS } from '../src/engine/client.ts';
import { CONTROL_BINDINGS, scenarioVariables, supportedVariables } from '../src/engine/controls.ts';

test('run request sends the selected engine settings and exposes server failures', async () => {
  const original = globalThis.fetch;
  const calls = [];
  const options = {...DEFAULT_RUN_OPTIONS, pedestrian_hours:8, discount_pct:10};
  globalThis.fetch = async (url, init) => {
    calls.push({url, init});
    return new Response(JSON.stringify({run_id:'ui-0123456789abcdef0123456789abcdef', state:'preparing', options}), {status:202});
  };
  try {
    const job = await startRun(options);
    assert.equal(job.state, 'preparing');
    assert.equal(job.legacy_options, false);
    assert.equal(calls[0].url, '/api/runs');
    assert.equal(calls[0].init.method, 'POST');
    assert.deepEqual(JSON.parse(calls[0].init.body), options);
    globalThis.fetch = async () => new Response(JSON.stringify({error:'다른 엔진 실행이 진행 중입니다'}), {status:409});
    await assert.rejects(startRun(options), /진행 중/);
  } finally { globalThis.fetch = original; }
});
test('old options load without representing the new baseline as an old result', async () => {
  const result=normalizeOptions({pedestrian_only:true,discount_pct:20});
  assert.equal(result.pedestrian_hours,8);
  assert.equal(result.discount_pct,20);
  assert.equal(result.stall_scale,1);
  const original=globalThis.fetch;
  globalThis.fetch=async()=>new Response(JSON.stringify({options:{pedestrian_only:true,discount_pct:20}}));
  try { assert.equal((await getJob('ui-0123456789abcdef0123456789abcdef')).legacy_options,true); }
  finally { globalThis.fetch=original; }
});
test('all original controls are present and only supported ones are enabled', () => {
  assert.equal(scenarioVariables.length,16);
  assert.equal(supportedVariables.length,9);
  assert.ok(supportedVariables.every(v => 'key' in CONTROL_BINDINGS[v.id]));
  assert.ok(!supportedVariables.some(v => [7,8,9,10,11,13,14].includes(v.id)));
  assert.ok(scenarioVariables.filter(v => 'reason' in CONTROL_BINDINGS[v.id]).every(v => CONTROL_BINDINGS[v.id].reason.length));
});
test('rejects invalid run identifiers before making a request', () => {
  assert.throws(() => getJob('../bad'), /실행 ID/);
  assert.throws(() => getMetrics('../bad'), /실행 ID/);
});
