import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseRunPath } from '../server/runFiles.ts';

test('serves only a run status or a generated frame inside the run root', () => {
  assert.deepEqual(parseRunPath('/runs/ui-a0b1/run_status.json?x=1'), {runId:'ui-a0b1',file:'run_status.json'});
  assert.deepEqual(parseRunPath('/runs/myeongdong5044.kGosxO/frames/frame_1400.bin'), {runId:'myeongdong5044.kGosxO',file:'frames/frame_1400.bin'});
  for (const path of ['/runs/../run_status.json','/runs/a..b/run_status.json','/runs/a/frames/../../secrets','/runs/a/agents.parquet','/runs/%2e%2e/run_status.json','/runs/a/frames/frame_14000.bin']) {
    assert.equal(parseRunPath(path), null);
  }
});
