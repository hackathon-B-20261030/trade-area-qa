import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { decodeFrameChunk } from '../src/engine/frames.ts';

const fields = ['body:u32', 'x:i16', 'z:i16', 'heading:u8', 'gaze:u8', 'focal_range_m:u8', 'focal_angle_deg:u8', 'activity:u8', 'flags:u8'];
const header = (changes = {}) => ({
  format: 'mdsim-frames-v1', tick_start: 0, tick_end: 4, dt_s: 0.5,
  render_every: 2, n_frames: 1, record_bytes: 14, origin: [0, 0], pos_scale_m: 0.05, fields,
  ...changes,
});
function frame(tick, records = []) {
  const bytes = Buffer.alloc(8 + records.length * 14);
  bytes.writeUInt32LE(tick, 0);
  bytes.writeUInt32LE(records.length, 4);
  records.forEach((r, i) => {
    const at = 8 + i * 14;
    bytes.writeUInt32LE(r.body, at);
    bytes.writeInt16LE(r.x, at + 4);
    bytes.writeInt16LE(r.z, at + 6);
    [r.heading, r.gaze, r.range, r.angle, r.activity, r.flags].forEach((v, j) => { bytes[at + 8 + j] = v; });
  });
  return bytes;
}
function chunk(h, ...frames) {
  return Buffer.concat([Buffer.from(`${JSON.stringify(h)}\n`), ...frames]);
}

test('reads a real Rust synthetic fixture without eagerly decoding every record', () => {
  const bytes = readFileSync(new URL('./fixtures/frame_1400.bin', import.meta.url));
  assert.equal(createHash('sha256').update(bytes).digest('hex'), '42e7aa3d012c21cab974c8b43cdf8c25ad3bf91883b82e708768ae5a5a6d18d6');
  const result = decodeFrameChunk(bytes);
  assert.equal(result.frameCount, 600);
  assert.deepEqual(result.readFrame(0), { tick: 0, records: [] });
  assert.deepEqual(result.readFrame(4), { tick: 8, records: [{
    body: 0, x_m: 3987 * 0.05, z_m: 666 * 0.05,
    heading_rad: 185 * Math.PI * 2 / 256, gaze_rad: 0,
    focal_range_m: 10, focal_angle_deg: 60, activity: 1, flags: 1,
  }] });
  assert.equal(result.readFrame(599).tick, 1198);
});

test('decodes signed positions, angles and flags', () => {
  const bytes = chunk(header({ tick_start: 2 }), frame(2, [{ body: 4294967295, x: -250, z: 100, heading: 64, gaze: 128, range: 35, angle: 75, activity: 6, flags: 0x1f }]));
  const result = decodeFrameChunk(bytes);
  assert.deepEqual(result.readFrame(0), { tick: 2, records: [{
    body: 4294967295, x_m: -12.5, z_m: 5,
    heading_rad: Math.PI / 2, gaze_rad: Math.PI,
    focal_range_m: 35, focal_angle_deg: 75, activity: 6, flags: 0x1f,
  }] });
});

test('handles empty chunks, empty frames and invalid indices', () => {
  const empty = decodeFrameChunk(chunk(header({ tick_end: 0, n_frames: 0 })));
  assert.equal(empty.frameCount, 0);
  assert.throws(() => empty.readFrame(0), RangeError);
  const one = decodeFrameChunk(chunk(header({ tick_end: 2 }), frame(0)));
  assert.deepEqual(one.readFrame(0), { tick: 0, records: [] });
  for (const index of [-1, 0.5, 1, NaN]) assert.throws(() => one.readFrame(index), RangeError);
});

test('rejects malformed, truncated, inconsistent and unsupported chunks', () => {
  const validRecord = { body: 1, x: 1, z: 2, heading: 1, gaze: 2, range: 3, angle: 4, activity: 1, flags: 0 };
  const full = chunk(header({ tick_end: 2 }), frame(0, [validRecord]));
  const invalid = [
    Buffer.from('no header'), Buffer.from('{bad json}\n'),
    chunk(header({ format: 'v2' }), frame(0)),
    chunk(header({ record_bytes: 13 }), frame(0)),
    chunk(header({ pos_scale_m: 1 }), frame(0)),
    chunk(header({ origin: [1, 0] }), frame(0)),
    chunk(header({ fields: [...fields].reverse() }), frame(0)),
    chunk(header({ dt_s: 0 }), frame(0)),
    chunk(header({ render_every: 0 }), frame(0)),
    chunk(header({ tick_start: -1 }), frame(0)),
    chunk(header({ tick_start: 3, tick_end: 2 }), frame(0)),
    chunk(header({ n_frames: -1 }), frame(0)),
    chunk(header({ origin: [0] }), frame(0)),
    chunk(header({ n_frames: 2 }), frame(0)),
    chunk(header({ n_frames: 2 }), frame(2), frame(0)),
    chunk(header({ tick_end: 6, n_frames: 2 }), frame(0), frame(4)),
    chunk(header(), frame(1)),
    chunk(header(), frame(4)),
    full.subarray(0, full.length - 1),
    Buffer.concat([chunk(header(), frame(0)), Buffer.from([1])]),
  ];
  for (const bytes of invalid) assert.throws(() => decodeFrameChunk(bytes));
});

test('uses the byte offset of a Uint8Array slice', () => {
  const original = chunk(header({ tick_end: 2 }), frame(0));
  const surrounded = Buffer.concat([Buffer.from([1, 2, 3]), original, Buffer.from([4, 5])]);
  const result = decodeFrameChunk(new Uint8Array(surrounded.buffer, surrounded.byteOffset + 3, original.length));
  assert.deepEqual(result.readFrame(0), { tick: 0, records: [] });
  const standalone = original.buffer.slice(original.byteOffset, original.byteOffset + original.length);
  assert.deepEqual(decodeFrameChunk(standalone).readFrame(0), { tick: 0, records: [] });
});
